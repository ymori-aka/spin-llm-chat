use serde::{Deserialize, Serialize};
use spin_sdk::http::{IntoResponse, Method, Request, Response};
use spin_sdk::http_component;
use spin_sdk::variables;

/// Bindings for the two guardrail interfaces. There is no implementation here:
/// Spin composes the Go and Python components in at build time (see
/// `[component.llm-chat.dependencies]`), so `check` is a direct call into
/// another language, not a network request.
mod guards {
    wit_bindgen::generate!({
        path: "wit",
        world: "chat-guards",
        generate_all,
    });
}

use guards::ymori::{injection::guard as injection, pii::guard as pii};

const INDEX_HTML: &str = include_str!("../static/index.html");

#[derive(Deserialize, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatRequest {
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatReply {
    reply: String,
}

#[derive(Serialize)]
struct UpstreamRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct UpstreamResponse {
    choices: Vec<UpstreamChoice>,
}

#[derive(Deserialize)]
struct UpstreamChoice {
    message: UpstreamChoiceMessage,
}

#[derive(Deserialize)]
struct UpstreamChoiceMessage {
    content: String,
}

#[http_component]
async fn handle(req: Request) -> anyhow::Result<impl IntoResponse> {
    match (req.method(), req.path()) {
        (&Method::Get, "/") => Ok(html_response(INDEX_HTML)),
        (&Method::Get, "/api/whereami") => whereami_response(&req).await,
        (&Method::Post, "/api/chat") => chat_response(req).await,
        _ => Ok(error_response(404, "not found")),
    }
}

#[derive(Deserialize)]
struct GeoLookup {
    city: Option<String>,
    region: Option<String>,
    country_name: Option<String>,
    org: Option<String>,
}

