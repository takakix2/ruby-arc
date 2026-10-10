# Flux Core

**汎用操作ログ記録・再生エンジン**

> "すべての操作に意味を。すべての状態に物語を。"

---

## 概要

Flux Core は、CLI ツールやプロジェクト管理において **すべての操作を構造化されたイベント（Signal）として記録し、任意の時点の状態（State）を再構築する** ためのエンジンです。

Event Sourcing パターンを開発ツールの世界に持ち込み、「何が起きたか」ではなく「なぜその状態になったか」を追跡可能にします。

### 既存ツールとの違い

| 特性 | シェル履歴 | git log | **Flux Core** |
|---|---|---|---|
| 構造化データ | ❌ | △ (コミットメッセージのみ) | ✅ JSON Value |
| 任意ペイロード | ❌ | ❌ | ✅ |
| 状態の再構築 | ❌ | △ (ファイル差分のみ) | ✅ State Machine |
| 言語非依存 | ✅ | ✅ | ✅ |
| プログラマブル | ❌ | △ | ✅ Rust API / NDJSON |

---

## 哲学

### 1. Signal-First Architecture

操作の「結果」ではなく「意図」を記録します。

```
❌ 従来: コマンドを実行 → 結果を確認 → 失敗したら調査
✅ Flux:  コマンドを記録 → 実行 → 結果を記録 → 状態を更新
```

### 2. Append-Only Log

Signal ログ (`signals.jsonl`) は **追記のみ** です。削除・変更はしません。
これにより、完全な監査証跡と時間旅行（任意時点の状態再現）が可能になります。

### 3. Language Agnostic

Flux Core は特定の言語やエコシステムに依存しません。
Ruby, Python, Node.js, Go, Rust — どのプロジェクトでも `.flux/signals.jsonl` を置くだけで使えます。

---

## Signal 仕様 — 正本は Flux Protocol v1

🚨 **この節はもう仕様ではない。** Signal の形を決めているのは
`flux-protocol` の
`spec/PROTOCOL_v1.md` と `schema/flux-envelope.v1.schema.json` で、arc はその
**消費者**である。ここに写しを置くと必ず片方が腐るので、以下は「arc が何を出すか」
だけを書き、綴りの根拠は仕様に譲る。

> **系譜**: この文書が Flux Protocol の親にあたる。2026-02 に arc が
> `{id, type, payload, timestamp}` という形を定め、その ROADMAP Phase 4 に
> 「Flux Protocol v1 specification document」を deliverable として挙げていた。
> それが独立リポになったのが今の `flux-protocol` で、**独立した先で envelope が
> 変わった**。arc は 2026-08-22 にその子の形へ移った。

### envelope（必須 4 フィールド）

```json
{"v":1,"source":"arc","type":"cmd","event":"end","ts":"2026-08-22T07:30:15.188368+00:00","id":"…","cmd":"bundle install","exit_code":0,"ms":4200}
```

| フィールド | 説明 |
|---|---|
| `v` | envelope バージョン。v1 では常に `1` |
| `source` | 生産者。arc のレコードは常に `"arc"` |
| `type` | 主語の分類（**名詞**・`^[a-z0-9-]+$`） |
| `event` | 主語に起きたこと（**過去分詞/状態名**・`^[a-z0-9-]+$`） |

残りはすべて payload で、**入れ子にせずトップレベルに平らに並べる**。

⚠️ `type`/`event` にアンダースコアは使えない。旧 arc の `add_start` /
`shell_enter` はこれに反していた。
自由形式の種別を作る口（旧 `SignalType::Custom`）を塞いであるのはそのため。

### 時刻 — `ts`

`ts` は**出来事が起きた時刻**を生産者の時計で書いたもの。RFC 3339 + 明示的オフセット。
arc は UTC・マイクロ秒で書く。詳細と `rx_ts` / `mtime` との違いは
`PROTOCOL_v1.md` §2.05。

⚠️ **精度は生産者ごとに違う**ので、複数の生産者を混ぜたストリームを時刻だけで
並べてはいけない（同着しうる）。

