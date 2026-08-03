# spin-llm-chat

[Fermyon Spin](https://spinframework.dev) の LLM チャットデモ。**Rust + Go + Python の3言語を WebAssembly Component Model で1つの Wasm コンポーネントに合成**している。
`spin up` / SpinKube / Akamai Functions の3環境すべてで同一成果物が動作する。

## アーキテクチャ

```
                 ビルド時に合成 (spin build)
   Rust (HTTPハンドラ) ─┬─ WIT ─→ Go     : プロンプトインジェクション検査
                        └─ WIT ─→ Python : PII / カード番号検査
                              ↓
                    1つの .wasm コンポーネント
                              ↓
Browser → Akamai Edge → [guards] ⛔遮断 or 通過 → Zuplo AI Gateway → Gemma
```

ガードは**外に出る前**に走るので、遮断されたプロンプトはゲートウェイにも LLM にも到達しない(トークン消費もラウンドトリップもゼロ)。

### Component Model の要点

- `check` は **HTTP 呼び出しではない**。`[component.llm-chat.dependencies]` により `spin build` が3つの `.wasm` を1つに合成するので、言語をまたいだ直接関数呼び出しになる。
- 各言語は [WIT](wit/) で定義した契約だけを共有する。実装言語を差し替えても Rust 側は無変更。
- ガードは純粋関数なので `variables` も `allowed_outbound_hosts` も持たない(Spin の仕様上、依存コンポーネントは Spin ランタイム設定を持てない)。
- デプロイされる成果物は単一コンポーネントなので、Akamai Functions にもそのまま乗る。

| 言語 | 役割 | ツールチェーン | サイズ |
|---|---|---|---|
| Rust | HTTP ハンドラ | `cargo` + `wit-bindgen` | — |
| Go | インジェクション検査 | TinyGo + `wit-bindgen-go` | 約 384 KB |
| Python | PII 検査 | `componentize-py` | 約 18 MB (CPython 同梱) |

⚠️ **ガードごとに WIT パッケージを分けること。** 2つのコンポーネントが同じパッケージのインターフェースをエクスポートすると、合成時に `package ymori:... has already been registered in the graph` で失敗する。

### その他

- `allowed_outbound_hosts` は `backend_url` と geo 参照用の `ipapi.co` の2ホストだけに制限されており、コンポーネントはそれ以外に一切到達できない([spin.toml](spin.toml))。
- Zuplo AI Gateway 側の `LLM-DOS-IN` / `LLM-DOS-OUT` ルールがリクエスト/レスポンスサイズを制限しているため、`max_tokens` はデフォルト200に抑えている(必要なら変数で調整)。
- Zuplo AI Gateway は `/v1/chat/completions` を URLベースでキャッシュするため、リクエストのたびに `?nocache=<nonce>` を付与してキャッシュミスを強制している。

### WASI のバージョンについて

**Akamai Functions は WASI 0.3 (Spin 4 の既定) に未対応**なので、このアプリは WASI 0.2 (`wasm32-wasip1` / spin-sdk 5.x / `#[http_component]`) で書いてある。
Spin 4 の `#[http_service]` + `wasm32-wasip2` で書くと `spin up` と SpinKube では動くが、`spin aka deploy` が次のエラーで落ちる:

```
Error: This app requires feature(s) that are not yet available in Akamai Functions:
* Component llm-chat uses the WASI 0.3 HTTP handler interface
```

`spin new` する場合は `http-rust` ではなく **`http-rust-p2`** テンプレートを使うこと。

## 必須の変数

| 変数 | 必須 | 説明 |
|---|---|---|
| `backend_url` | ✅ | Zuplo AI Gateway のベースURL(例: `https://<your-gateway>.zuplo.app`) |
| `zuplo_api_key` | ✅ (secret) | Zuplo AI Gateway の API キー |
| `model` | - | デフォルト `google_gemma-4-26B-A4B-it-Q4_K_M.gguf` |
| `max_tokens` | - | デフォルト `200`(Firewall for AI の LLM-DOS-OUT 対策) |
| `deployment` | - | `/api/whereami` が返す実行環境名。デフォルト `local` |

## エンドポイント

| パス | 内容 |
|---|---|
| `GET /` | チャット UI(遮断デモ用のワンクリックボタン付き) |
| `POST /api/chat` | ガード通過後に LLM へ。遮断時は `{"blocked":true,"blockedBy":"go"\|"python"}` |
| `GET /api/whereami` | 実行環境・Akamai エッジ経由か・クライアント IP と地名・ゲートウェイ |

## 必要なツールチェーン

```bash
rustup target add wasm32-wasip1
brew install tinygo binaryen wasm-tools     # Go コンポーネント用
go install go.bytecodealliance.org/cmd/wit-bindgen-go@latest
# Python 側の componentize-py は spin build が venv を作って自動導入する
```

## ローカル実行 (`spin up`)

```bash
spin build

SPIN_VARIABLE_BACKEND_URL="https://<your-gateway>.zuplo.app" \
SPIN_VARIABLE_ZUPLO_API_KEY="zpka_..." \
spin up --listen 127.0.0.1:3030
```

`http://127.0.0.1:3030` を開く。

## Akamai Functions へのデプロイ (`spin aka`)

```bash
spin plugins install aka --yes
spin aka login   # ブラウザ認証。PAT は最長90日で失効する

spin aka deploy --build --no-confirm \
  --create-name llm-chat \
  --variable backend_url=https://<your-gateway>.zuplo.app \
  --variable zuplo_api_key=zpka_...
```

2回目以降は `--create-name` 不要(workspace がアプリに紐づく)。非対話環境では `--no-confirm` が必須。

## SpinKube (Kubernetes) へのデプロイ

```bash
spin plugins install kube --yes
spin kube scaffold -f spin.toml -o spinapp.yaml
kubectl apply -f spinapp.yaml
```

### クラスタ準備 (kind、検証済み手順)

`containerd-shim-spin` 入りの kind イメージでクラスタを作る。

```bash
kind create cluster --name spin-llm-chat --image ghcr.io/spinframework/containerd-shim-spin/kind:v0.25.1 --config=- <<'EOF'
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
containerdConfigPatches:
- |-
  [plugins."io.containerd.cri.v1.runtime".containerd.runtimes.spin]
    runtime_type = "io.containerd.spin.v2"
  [plugins."io.containerd.cri.v1.runtime".containerd.runtimes.spin.options]
    SystemdCgroup = true
EOF

kubectl apply -f https://github.com/cert-manager/cert-manager/releases/download/v1.20.0/cert-manager.yaml
kubectl wait --for=condition=available --timeout=300s deployment/cert-manager-webhook -n cert-manager

kubectl apply -f https://github.com/spinframework/spin-operator/releases/download/v0.6.1/spin-operator.runtime-class.yaml
kubectl apply -f https://github.com/spinframework/spin-operator/releases/download/v0.6.1/spin-operator.crds.yaml

helm upgrade --install spin-operator \
  --namespace spin-operator --create-namespace \
  --version 0.6.1 --wait \
  oci://ghcr.io/spinframework/charts/spin-operator

kubectl apply -f https://github.com/spinframework/spin-operator/releases/download/v0.6.1/spin-operator.shim-executor.yaml
```

> ghcr.io の pull が `denied` になる場合、古い認証情報が残っていることがある。`docker logout ghcr.io` で匿名 pull に戻せる。

### アプリの配置

アプリを OCI レジストリに push し、APIキーは Secret 経由で渡す。

```bash
gh auth token | spin registry login ghcr.io -u <user> --password-stdin
spin build && spin registry push ghcr.io/<user>/spin-llm-chat:latest

# private パッケージを pull するための secret
kubectl create secret docker-registry ghcr-pull \
  --docker-server=ghcr.io --docker-username=<user> \
  --docker-password="$(gh auth token)"

# APIキーは YAML に平文で書かず Secret から参照する
kubectl create secret generic zuplo-api-key --from-literal=api-key='zpka_...'

kubectl apply -f spinapp.yaml
kubectl port-forward svc/spin-llm-chat 8084:80
```

[spinapp.yaml](spinapp.yaml) は `zuplo_api_key` を `secretKeyRef` で参照しているので、秘密情報はリポジトリに入らない。
