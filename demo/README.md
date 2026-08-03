# デモ手順

同じ 1 つの Wasm コンポーネントを 3 つの実行環境へ順に配る流れを見せる。
ビルドのたびにバージョンとアクセント色が自動で変わるので、**どこまで新版が行き渡ったかが画面の色で分かる**。

## 事前準備 (デモ開始前に済ませておく)

```bash
cd llm-chat
source demo/env.sh          # ZUPLO_KEY を読み込む。demo/env.sh.example を参照
kubectl config use-context spinkube-demo
```

ツールチェーンの確認(初回のみ):

```bash
spin --version && tinygo version && kubectl get nodes
```

## 画面構成

| 場所 | 内容 |
|---|---|
| ターミナル A | コマンドを打つ(下記の3ステップ) |
| ターミナル B | `./demo/watch.py` を流しっぱなしにする |
| ブラウザ | SpinKube と Akamai Functions の URL を 2 タブ開いておく |

ターミナル B は 5 秒ごとに 3 環境を叩き、それぞれが返す **version / git SHA / アクセント色** を並べる。
新版が行き渡ると行の色が順に変わる。

## ステップ 1 — `spin up` (ローカル)

```bash
spin build
spin up
```

- ターミナル B の `spin up` 行に新しい version が出る
- `http://127.0.0.1:3030` を開くと、上部の帯・タイトル・バッジが**新しい色**になっている
- ここで遮断デモを1回見せておくと、後の2環境では「同じものが動いている」だけで済む
  - 「インジェクション (Go が遮断)」→ 赤いバブル
  - 「カード番号 (Python が遮断)」→ 赤いバブル

> このとき **spinkube と spin aka の行はまだ古い色のまま**。ここが後の対比になる。

## ステップ 2 — SpinKube (Kubernetes)

```bash
spin registry push ghcr.io/ymori-aka/spin-llm-chat:latest
kubectl rollout restart deploy/spin-llm-chat
kubectl rollout status deploy/spin-llm-chat
```

- ロールアウト完了と同時に、ターミナル B の `spinkube` 行が新しい色に変わる
- ブラウザの SpinKube タブを再読込すると色が変わる(実行場所 = `spinkube`)

## ステップ 3 — `spin aka deploy` (Akamai Functions)

```bash
spin aka deploy --variable zuplo_api_key=$ZUPLO_KEY
```

> **`--build` を付けないこと。** 付けると再ビルドされて build id が変わり、
> Akamai だけ別バージョン・別色になってしまう。ステップ1で作った成果物を
> そのまま配ることで、3環境の version と色が完全に一致する。

- 反映まで 1〜2 分かかる。その間に「同じ成果物がエッジへ配られている」話をする
- 反映されると `spin aka` 行が新しい色になり、3行すべてが同じ色で揃う
- ブラウザの Akamai タブでは **実行場所が `akamai functions`、アクセス元に都市名**が出る

> `spin aka deploy` が `Deployment did not go live within 60 seconds` を返すことがあるが、
> 実際には反映されている。ターミナル B を見て待てばよい。

## 話す順番の目安

1. **1つの成果物** — Rust + Go + Python が 1 つの `.wasm` に合成されている(`spin build` の出力に3つのコンポーネントが出る)
2. **同じものが3か所で動く** — ローカル / Kubernetes / エッジ。再ビルドも書き換えもしていない
3. **ガードはエッジで効く** — 遮断されたプロンプトはゲートウェイにも LLM にも到達しない
4. **色とバージョン** — デプロイのたびに自動で変わるので、反映漏れがひと目で分かる

## 色を固定したいとき

環境ごとに色を決め打ちしたい場合は `accent` を渡す(自動生成より優先される)。

```bash
spin aka deploy --build \
  --variable zuplo_api_key=$ZUPLO_KEY \
  --variable version=v2 \
  --variable accent='#ff6b6b'
```