### arc が出す Signal

| `type:event` | いつ | 主な payload |
|---|---|---|
| `project:created` | `arc init` | `path`, `version`, `ruby_version` |
| `cmd:run` | 子プロセスの起動 | `cmd`, `args`, `cwd`, `verb`, `env_mode` |
| `cmd:end` | 子プロセスの終了 | `cmd`, `exit_code`, `ms`, `success`, `verb` |
| `gem:added` | `arc add` | `gem`, `version` |
| `gem:removed` | `arc remove` | `gem` |
| `runtime:linked` | `arc bootstrap` | `ruby_version`, `cache_hit`, `dest` |
| `operation:reverted` | `arc undo` | `target_id`, `target_type`, `gem` |
| `session:start` | `arc shell` に入る | `sid`, `shell`, `cwd` |
| `session:end` | `arc shell` を出る | `sid`, `exit_code`, `how` |

- `verb` は同じ `cmd:*` を出す 3 つの入口（`exec` / `sync` / `run`）を区別する。
- `cmd:run` と `cmd:end` は**同じ `id`** を持つ。終端が始端を指すのではなく、
  両者が同じ鍵を名乗る。
- `cmd` は 1 本のコマンドライン、`args` は argv の忠実な形。空白を含む引数は
  `cmd` では潰れるので、機械が読むなら `args` を引く。

### 旧形式（2026-02〜2026-08）

`{id, type, payload, timestamp}` に `add_start` のような綴りで書かれていた。
**改名は歴史を直さない**ので、arc の読み取り側が翻訳して吸収する:

- 旧 `type` → `type:event`（未知の綴りは `legacy:recorded` に落とし、元の文字列を
  `legacy_type` として payload に残す）
- 入れ子の `payload` → トップレベルに展開
- 時刻は `ts ?? modified ?? timestamp` の順で最初に在るものを取る（§2.05.3）
- 終端の `ref_id` は相関鍵としてそのまま使う

---

## ストレージ

### ディレクトリ構造

```
my_project/
├── .flux/                    # Flux Core のデータディレクトリ
│   ├── signals.jsonl         # Signal ログ（追記のみ、NDJSON形式）
│   └── state.json            # 最新の State スナップショット（将来実装）
├── src/
└── ...
```

> **注**: 現在の `arc` 実装では `.arc/` を使用していますが、
> 独立クレートとしては `.flux/` を標準ディレクトリとします。

### signals.jsonl

- **形式**: NDJSON (Newline Delimited JSON)
- **エンコーディング**: UTF-8
- **書き込み**: Append-Only (追記のみ)
- **読み込み**: 行単位でパース可能（ストリーミング対応）

1行が1つの Signal に対応します。

```
{"v":1,"source":"arc","type":"project","event":"created","ts":"2026-08-22T07:30:15.177395+00:00","id":"…","path":"my_project","version":"0.1.0"}
{"v":1,"source":"arc","type":"cmd","event":"run","ts":"2026-08-22T07:30:15.186032+00:00","id":"019c…","cmd":"echo hello","args":["hello"],"cwd":"/path","verb":"exec"}
{"v":1,"source":"arc","type":"cmd","event":"end","ts":"2026-08-22T07:30:15.188368+00:00","id":"019c…","cmd":"echo hello","exit_code":0,"ms":2,"success":true,"verb":"exec"}
```

---

## Rust API

### Signal の記録

```rust
use flux_core::{FluxProject, Signal};
use serde_json::json;

// プロジェクトの初期化
let project = FluxProject::init("./my_project")?;

// Signal の記録（任意の Serialize 可能な型）
project.record("deploy", json!({
    "target": "production",
    "commit": "abc123"
}))?;
```

### Signal の読み込み

```rust
// 全 Signal の読み込み
let signals = project.read_signals()?;

// フィルタリング
let exec_signals: Vec<&Signal> = signals
    .iter()
    .filter(|s| s.r_type.starts_with("exec"))
    .collect();
```

### State の再構築（将来実装）

