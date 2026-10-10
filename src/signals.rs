use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Flux Core のデータディレクトリ名
const FLUX_DIR: &str = ".flux";
/// Signal ログファイル名
const SIGNAL_FILE: &str = "signals.jsonl";
/// プロジェクト固有の環境ディレクトリ (Gem のインストール先)
pub const ARC_ENV_DIR: &str = ".arc/env";
/// グローバルキャッシュルート名
pub const ARC_CACHE_ROOT: &str = ".arc/cache";

/// Flux Protocol の envelope バージョン。v1 では常に 1。
pub const FLUX_V: u8 = 1;
/// この生産者の名前。すべてのレコードが `source` として名乗る。
pub const FLUX_SOURCE: &str = "arc";

/// グローバルなキャッシュディレクトリを取得する (~/.arc/cache)
pub fn get_global_cache_dir() -> PathBuf {
    // std::env::home_dir() は deprecated のため、HOME 環境変数を直接参照する。
    // Windows の素の席（cmd / hsh）には HOME が無いことがある —— `/tmp` に落ちると
    // 今いるドライブの `\tmp` を指すので、USERPROFILE を先に見る。
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    home.join(ARC_CACHE_ROOT)
}

/// Gem のグローバルキャッシュディレクトリを取得する (~/.arc/cache/gems)
pub fn get_global_gems_dir() -> PathBuf {
    get_global_cache_dir().join("gems")
}

// ─────────────────────────────────────────────
// SignalType (型安全なシグナル種別)
// ─────────────────────────────────────────────

/// Signal の種別。Flux Protocol v1 の `type`(主語=名詞) と `event`(起きたこと=過去分詞)
/// の組に対応する。
///
/// 🚨 **自由形式の種別はここには無い**。以前は `Custom(String)` があり、そこから
/// `shell_enter` / `add_start` のようなアンダースコア入りの名前が生まれていたが、
/// v1 の §3/§4 は `type`/`event` を `^[a-z0-9-]+$` に限っている。hsh が
/// `job:job_started` で同じ罠を踏み、「内部語彙を journal の境界で翻訳する」ことで
/// 直したのと同じ判断で、この enum が唯一の入口になっている。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    /// `project:created` — `arc init`
    ProjectCreated,
    /// `cmd:run` — 子プロセスの起動 (`exec` / `sync` / `run` 共通)
    CmdRun,
    /// `cmd:end` — 子プロセスの終了
    CmdEnd,
    /// `gem:added` — Gemfile に gem を足した
    GemAdded,
    /// `gem:removed` — Gemfile から gem を消した
    GemRemoved,
    /// `runtime:linked` — Ruby ランタイムをキャッシュからプロジェクトへ張った
    RuntimeLinked,
    /// `operation:reverted` — `arc undo`
    OperationReverted,
    /// `session:start` — `arc shell` に入った
    SessionStart,
    /// `session:end` — `arc shell` を出た
    SessionEnd,
}

impl SignalType {
    /// Flux v1 の `type`(主語)。
    pub const fn subject(self) -> &'static str {
        match self {
            SignalType::ProjectCreated    => "project",
            SignalType::CmdRun            => "cmd",
            SignalType::CmdEnd            => "cmd",
            SignalType::GemAdded          => "gem",
            SignalType::GemRemoved        => "gem",
            SignalType::RuntimeLinked     => "runtime",
            SignalType::OperationReverted => "operation",
            SignalType::SessionStart      => "session",
            SignalType::SessionEnd        => "session",
        }
    }

    /// Flux v1 の `event`(起きたこと)。
    pub const fn event(self) -> &'static str {
        match self {
            SignalType::ProjectCreated    => "created",
            SignalType::CmdRun            => "run",
            SignalType::CmdEnd            => "end",
            SignalType::GemAdded          => "added",
            SignalType::GemRemoved        => "removed",
            SignalType::RuntimeLinked     => "linked",
            SignalType::OperationReverted => "reverted",
            SignalType::SessionStart      => "start",
            SignalType::SessionEnd        => "end",
        }
    }
}

impl fmt::Display for SignalType {
    /// registry の引き方と同じ `type:event` 表記。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.subject(), self.event())
    }
}

// ─────────────────────────────────────────────
// Signal (イベント)
// ─────────────────────────────────────────────

