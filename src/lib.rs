use serde::{Deserialize, Serialize};
use spin_sdk::http::{IntoResponse, Method, Request, Response};
use spin_sdk::http_component;
use spin_sdk::variables;

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
        (&Method::Post, "/api/chat") => chat_response(req).await,
        _ => Ok(error_response(404, "not found")),
    }
}

async fn chat_response(req: Request) -> anyhow::Result<Response> {
    let chat_req: ChatRequest = match serde_json::from_slice(req.body()) {
        Ok(v) => v,
        Err(e) => return Ok(error_response(400, &format!("invalid request body: {e}"))),
    };

    if chat_req.messages.is_empty() {
        return Ok(error_response(400, "messages must not be empty"));
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

fn error_response(status: u16, msg: &str) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(msg.to_string())
        .build()
}
