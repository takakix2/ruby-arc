//! 子プロセスの寿命を arc に結びつける。
//!
//! 📏 2026-10-11（Rapi #7 を撃っていて踏んだ・Follow-Ups 14）: `arc x --with puma … -- -S puma …` を
//! 裏で起こして `arc` の PID に `kill` すると、**Puma（PPID 1）が残ってポートを握り続けた** ——
//! arc は子を `status()` で待つだけで、受けたシグナルを子に渡していなかった。
//!
//! - **Linux / macOS**: SIGTERM / SIGHUP / SIGQUIT を子へ渡し、子の終わりを待つ。
//!   ⚠️ **SIGINT は渡さず受け流す** —— 端末の Ctrl+C はプロセスグループ全員（arc も子も）に届くので、
//!   渡し直すと子が 2 回受け取る。bash が子を待つときと同じ作法。
//!   （記録を残さない `arc x` は、そもそも `exec` で ruby に置き換わる —— `exec_or_run`）
//! - **Windows**: 子を job object（`KILL_ON_JOB_CLOSE`）に入れる ＝ arc が消えれば job の handle が閉じて子も消える。
//!   子を起こした**後で** arc 自身は Ctrl+C を受け流す —— 先に死ぬと、job の道連れで子が後始末の途中でも殺される。
//!   ⚠️ 起こす**前**に無視を設定すると、子が「Ctrl+C を無視する」を継いでしまう（`SetConsoleCtrlHandler(NULL, TRUE)` は
//!   継がれる）ので、継がれない handler 関数で受け流す。

use std::process::{Command, ExitStatus};

/// 子を起こし、arc に結びつけたまま終わりを待つ（`arc run` / `arc shell` / Windows の `arc x`）
pub fn run_tied(command: &mut Command) -> std::io::Result<ExitStatus> {
    let mut child = command.spawn()?;
    let _tie = Tie::new(&child);
    child.wait()
}

/// 記録を残さない呼び出し（`arc x`）: Linux / macOS は `exec` で置き換わり、戻らない。
/// Windows は置き換えられないので `run_tied` で待ち、その終了コードで抜ける。
pub fn exec_or_run(command: &mut Command) -> std::io::Result<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec()) // 戻ってきたら exec に失敗している
    }
    #[cfg(not(unix))]
    {
        run_tied(command).map(|s| exit_code(&s))
    }
}

/// 終了コード。シグナルで死んだ子は、シェルと同じく 128 + シグナル番号（`code()` は None になる）
pub fn exit_code(status: &ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    1
}

// ─────────────────────────────────────────────
// Linux / macOS: シグナルを子へ渡す
// ─────────────────────────────────────────────

#[cfg(unix)]
struct Tie {
    handle: signal_hook::iterator::Handle,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl Tie {
    fn new(child: &std::process::Child) -> Option<Self> {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
        let pid = child.id() as libc::pid_t;
        // SIGINT も登録する ＝ 既定の「arc が死ぬ」を止める（子には端末から届くので渡さない）
        let mut signals = signal_hook::iterator::Signals::new([SIGINT, SIGTERM, SIGHUP, SIGQUIT]).ok()?;
        let handle = signals.handle();
        let thread = std::thread::spawn(move || {
            for sig in signals.forever() {
                if sig != SIGINT {
                    // SAFETY: 子の pid へシグナルを送るだけ。子が既に居なければ ESRCH で何も起きない
                    unsafe {
                        libc::kill(pid, sig);
                    }
                }
            }
        });
        Some(Tie { handle, thread: Some(thread) })
    }
}

#[cfg(unix)]
impl Drop for Tie {
    fn drop(&mut self) {
        // 子は終わっている ＝ 受け取りをやめ、既定の扱いに戻す
        self.handle.close();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ─────────────────────────────────────────────
// Windows: job object で子を道連れにする
// ─────────────────────────────────────────────

#[cfg(windows)]
struct Tie {
    job: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe extern "system" fn swallow_ctrl(_ctrl_type: u32) -> windows_sys::Win32::Foundation::BOOL {
    1 // 受け流す（子は自分で Ctrl+C を受けて自分で終わる）
}

#[cfg(windows)]
impl Tie {
    fn new(child: &std::process::Child) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: Win32 の呼び出し。失敗したら（入れ子にできない古い OS 等）結びつけずに待つだけ
        unsafe {
            SetConsoleCtrlHandler(Some(swallow_ctrl), 1);
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0 {
                windows_sys::Win32::Foundation::CloseHandle(job);
                return None;
            }
            Some(Tie { job })
        }
    }
}

#[cfg(windows)]
impl Drop for Tie {
    fn drop(&mut self) {
        // 子は終わっている ＝ job を閉じても誰も道連れにならない。Ctrl+C の扱いも戻す
        // SAFETY: 自分で作った handle を 1 回だけ閉じる
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(swallow_ctrl), 0);
        }
    }
}