/// 構造化された操作イベント。Flux Protocol v1 の 1 レコード = 1 行。
///
/// 必須 4 フィールド (`v` / `source` / `type` / `event`) が envelope で、
/// 残りはすべて payload としてトップレベルに平らに載る（入れ子にしない）。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Signal {
    /// envelope バージョン。v1 では常に 1。
    pub v: u8,
    /// 生産者の名前。arc のレコードは常に `"arc"`。
    pub source: String,
    /// 主語の分類 (名詞)。
    #[serde(rename = "type")]
    pub subject: String,
    /// 主語に起きたこと (過去分詞 / 状態名)。
    pub event: String,
    /// 出来事が起きた時刻。RFC 3339 + 明示的オフセット (UTC)。
    ///
    /// 綴りは `PROTOCOL_v1.md` §2.05 の `ts`。`modified` / `timestamp` は
    /// 同じ意味で使われていた旧綴りで、読むときだけ受け入れる (§2.05.3)。
    pub ts: String,
    /// 残り全部。envelope の隣にトップレベルで並ぶ。
    #[serde(flatten)]
    pub payload: Map<String, Value>,
}

impl Signal {
    /// registry の引き方と同じ `type:event` 表記。
    pub fn name(&self) -> String {
        format!("{}:{}", self.subject, self.event)
    }

    /// このレコードの識別子 (UUID v7)。`cmd:run` と `cmd:end` は**同じ値**を持ち、
    /// それが両者を綴じる鍵になる (hsh の journal と同じ相関モデル)。
    pub fn id(&self) -> &str {
        self.payload.get("id").and_then(Value::as_str).unwrap_or("")
    }

    /// 終端レコードが始端を指す鍵。新形式では `id` が両方に載るので `id` を返し、
    /// 旧形式では終端だけが持っていた `ref_id` を返す。
    pub fn correlation_id(&self) -> &str {
        self.payload
            .get("ref_id")
            .and_then(Value::as_str)
            .unwrap_or_else(|| self.id())
    }

    /// 1 行を Signal に直す。新形式と、`v` を持たない旧形式の両方を受ける。
    ///
    /// 🚨 **改名は歴史を直さない**。`arc` が envelope を移す前に書かれた
    /// `.flux/signals.jsonl` は旧綴りのまま残っているので、読む側が
    /// 吸収する。これは一時的な措置ではない (`PROTOCOL_v1.md` §2.05.3)。
    pub fn from_value(value: Value) -> Result<Self> {
        let Value::Object(obj) = value else {
            bail!("signal is not a JSON object");
        };

        if obj.contains_key("v") {
            return serde_json::from_value(Value::Object(obj))
                .context("failed to read a Flux v1 signal");
        }

        Ok(Self::from_legacy(obj))
    }

    /// `{id, type, payload, timestamp}` 形式 (2026-02〜2026-08) を読む。
    fn from_legacy(obj: Map<String, Value>) -> Self {
        let legacy_type = obj.get("type").and_then(Value::as_str).unwrap_or("");
        let (subject, event) = legacy_name(legacy_type);

        // 消費側は最初に在るものを取る: ts ?? modified ?? timestamp (§2.05.3)
        let ts = ["ts", "modified", "timestamp"]
            .iter()
            .find_map(|k| obj.get(*k).and_then(Value::as_str))
            .unwrap_or("")
            .to_string();

        // 旧形式は payload を入れ子にし、id を envelope 側に置いていた。
        let mut payload = match obj.get("payload") {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        };
        if let Some(id) = obj.get("id") {
            payload.insert("id".to_string(), id.clone());
        }
        if subject == "legacy" {
            payload.insert("legacy_type".to_string(), json!(legacy_type));
        }

        Signal {
            v: FLUX_V,
            source: FLUX_SOURCE.to_string(),
            subject: subject.to_string(),
            event: event.to_string(),
            ts,
            payload,
        }
    }
}

/// 旧 `type` 文字列を v1 の `type:event` に翻訳する。
///
/// 旧形式は動作を主語に畳んでいた (`add_start`) 上、アンダースコアが
/// §3/§4 の `^[a-z0-9-]+$` に反していた。既知でない綴りは `legacy:recorded`
/// に落とし、元の文字列を `legacy_type` として payload に残す。
fn legacy_name(legacy_type: &str) -> (&'static str, &'static str) {
    match legacy_type {
        "init"                          => ("project", "created"),
        "exec_start" | "install_start" | "run_start" => ("cmd", "run"),
        "exec_end"   | "install_end"   | "run_end"   => ("cmd", "end"),
        "add"                           => ("gem", "added"),
        "remove"                        => ("gem", "removed"),
        "bootstrap"                     => ("runtime", "linked"),
        "undo"                          => ("operation", "reverted"),
        "shell_enter"                   => ("session", "start"),
        "shell_exit"                    => ("session", "end"),
        _                               => ("legacy", "recorded"),
    }
}