/// Reports where this request entered and where it is being served from, so the
/// UI can show the real path rather than a diagram of one.
async fn whereami_response(req: &Request) -> anyhow::Result<Response> {
    let header = |name: &str| {
        req.header(name)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    // Akamai terminates at a "ghost" edge server and tags the hop; its absence
    // means we are running somewhere else (SpinKube, or a local spin up).
    let via = header("via");
    let on_akamai = via.as_deref().is_some_and(|v| v.contains("akamai"))
        || header("cdn-loop").is_some_and(|v| v.contains("akamai"));
    let deployment = variables::get("deployment").unwrap_or_else(|_| "unknown".into());
    let runtime = if on_akamai {
        "akamai functions".to_string()
    } else {
        deployment
    };

    let client_ip = header("true-client-ip")
        .or_else(|| header("x-forwarded-for"))
        .unwrap_or_default();

    let geo = if client_ip.is_empty() {
        None
    } else {
        lookup_geo(&client_ip).await
    };
    let location = match geo {
        Some(g) => {
            let mut parts: Vec<String> = Vec::new();
            if let Some(c) = g.city.filter(|s| !s.is_empty()) {
                parts.push(c);
            }
            if let Some(r) = g.region.filter(|s| !s.is_empty()) {
                parts.push(r);
            }
            if let Some(c) = g.country_name.filter(|s| !s.is_empty()) {
                parts.push(c);
            }
            serde_json::json!({
                "place": parts.join(", "),
                "org": g.org.unwrap_or_default(),
            })
        }
        None => serde_json::json!({ "place": "", "org": "" }),
    };

    let backend_url = variables::get("backend_url").unwrap_or_default();
    let body = serde_json::json!({
        "runtime": runtime,
        "onAkamai": on_akamai,
        "via": via.unwrap_or_default(),
        "clientIp": client_ip,
        "location": location,
        "gateway": backend_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string(),
        "model": variables::get("model").unwrap_or_default(),
    });

    Ok(Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(serde_json::to_string(&body)?)
        .build())
}

/// Best-effort: the panel degrades to just the IP if the lookup is rate limited.
async fn lookup_geo(ip: &str) -> Option<GeoLookup> {
    let request = Request::builder()
        .method(Method::Get)
        .uri(format!("https://ipapi.co/{ip}/json/"))
        .header("user-agent", "spin-llm-chat")
        .build();
    let resp: Response = spin_sdk::http::send(request).await.ok()?;
    if !(200..300).contains(&*resp.status()) {
        return None;
    }
    serde_json::from_slice(resp.body()).ok()
}

async fn chat_response(req: Request) -> anyhow::Result<Response> {
    let chat_req: ChatRequest = match serde_json::from_slice(req.body()) {
        Ok(v) => v,
        Err(e) => return Ok(error_response(400, &format!("invalid request body: {e}"))),
    };

    if chat_req.messages.is_empty() {
        return Ok(error_response(400, "messages must not be empty"));
    }

    // Guardrails run before anything leaves the component: a blocked prompt
    // never reaches the gateway, so it costs no tokens and no round trip.
    let latest = chat_req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or_default();

    // Each guard lives in its own WIT package, so the two `verdict` records are
    // distinct Rust types — check them one at a time rather than in a loop.
    let injection_verdict = injection::check(latest);
    if injection_verdict.blocked {
        return Ok(blocked_response("go", &injection_verdict.reason));
    }
    let pii_verdict = pii::check(latest);
    if pii_verdict.blocked {
        return Ok(blocked_response("python", &pii_verdict.reason));
    }

    let backend_url = variables::get("backend_url")
        .map_err(|e| anyhow::anyhow!("missing backend_url variable: {e}"))?;
    let model =
        variables::get("model").map_err(|e| anyhow::anyhow!("missing model variable: {e}"))?;
    let zuplo_api_key = variables::get("zuplo_api_key")
        .map_err(|e| anyhow::anyhow!("missing zuplo_api_key variable: {e}"))?;
    // Kept low and configurable: the Zuplo Firewall for AI's LLM-DOS-OUT rule
    // rejects completions with 400 once generated output gets too large.
    let max_tokens: u32 = variables::get("max_tokens")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);

    let upstream_req = UpstreamRequest {
        model: &model,
        messages: &chat_req.messages,
        stream: false,
        max_tokens,
        temperature: 0.7,
    };
    let payload = serde_json::to_string(&upstream_req)?;

    // The Zuplo AI Gateway caches /v1/chat/completions by URL only (body-insensitive),
    // so every request needs a unique query param or it replays the first cached answer.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let url = format!(
        "{}/v1/chat/completions?nocache={nonce}",
        backend_url.trim_end_matches('/')
    );
    let upstream_request = Request::builder()
        .method(Method::Post)
        .uri(&url)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {zuplo_api_key}"))
        .header("cache-control", "no-cache, no-store")
        .body(payload)
        .build();

    let upstream_resp: Response = match spin_sdk::http::send(upstream_request).await {
        Ok(r) => r,
        Err(e) => return Ok(error_response(502, &format!("upstream request failed: {e}"))),
    };

    let status = *upstream_resp.status();
    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(upstream_resp.body()).to_string();
        return Ok(error_response(
            502,
            &format!("upstream returned {status}: {text}"),
        ));
    }

    let parsed: UpstreamResponse = match serde_json::from_slice(upstream_resp.body()) {
        Ok(v) => v,
        Err(e) => {
            return Ok(error_response(
                502,
                &format!("failed to parse upstream response: {e}"),
            ))
        }
    };

    let reply = parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default();

    Ok(Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(serde_json::to_string(&ChatReply { reply })?)
        .build())
}

fn html_response(html: &str) -> Response {
    Response::builder()
        .status(200)
        .header("content-type", "text/html; charset=utf-8")
        .body(html)
        .build()
}

/// 200 with a `blocked` payload rather than an error status: from the user's
/// point of view the assistant answered, it just refused.
fn blocked_response(lang: &str, reason: &str) -> Response {
    let body = serde_json::json!({
        "reply": format!("⛔ {reason}"),
        "blocked": true,
        "blockedBy": lang,
    });
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(body.to_string())
        .build()
}

fn error_response(status: u16, msg: &str) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(msg.to_string())
        .build()
}