```rust
// Signal ログから State を再構築
let state = project.rebuild_state()?;

// 特定時点の State を再構築
let state_at = project.rebuild_state_at("2026-02-18T16:30:00+09:00")?;
```

---

## ユースケース

### 1. AI エージェントとの協業

AI エージェントが実行した操作を完全に記録し、人間が後から検証できます。

```bash
# AI エージェントが実行
flux exec npm install express
flux exec npm run build

# 人間が後から確認
flux state
# → 何をインストールし、ビルドが成功したかが構造化データで分かる
```

### 2. 環境の再現

Signal ログを別のマシンに持っていけば、同じ手順で環境を再現できます。

```bash
# マシンA で作業
flux exec ruby -v
flux exec gem install rails
flux exec rails new my_app

# マシンB で再現
flux replay signals.jsonl
# → 同じコマンドを同じ順序で実行
```

### 3. デバッグ・監査

「いつ、何が起きたか」を正確に追跡できます。

```bash
# 失敗したコマンドだけを抽出
flux state --filter "cmd:end" --where "success=false"
```

### 4. 教育・メンタリング

環境構築の手順を Signal ログとして共有できます。
README に「この signals.jsonl を replay すれば環境が作れます」と書くだけ。

---

## ロードマップ

### Phase 1: Core Engine ✅ (現在)
- [x] Signal の記録 (`record`)
- [x] Signal の読み込み (`read_signals`)
- [x] 構造化ペイロード (`serde_json::Value`)
- [x] CLI ラッパー (`exec` コマンド)

### Phase 2: State Machine
- [ ] State の定義と再構築 (`rebuild_state`)
- [ ] Signal → State 変換ルールの定義
- [ ] 時間旅行（特定時点の State 再現）
- [ ] `snapshot` Signal の実装

### Phase 3: Replay Engine
- [ ] `flux replay` コマンド（Signal ログの再生）
- [ ] Dry-run モード（実行せずに手順を表示）
- [ ] 差分検出（現在の環境と Signal ログの差分）

### Phase 4: Ecosystem
- [ ] crates.io への公開 (`flux-core`)
- [ ] Python / Node.js バインディング
- [ ] VS Code 拡張（Signal の可視化）
- [ ] MCP Server 連携（AI エージェントからの直接記録）

---

## 設計原則

1. **Zero Configuration**: `.flux/` ディレクトリを作るだけで使える
2. **Human Readable**: NDJSON なので `cat` や `jq` で直接読める
3. **Machine Friendly**: 構造化 JSON なのでプログラムから解析しやすい
4. **Append Only**: ログは追記のみ。破壊的操作は存在しない
5. **Language Agnostic**: Rust クレートだが、NDJSON 形式なのでどの言語からも利用可能

---

## デモンストレーション: arc

`arc` は Flux Core の最初のショーケースです。
Ruby ツールチェーン（rv の補完ツール）として、以下を実証します：

- `arc init`: Flux プロジェクトの初期化
- `arc exec <cmd>`: 任意コマンドの記録実行
- `arc state`: Signal ログからの状態表示

```bash
$ arc init my_ruby_app
✨ arc project initialized successfully.

$ cd my_ruby_app
$ arc exec bundle install
🚀 Executing: bundle install
Signal recorded: cmd:run {"cmd":"bundle install","cwd":"/path/to/my_ruby_app","verb":"sync"}
# ... bundle install の出力 ...
Signal recorded: cmd:end {"exit_code":0,"ms":4200,"success":true}

$ arc state
🦄 Loading Flux State...
-------------------------------------------------------------
Signal            | ts                                | Payload
-------------------------------------------------------------
project:created   | 2026-08-22T07:30:15.177395+00:00  | {"path":"my_ruby_app",…}
cmd:run           | 2026-08-22T07:30:15.186032+00:00  | {"cmd":"bundle install",…}
cmd:end           | 2026-08-22T07:30:19.386032+00:00  | {"exit_code":0,"ms":4200,…}
-------------------------------------------------------------
```

---

## ライセンス

MIT

---

*Flux Core — すべての操作に意味を。*