// ─────────────────────────────────────────────
// FluxProject (プロジェクト)
// ─────────────────────────────────────────────

/// Flux Core プロジェクト。
/// `.flux/` ディレクトリを管理し、Signal の記録・読み込みを行う。
pub struct FluxProject {
    /// プロジェクトルートディレクトリ (Phase 2 再構築で使用予定)
    #[allow(dead_code)]
    pub root: PathBuf,
    /// `.flux/` ディレクトリのパス
    pub flux_dir: PathBuf,
    /// `signals.jsonl` のパス
    pub signal_file: PathBuf,
}

impl FluxProject {
    /// 新しい Flux プロジェクトを初期化する。
    /// `.flux/` ディレクトリと `signals.jsonl` を作成する。
    /// 既に初期化済みの場合はエラーを返す。
    pub fn init(project_root: &Path) -> Result<Self> {
        let flux_dir = project_root.join(FLUX_DIR);
        let signal_file = flux_dir.join(SIGNAL_FILE);

        if signal_file.exists() {
            bail!(
                "Already initialized: {:?} exists. Use FluxProject::open() instead.",
                signal_file
            );
        }

        fs::create_dir_all(&flux_dir)
            .with_context(|| format!("Failed to create {:?}", flux_dir))?;

        Ok(Self {
            root: project_root.to_path_buf(),
            flux_dir,
            signal_file,
        })
    }

    /// 既存の Flux プロジェクトを開く。
    /// カレントディレクトリから `.flux/` を探す。存在しない場合はエラーを返す。
    pub fn open(project_root: &Path) -> Result<Self> {
        let flux_dir = project_root.join(FLUX_DIR);
        let signal_file = flux_dir.join(SIGNAL_FILE);

        if !flux_dir.exists() {
            bail!(
                "Not a Flux project: {:?} not found. Run `arc init` first.",
                flux_dir
            );
        }

        Ok(Self {
            root: project_root.to_path_buf(),
            flux_dir,
            signal_file,
        })
    }

    /// Signal を記録し、記録された Signal を返す。
    ///
    /// payload が `id` を持っていなければ UUID v7 を採番する。呼び出し側が
    /// `id` を渡した場合はそれを尊重する —— `cmd:end` が `cmd:run` と同じ
    /// `id` を名乗るのはこの経路。
    pub fn record<T: Serialize>(&self, signal_type: SignalType, payload: T) -> Result<Signal> {
        let mut payload = match serde_json::to_value(payload)? {
            Value::Object(m) => m,
            Value::Null      => Map::new(),
            other            => {
                let mut m = Map::new();
                m.insert("value".to_string(), other);
                m
            }
        };
        payload
            .entry("id".to_string())
            .or_insert_with(|| json!(Uuid::now_v7().to_string()));

        let signal = Signal {
            v: FLUX_V,
            source: FLUX_SOURCE.to_string(),
            subject: signal_type.subject().to_string(),
            event: signal_type.event().to_string(),
            // hsh と同じ綴り・同じ時計: UTC・マイクロ秒・明示的オフセット。
            ts: Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false),
            payload,
        };

        let json = serde_json::to_string(&signal)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.signal_file)
            .with_context(|| format!("Failed to open {:?}", self.signal_file))?;

        writeln!(file, "{}", json)?;

        Ok(signal)
    }

    /// すべての Signal を時系列順に読み込む。
    pub fn read_signals(&self) -> Result<Vec<Signal>> {
        if !self.signal_file.exists() {
            return Ok(vec![]);
        }

        let content = fs::read_to_string(&self.signal_file)?;
        let mut signals = Vec::new();

        for (i, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line)
                .with_context(|| format!("Failed to parse signal at line {}", i + 1))?;
            let signal = Signal::from_value(value)
                .with_context(|| format!("Failed to read signal at line {}", i + 1))?;
            signals.push(signal);
        }

        Ok(signals)
    }
}

// ─────────────────────────────────────────────
// ヘルパー関数
// ─────────────────────────────────────────────

/// 文字列を指定文字数で安全に切り詰める（Unicode 安全）。
pub fn truncate_display(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count > max_chars {
        let truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    } else {
        s.to_string()
    }
}
