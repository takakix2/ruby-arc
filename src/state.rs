use crate::signals::Signal;
use std::collections::HashMap;

// ─────────────────────────────────────────────
// State (Signal ログから再構築される環境状態)
// ─────────────────────────────────────────────

/// 個々のコマンド実行記録（`cmd:run` + `cmd:end` のペア）。
/// 両者は同じ `id` を持つので、それが突き合わせの鍵になる。
/// Phase 2 で `cwd`, `ended_at`, `start_id` の内容を利用予定。
#[derive(Debug)]
#[allow(dead_code)]
pub struct Execution {
    /// 表示用の 1 本のコマンドライン（新形式は `cmd`、旧形式は `command`+`args` を繋いだもの）
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub exit_code: Option<i64>,
    pub success: bool,
    pub duration_ms: Option<u64>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub start_id: String,
}

/// コマンドごとの集計統計
#[derive(Debug)]
pub struct CommandStats {
    pub command: String,
    pub total_runs: usize,
    pub successes: usize,
    pub failures: usize,
    pub avg_duration_ms: Option<u64>,
    pub last_run: String,
}

/// Signal ログから再構築されたプロジェクト状態
#[derive(Debug)]
pub struct FluxState {
    /// プロジェクトパス
    pub project_path: Option<String>,
    /// arc バージョン
    pub version: Option<String>,
    /// 初期化日時
    pub initialized_at: Option<String>,
    /// 全実行記録
    pub executions: Vec<Execution>,
    /// Signal 総数
    pub signal_count: usize,
}

/// レコードから表示用のコマンドラインを取り出す。
///
/// 新形式は `cmd` に 1 本の文字列として持っている。旧形式は `command` と `args` に
/// 割れていたので、繋いで同じ形にする。
fn command_line(signal: &Signal) -> String {
    if let Some(cmd) = signal.payload.get("cmd").and_then(|v| v.as_str()) {
        return cmd.to_string();
    }
    let prog = signal.payload.get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let args = argv(signal);
    if args.is_empty() {
        prog.to_string()
    } else {
        format!("{} {}", prog, args.join(" "))
    }
}

/// argv の忠実な形。`cmd` が潰した空白入りの引数はこちらに残っている。
fn argv(signal: &Signal) -> Vec<String> {
    signal.payload.get("args")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

impl FluxState {
    /// Signal のベクターから State を再構築する
    pub fn from_signals(signals: &[Signal]) -> Self {
        let mut state = FluxState {
            project_path: None,
            version: None,
            initialized_at: None,
            executions: Vec::new(),
            signal_count: signals.len(),
        };

        // `cmd:run` を一時的に保持する HashMap（鍵 = 相関 id）
        let mut pending_starts: HashMap<String, &Signal> = HashMap::new();

        for signal in signals {
            match signal.name().as_str() {
                "project:created" => {
                    state.initialized_at = Some(signal.ts.clone());
                    state.project_path = signal.payload.get("path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    state.version = signal.payload.get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
                "cmd:run" => {
                    // 相方の `cmd:end` と突き合わせるまで置いておく。
                    pending_starts.insert(signal.id().to_string(), signal);
                }
                "cmd:end" => {
                    // 新形式は `cmd:run` と同じ `id`、旧形式は `ref_id` が始端を指す。
                    let start_signal = pending_starts.remove(signal.correlation_id());

                    let (command, args, cwd, started_at, start_id) = if let Some(start) = start_signal {
                        (
                            command_line(start),
                            argv(start),
                            start.payload.get("cwd")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            start.ts.clone(),
                            start.id().to_string(),
                        )
                    } else {
                        // 始端を失った終端でも、`cmd:end` 自身がコマンドラインを持っている。
                        (command_line(signal), argv(signal), String::new(), String::new(), String::new())
                    };

                    let exit_code = signal.payload.get("exit_code")
                        .and_then(|v| v.as_i64());
                    let success = signal.payload.get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or_else(|| exit_code == Some(0));
                    // 綴りは `ms`（家族の綴り）。`duration_ms` は arc の旧綴り。
                    let duration_ms = signal.payload.get("ms")
                        .or_else(|| signal.payload.get("duration_ms"))
                        .and_then(|v| v.as_u64());

                    state.executions.push(Execution {
                        command,
                        args,
                        cwd,
                        exit_code,
                        success,
                        duration_ms,
                        started_at,
                        ended_at: Some(signal.ts.clone()),
                        start_id,
                    });
                }
                _ => {
                    // Unknown signal types are ignored (forward compatibility)
                }
            }
        }

        // 未完了の `cmd:run` (SIGKILL 等で `cmd:end` がない) を orphan として記録
        for (_id, start) in pending_starts {
            let cwd = start.payload.get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            state.executions.push(Execution {
                command: command_line(start),
                args: argv(start),
                cwd,
                exit_code: None,
                success: false,
                duration_ms: None,
                started_at: start.ts.clone(),
                ended_at: None,
                start_id: start.id().to_string(),
            });
        }

        state
    }

    /// コマンドごとの統計を計算する
    pub fn command_stats(&self) -> Vec<CommandStats> {
        let mut stats_map: HashMap<String, Vec<&Execution>> = HashMap::new();

        for exec in &self.executions {
            stats_map.entry(exec.command.clone()).or_default().push(exec);
        }

        let mut stats: Vec<CommandStats> = stats_map
            .into_iter()
            .map(|(command, execs)| {
                let total_runs = execs.len();
                let successes = execs.iter().filter(|e| e.success).count();
                let failures = total_runs - successes;

                let durations: Vec<u64> = execs.iter()
                    .filter_map(|e| e.duration_ms)
                    .collect();
                let avg_duration_ms = if durations.is_empty() {
                    None
                } else {
                    Some(durations.iter().sum::<u64>() / durations.len() as u64)
                };

                let last_run = execs.iter()
                    .max_by_key(|e| &e.started_at)
                    .map(|e| e.started_at.clone())
                    .unwrap_or_default();

                CommandStats {
                    command,
                    total_runs,
                    successes,
                    failures,
                    avg_duration_ms,
                    last_run,
                }
            })
            .collect();

        // 最新のコマンドが上に来るようにソート
        stats.sort_by(|a, b| b.last_run.cmp(&a.last_run));
        stats
    }

    /// 最後に実行されたコマンド
    pub fn last_execution(&self) -> Option<&Execution> {
        self.executions.last()
    }

    /// 失敗したコマンドの一覧
    pub fn failed_executions(&self) -> Vec<&Execution> {
        self.executions.iter().filter(|e| !e.success).collect()
    }
}
