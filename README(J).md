
# ⚡ arc

**すべての操作に意味を。すべての状態に物語を。**

[![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Flux Core](https://img.shields.io/badge/Powered_by-Flux_Core-blueviolet)](docs/FLUX_CORE.md)

*[Flux Core](docs/FLUX_CORE.md) アーキテクチャのフラッグシップ・ショーケース*

---

> **「ツールは記憶すべきだ。」**

他のツールがシェルの高速化を目指す中、Arc は **エンジンの賢さ** を目指します。
`uv` の速度と、イベントソーシングによる **タイムマシン機能** を融合させた、未来の Ruby パッケージマネージャーです。

</div>

---

## 問題

すべての Ruby 開発者が経験したことがあるはずです：

```
「昨日は動いてたのに。」
「何が変わったの？」
「bundle install を何回か実行した気がするけど…」
```

シェルの履歴は *コマンド* を教えてくれます。Git は *ファイルの差分* を教えてくれます。しかし、どちらも **環境の状態** を教えてくれません — どの gem がインストールされていたか、どの Ruby が動いていたか、何が失敗してなぜ失敗したか。

**arc** がその物語を記録します。すべての操作を。すべての結果を。すべての変化を。

---

## arc とは？

arc は **[Flux Core](#アーキテクチャ-flux-core)** — ターミナルのためのイベントソーシングエンジン — の上に構築された **Ruby プロジェクトマネージャー** です。

`uv` の Ruby 版として設計されています：
- **プロジェクトごとの隔離環境**（gem の競合とは永遠にさようなら）
- **グローバルバイナリキャッシュ**（一度インストールすれば、どこでもリンク — `uv` のハードリンクキャッシュと同じ）
- **爆速の Ruby セットアップ**（プリコンパイル済みバイナリを使用）
- **完全な操作履歴**（`add`、`remove`、`install` がすべて構造化された不変イベントとして記録）

しかし arc は `uv` をさらに超えます：
- **`arc undo`** — `add` や `remove` をコマンド一発で取り消す
- **`arc state --diff`** — 最後の操作から何が変わったかを確認
- **PATH を汚染しない** — arc はシステムの PATH に一切触れない

---

## クイックスタート

```bash
# ソースからビルド
git clone https://github.com/yourname/ruby-arc.git
cd ruby-arc && cargo build --release
cp target/release/arc ~/.local/bin/

# 新しい Ruby プロジェクトを開始
mkdir my_app && cd my_app
arc init .
arc bootstrap          # Ruby 4.0.7 をダウンロード＆リンク（キャッシュから約 0.07 秒）

# gem を管理する
arc add rails
arc add rspec --version "~> 3.0"
arc sync               # バイナリキャッシュ付き bundle install

# コードを実行する
arc run ruby app.rb
arc run rails server

# 何が起きたか確認する
arc state
arc state --diff
```

---

## コマンド一覧

| コマンド | 説明 |
|---|---|
| `arc init [path]` | 新しい Flux プロジェクトを初期化（`.flux/` と `.arc/env/` を作成） |
| `arc bootstrap [version]` | Ruby をプロジェクトにダウンロード＆リンク（グローバルキャッシュ使用） |
| `arc add <gem> [--version]` | Gemfile に gem を追加してインストール |
| `arc remove <gem>` | Gemfile から gem を削除して同期 |
| `arc sync` | Gemfile.lock と環境を同期（`uv sync` 相当） |
| `arc run <cmd> [args...]` | 隔離されたプロジェクト環境でコマンドを実行 |
| `arc x [--ruby V] [--with GEM]... -- <ruby の引数>` | **プロジェクト無しで Ruby を 1 回撃つ**（`uvx` / `npx` 相当）。キャッシュの Ruby を使い、`.arc/` も記録も作らない。`--with` の gem（`name` か `name:version`）は「Ruby の版 × gem の組」ごとにキャッシュして使い回す |
| `arc exec <cmd> [args...]` | Flux ログ付きで任意のコマンドを実行 |
| `arc env` | 現在の環境情報を表示（Ruby パス、GEM_HOME、バージョン） |
| `arc undo` | 最後の `add` または `remove` 操作を取り消す |
| `arc state` | 完全な操作履歴と統計を表示 |
| `arc state --diff` | 最後の操作で何が変わったかを表示 |
| `arc state --json` | 機械可読な JSON 出力（`jq` へのパイプに最適） |

---

## なぜ shim を使わないのか？

`rbenv` や `rvm` は **shim** — `PATH` の先頭に置かれた薄いラッパースクリプト — を使って、すべての `ruby` 呼び出しを横取りします。

**arc はこれを一切行いません。** 理由はこちら：

### shim の問題点

```bash
# rbenv の shim を使う場合：
$ ruby script.rb
# → どの ruby？rbenv のもの？rvm のもの？システムのもの？
# → PATH の順序、.ruby-version の場所、シェルの初期化順序に依存...
# → CI/CD スクリプトでは .bashrc が読まれない → 意図と違う Ruby が使われる
# → VSCode では、ターミナルの PATH と統合ターミナルの PATH が異なる
# → rbenv + rvm を同時に使う？公式が「絶対にやるな」と言っている
```

shim は **暗黙的** です。うまく動いているときは魔法のように見えます。壊れたときは地獄です。

### arc の方法

```bash
# arc を使う場合：
arc run ruby script.rb
# → 常に .arc/env/ruby_runtime/bin/ruby を使用
# → PATH の操作なし。シェルフックなし。サプライズなし。
# → ターミナル、CI/CD、cron、VSCode、Docker で同一の動作
```

> **「arc はあなたの PATH に一切触れません。何が実行されるかは常に見えています。」**

これは `uv` が選んだ哲学と同じです：shim で管理された `python` に頼るのではなく、`uv run python script.py` を使う。

---

## グローバルバイナリキャッシュ

arc は `~/.arc/cache/` に **グローバルキャッシュ** を使用します — すべてのプロジェクト間で共有されます。

```
~/.arc/cache/
  rubies/
    3.3.6-linux-x86_64/   ← 一度ダウンロードして、すべてのプロジェクトにリンク
  gems/
    gems/                  ← ハードリンクで共有されたコンパイル済み gem
    specifications/
    extensions/            ← C 拡張バイナリ（再コンパイル不要）
```

### 仕組み

```bash
# プロジェクト A：初回
arc bootstrap    # Ruby 4.0.7 をダウンロード → 約 30 秒
arc add nokogiri # C 拡張をコンパイル → 約 20 秒

# プロジェクト B：同じバージョン
arc bootstrap    # キャッシュからハードリンク → 0.07 秒 ⚡
arc sync         # キャッシュから nokogiri を復元 → 即座 ⚡
```

これは `uv` が伝説的な速度を実現している方法と同じです — ハードリンクは **コピーのオーバーヘッドゼロ**、**ディスクの重複ゼロ** を意味します。

---

## アーキテクチャ: Flux Core

arc は **Flux Core** の上に構築されています — CLI ツールのための汎用イベントソーシングエンジンです。

```
┌─────────────────────────────────────────────────────┐
│                      arc CLI                         │
│  ┌──────┐  ┌──────┐  ┌──────┐  ┌───────┐  ┌──────┐ │
│  │ init │  │ add  │  │ sync │  │ state │  │ undo │ │
│  └──┬───┘  └──┬───┘  └──┬───┘  └───┬───┘  └──┬───┘ │
│     │         │          │          │          │     │
├─────┴─────────┴──────────┴──────────┴──────────┴────┤
│                   Flux Core エンジン                  │
│  ┌─────────────────┐  ┌──────────────────────────┐  │
│  │  FluxProject    │  │  Signal (Flux v1 NDJSON)  │  │
│  │  .init()        │  │  - v: 1                   │  │
│  │  .open()        │  │  - source: "arc"          │  │
│  │  .record()      │  │  - type / event           │  │
│  │  .read()        │  │  - ts: RFC 3339           │  │
│  └─────────────────┘  └──────────────────────────┘  │
├─────────────────────────────────────────────────────┤
│              .flux/signals.jsonl                     │
│           （追記専用 NDJSON ログ）                    │
└─────────────────────────────────────────────────────┘
```

### Signal とは？

arc が実行するすべての操作は、1つ以上の **Signal** を発行します — 追記専用ログに書き込まれる、構造化された不変のイベントです。1 行が Flux Protocol v1 の NDJSON レコード 1 件にあたります。

```json
{"v":1,"source":"arc","type":"gem","event":"added","ts":"2026-08-22T07:30:15.177395+00:00","id":"019c70b2-5c33-7d03-9911-112233445566","gem":"rails","version":"~> 8.0"}
{"v":1,"source":"arc","type":"cmd","event":"run","ts":"2026-08-22T07:30:15.186032+00:00","id":"019c70b2-3a11-7f01-b178-bd89a26ed073","cmd":"bundle install","args":["install"],"cwd":"/proj","verb":"sync","env_mode":"isolated"}
{"v":1,"source":"arc","type":"cmd","event":"end","ts":"2026-08-22T07:30:19.386032+00:00","id":"019c70b2-3a11-7f01-b178-bd89a26ed073","cmd":"bundle install","exit_code":0,"ms":4200,"success":true,"verb":"sync","env_mode":"isolated"}
```

主な特性：
- **必須 4 フィールド** — `v` / `source` / `type` / `event`。残りはすべて payload で、同じ行にトップレベルで並ぶ。
- **`type` は名詞、`event` はそれに起きたこと** — `gem:added`, `cmd:run`, `session:end`。小文字ケバブケースで、この組が Signal Registry の鍵になる。
- **UUID v7 の `id`** — 時刻順にソート可能、グローバルに一意。
- **同じ `id` で相関する** — `cmd:run` と `cmd:end` は**同じ** `id` を持つ。終端が始端を指すのではなく、両者が同じ鍵を名乗る。
- **`ts` は出来事が起きた時刻** — RFC 3339 + 明示的オフセット、生産者の時計（`PROTOCOL_v1.md` §2.05）。
- **追記専用** — Signal は変更・削除されない。

> **古いログを読むこと。** 2026-08-22 より前に書かれた journal は arc の
> プロトコル以前の形（`{id, type, payload, timestamp}`・`add_start` 等の綴り）です。
> arc はそれを透過的に読みます —— 旧 `type` を `type:event` の組に翻訳し、時刻は
> `ts ?? modified ?? timestamp` の順で取ります（プロトコルがまさにこの移行のために
> 定めたフォールバック順）。**改名は歴史を直さない**ので、これは一時的な措置ではありません。

### なぜイベントソーシングなのか？

従来のツールは状態を変更して履歴を捨てます。Flux Core はすべての操作を、特定の時刻に起きた **事実** として扱います。

これにより以下が可能になります：

| 機能 | 仕組み |
|---|---|
| `arc undo` | Signal ログを逆走して最後の `gem:added`/`gem:removed` を見つけ、取り消す |
| `arc state --diff` | 連続する Signal に保存された Gemfile のスナップショットを比較 |
| `arc state` 統計 | コマンドライン別に `cmd:end` Signal を集計し、成功/失敗を数える |
| 将来: リプレイ | 新しいマシンで Signal を再発行して環境を再現 |
| 将来: AI 監査 | Signal ログを LLM に渡して、何が起きてなぜ起きたかを説明させる |

### Flux Core はスタンドアロンクレートとして

Flux Core はスタンドアロンの `flux-core` クレートとして抽出されるよう設計されています — arc だけでなく、**あらゆる Rust CLI ツール** が使用できます。

```rust
// どんな CLI ツールでも Flux Core を使える：
let project = FluxProject::open(&cwd)?;
project.record(SignalType::Custom, json!({ "action": "deploy", "env": "production" }))?;
```

---

## 比較

### vs. rvm / rbenv

| | rvm | rbenv | arc |
|---|:---:|:---:|:---:|
| Ruby バージョン管理 | ✅ | ✅ | ✅（プロジェクト単位） |
| Gem 管理 | ❌ | ❌ | ✅ |
| グローバルバイナリキャッシュ | ❌ | ❌ | ✅ |
| Gem バイナリキャッシュ | ❌ | ❌ | ✅ |
| 操作履歴 | ❌ | ❌ | ✅ |
| Undo | ❌ | ❌ | ✅ |
| PATH 汚染 | ✅（重い） | ✅（shim） | ❌（なし） |
| 実装言語 | Bash | Bash | Rust |

### vs. rv（uv にインスパイアされた Ruby マネージャー）

| | rv | arc |
|---|:---:|:---:|
| Ruby バージョン管理 | ✅ 複数バージョン | ✅ プロジェクト単位 |
| Gem 管理 | ❌ | ✅ |
| Gem バイナリキャッシュ | ❌ | ✅ |
| 操作履歴 | ❌ | ✅ |
| Undo | ❌ | ✅ |
| Shim | ✅ | ❌（設計上の選択） |
| 思想 | 「rbenv を速くした」 | 「uv の Ruby 版」 |

### vs. uv（Python）

| | uv | arc |
|---|:---:|:---:|
| グローバルバイナリキャッシュ | ✅ | ✅ |
| ハードリンク共有 | ✅ | ✅ |
| 隔離環境 | ✅ | ✅ |
| `add` / `remove` / `sync` | ✅ | ✅ |
| 操作履歴 | ❌ | ✅ |
| Undo | ❌ | ✅ |
| 複数バージョン管理 | ✅ | ⚠️（ロードマップ） |

> **arc は `uv` の Ruby 版であり、`uv` が持っていないイベントソーシングエンジンを備えています。**

---

## プロジェクト構成

```
my_project/
├── Gemfile
├── Gemfile.lock
├── .flux/
│   ├── signals.jsonl    ← 追記専用の操作ログ（Flux Core）
│   └── config.toml      ← arc の設定
└── .arc/
    └── env/
        ├── ruby_runtime/ ← ~/.arc/cache/rubies/ からリンク
        ├── bin/          ← gem の実行ファイル
        ├── gems/         ← インストール済み gem
        └── ...
```

```toml
# .flux/config.toml
[ruby]
version = "3.3.6"
```

Ruby バージョンはいつでも変更できます：
```bash
arc bootstrap 3.4.0   # config.toml を更新して Ruby を再リンク
```

---

## 設計哲学

> **「ツールは記憶すべきだ。」**

AI エージェントが数秒で何十ものコマンドを実行する時代に生きています。シェルの履歴では不十分です。Git の差分は *プロセス* を捉えられません。

arc はそのギャップを埋めます — すべてのターミナル操作に **アイデンティティ**、**構造**、**永続性** を与えます。

arc の設計を導く 3 つの原則：

1. **明示的 > 暗黙的** — shim の代わりに `arc run ruby`。何が実行されるかは常に明確。
2. **すべてを記録する** — すべての操作は Signal。何も失われない。
3. **プロジェクト完結型** — すべてが `.arc/env/` と `.flux/` に収まる。リポジトリをクローンして `arc bootstrap && arc sync` を実行すれば完了。

---

## 技術スタック

- **言語**: Rust（2024 エディション）
- **CLI フレームワーク**: clap v4
- **シリアライゼーション**: serde + serde_json + toml
- **ID**: UUID v7（時刻順ソート可能、単調増加）
- **ログ形式**: NDJSON（改行区切り JSON）
- **Ruby ソース**: [ruby/ruby-builder](https://github.com/ruby/ruby-builder) プリコンパイル済みバイナリ

---

## ロードマップ

| フェーズ | 状態 | 説明 |
|---|---|---|
| **1. コアエンジン** | ✅ 完了 | Flux Core、Signal 記録、FluxProject API |
| **2. Ruby ブートストラップ** | ✅ 完了 | グローバルキャッシュ、ハードリンク共有、`arc bootstrap` |
| **3. Gem 管理** | ✅ 完了 | `add`、`remove`、`sync`、gem バイナリキャッシュ |
| **4. Undo と Diff** | ✅ 完了 | `arc undo`、`arc state --diff` |
| **5. 設定** | ✅ 完了 | `.flux/config.toml`、動的 Ruby バージョン |
| **6. 複数バージョン** | 📋 計画中 | 既存バージョンと並行した `arc bootstrap 3.4.0` |
| **7. タイムマシン機能 (v2)** | 🚀 計画中 | `arc checkout <id>` / `arc reset` / 環境の完全リプレイ |
| **8. Windows サポート** | ✅（2026-10-11）| `bootstrap` が RubyInstaller の持ち運べる `.7z` を取る。`sync` / `run` / `undo` は動く。C 拡張をコンパイルする gem には `arc bootstrap --devkit`（MSYS2 ＋ MINGW toolchain・約 1GB をキャッシュの Ruby の中へ）。Pharos（puma・io-event・bigdecimal 等）が Ruby 4.0.7 で通った |
| **9. flux-core クレート** | � 計画中 | Flux Core をスタンドアロンクレートとして抽出 |
| **10. macOS サポート** | 📋 計画中 | ARM64 バイナリサポート |

---

## ❤️ プロジェクト支援

Arc と Flux Core は、「環境構築で時間を溶かす時代」を終わらせるための挑戦です。

もしこの **「ツールは記憶すべきだ (Flux Philosophy)」** というビジョンに共感していただけたら、ぜひ支援をご検討ください。
いただいた支援は、主に **Windows 版開発のための機材購入** やテスト環境の整備に使われます。

[**GitHub Sponsors**](#) | [**Buy Me a Coffee**](#)

---

## ライセンス

MIT

---

<div align="center">

*[Flux Core](docs/FLUX_CORE.md) の上に構築 — ターミナルのためのイベントソーシング。*

**arc** — すべての操作に意味がある。すべての状態が物語を語る。

</div>
