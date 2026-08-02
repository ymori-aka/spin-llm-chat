# spin-llm-chat

Rust + [Fermyon Spin](https://spinframework.dev) の HTTP コンポーネント1つで完結する LLM チャットデモ。
静的なチャットUI(`/`)と、Zuplo AI Gateway(Firewall for AI)を経由して LLM を叩く API(`/api/chat`)を同一 Wasm コンポーネントで提供する。

## アーキテクチャ

```
Browser --GET /--------------> Spin component (embedded HTML/JS)
Browser --POST /api/chat-----> Spin component --HTTPS--> Zuplo AI Gateway (Firewall for AI) --> Gemma (OpenAI互換API)
```

- `allowed_outbound_hosts` は `backend_url` 変数1本にロックされており、コンポーネントはそれ以外のホストに一切到達できない([spin.toml](spin.toml))。
- Zuplo AI Gateway 側の `LLM-DOS-IN` / `LLM-DOS-OUT` ルールがリクエスト/レスポンスサイズを制限しているため、`max_tokens` はデフォルト200に抑えている(必要なら変数で調整)。
- Zuplo AI Gateway は `/v1/chat/completions` を URLベースでキャッシュするため、リクエストのたびに `?nocache=<nonce>` を付与してキャッシュミスを強制している。

## 必須の変数

| 変数 | 必須 | 説明 |
|---|---|---|
| `backend_url` | ✅ | Zuplo AI Gateway のベースURL(例: `https://<your-gateway>.zuplo.app`) |
| `zuplo_api_key` | ✅ (secret) | Zuplo AI Gateway の API キー |
| `model` | - | デフォルト `google_gemma-4-26B-A4B-it-Q4_K_M.gguf` |
| `max_tokens` | - | デフォルト `200`(Firewall for AI の LLM-DOS-OUT 対策) |

## ローカル実行 (`spin up`)

```bash
rustup target add wasm32-wasip2
spin build

SPIN_VARIABLE_BACKEND_URL="https://<your-gateway>.zuplo.app" \
SPIN_VARIABLE_ZUPLO_API_KEY="zpka_..." \
spin up --listen 127.0.0.1:3030
```

`http://127.0.0.1:3030` を開く。

## Akamai Functions へのデプロイ (`spin aka`)

```bash
spin plugins install aka --yes
spin aka login

spin aka deploy --build \
  --variable backend_url=https://<your-gateway>.zuplo.app \
  --variable zuplo_api_key=zpka_...
```

## SpinKube (Kubernetes) へのデプロイ

```bash
spin plugins install kube --yes
spin kube scaffold -f spin.toml -o spinapp.yaml
kubectl apply -f spinapp.yaml
```

SpinKube 未導入のクラスタでは事前に `spin-operator` と `containerd-shim-spin` (runtime class) のインストールが必要。
