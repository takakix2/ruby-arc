use anyhow::Result;
use serde_json::json;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::signals::{ARC_ENV_DIR, FluxProject, SignalType};
use uuid::Uuid;

/// プロセスの環境モード。
/// `Isolated` は `.arc/env` を GEM_HOME として使用し、
/// `System` はシステムの環境変数をそのまま引き継ぐ。
#[derive(Debug, Clone, PartialEq)]
pub enum ArcEnv {
    /// プロジェクト固有の隔離環境 (.arc/env) を使用する
    Isolated,
    /// システムの環境変数をそのまま使用する
    System,
}

// ─────────────────────────────────────────────
// 環境パス解決ユーティリティ (公開: env コマンドから再利用)
// ─────────────────────────────────────────────

/// `.arc/env` パスから `ruby_runtime` のルートを返す
pub fn ruby_runtime_root(env_path: &Path) -> PathBuf {
    env_path.join("ruby_runtime")
}

/// `ruby_runtime/bin` パスを返す
pub fn ruby_runtime_bin(env_path: &Path) -> PathBuf {
    ruby_runtime_root(env_path).join("bin")
}

/// `ruby_runtime/lib` パスを返す
pub fn ruby_runtime_lib(env_path: &Path) -> PathBuf {
    ruby_runtime_root(env_path).join("lib")
}

/// `ruby_runtime/bin/ruby` パスを返す
pub fn ruby_bin(env_path: &Path) -> PathBuf {
    ruby_runtime_bin(env_path).join("ruby")
}

/// LD_LIBRARY_PATH を構築する。
/// `ruby_runtime/lib` が存在する場合、それを既存の値の先頭に追加する。
pub fn build_ld_library_path(env_path: &Path) -> Option<OsString> {
    let lib = ruby_runtime_lib(env_path);
    if !lib.exists() {
        return None;
    }

    let result = match env::var_os("LD_LIBRARY_PATH") {
        Some(current) => {
            let mut paths = vec![lib];
            paths.extend(env::split_paths(&current));
            env::join_paths(paths).ok()?
        }
        None => lib.into_os_string(),
    };
    Some(result)
}

/// RUBYLIB を構築する。
/// `ruby_runtime/lib/ruby/<version>/` ディレクトリを探索し、
/// site_ruby / vendor_ruby / standard lib を RUBYLIB にセットする。
/// ポータブルな GitHub Actions 由来の Ruby バイナリのパス問題を解決する。
pub fn build_rubylib_path(env_path: &Path) -> Option<OsString> {
    let ruby_lib_dir = ruby_runtime_lib(env_path).join("ruby");
    if !ruby_lib_dir.exists() {
        return None;
    }

    // 数字で始まるディレクトリを探す (例: "3.3.0")
    let ver_dir = std::fs::read_dir(&ruby_lib_dir)
        .ok()?
        .flatten()
        .find(|e| {
            e.path().is_dir()
                && e.file_name()
                    .to_str()
                    .is_some_and(|n| n.chars().next().is_some_and(|c| c.is_numeric()))
        })?;

    let ver_path = ver_dir.path();
    let ver_name = ver_dir.file_name();
    let ver_name = ver_name.to_str()?;

    let site_ruby   = ruby_lib_dir.join("site_ruby");
    let vendor_ruby = ruby_lib_dir.join("vendor_ruby");

    // Ruby の標準的な $LOAD_PATH 構成を再現する
    //   site_ruby/<ver>, site_ruby,
    //   vendor_ruby/<ver>, vendor_ruby,
    //   <ver>/<arch>-linux  (重要: rbconfig.rb はここに存在する)
    //   <ver>
    let mut lib_paths = vec![
        site_ruby.join(ver_name),
        site_ruby,
        vendor_ruby.join(ver_name),
        vendor_ruby,
    ];

    // アーキテクチャ依存パス (rbconfig.rb の場所)
    // 例: lib/ruby/3.3.0/x86_64-linux
    let arch_suffix = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
    let arch_dir = ver_path.join(&arch_suffix);
    if arch_dir.exists() {
        lib_paths.push(arch_dir);
    }

    // 標準ライブラリルート
    lib_paths.push(ver_path);

    let result = match env::var_os("RUBYLIB") {
        Some(current) => {
            lib_paths.extend(env::split_paths(&current));
            env::join_paths(lib_paths).ok()?
        }
        None => env::join_paths(lib_paths).ok()?,
    };
    Some(result)
}

// ─────────────────────────────────────────────
// コマンド実行 (Flux シグナル記録付き)
// ─────────────────────────────────────────────

/// コマンドを実行し、開始・終了を Flux シグナルとして記録する。
/// `exec`, `install`, `run` の共通ロジックを一元化する。
pub fn run_with_flux(
    project: &FluxProject,
    verb: &str,
    cmd: &str,
    args: &[String],
    cwd: &Path,
    env_mode: ArcEnv,
) -> Result<()> {
    // `cmd:run` / `cmd:end` は同じ `id` を名乗ることで綴じられる。
    // 終端が始端を指すのではなく、両者が同じ鍵を持つ —— hsh の journal と同じ相関モデル。
    let id = Uuid::now_v7().to_string();

    // 家族の綴りに合わせた 1 本のコマンドライン。argv の忠実な形は `args` に残す
    // （空白を含む引数はここで潰れるが、`args` を引けば元が取れる）。
    let cmd_line = crate::display::fmt_cmd(cmd, args);
    let env_mode_name = match env_mode {
        ArcEnv::Isolated => "isolated",
        ArcEnv::System   => "system",
    };

    project.record(
        SignalType::CmdRun,
        json!({
            "id":       &id,
            "cmd":      &cmd_line,
            "args":     args,
            "cwd":      cwd.to_string_lossy(),
            "verb":     verb,
            "env_mode": env_mode_name,
        }),
    )?;

    let mut command = Command::new(cmd);
    command.args(args);

    // 隔離モードの場合、環境変数を注入する
    if env_mode == ArcEnv::Isolated {
        inject_isolated_env(&mut command, cwd)?;
    }

    let timer = Instant::now();
    let status = command
        .status()
        .map_err(|e| anyhow::anyhow!("コマンド '{}' の起動に失敗しました: {}", cmd, e))?;

    let ms = timer.elapsed().as_millis() as u64;
    let exit_code = status.code().unwrap_or(1);

    project.record(
        SignalType::CmdEnd,
        json!({
            "id":         &id,
            "cmd":        &cmd_line,
            "exit_code":  exit_code,
            "success":    status.success(),
            "ms":         ms,
            "cwd":        cwd.to_string_lossy(),
            "verb":       verb,
            "env_mode":   env_mode_name,
        }),
    )?;

    if !status.success() {
        // std::process::exit() は Rust の Drop トレイトを呼び出さずに即座に終了する。
        // 現状すべての Signal 記録は完了しているため問題ないが、
        // 将来バッファリングされた書き込みを導入する場合は要注意。
        std::process::exit(exit_code);
    }

    Ok(())
}

/// 隔離モード用の環境変数を `Command` に注入する。
/// PATH, GEM_HOME, BUNDLE_PATH, LD_LIBRARY_PATH, RUBYLIB を設定する。
/// `arc shell` からも再利用できるよう `pub` に公開している。
pub fn inject_isolated_env(command: &mut Command, cwd: &Path) -> Result<()> {
    let env_path = cwd.join(ARC_ENV_DIR);
    let gem_home = env_path.to_string_lossy().to_string();

    command.env("GEM_HOME",    &gem_home);
    command.env("BUNDLE_PATH", &gem_home);

    // LD_LIBRARY_PATH: 共有ライブラリの解決
    if let Some(ld_path) = build_ld_library_path(&env_path) {
        command.env("LD_LIBRARY_PATH", ld_path);
    }

    // PATH: ruby_runtime/bin を最優先
    let bin_path = ruby_bin(&env_path);
    if !bin_path.exists() {
        anyhow::bail!(
            "Ruby runtime not found in {:?}.\nRun `arc bootstrap` to install it.",
            bin_path.parent().unwrap()
        );
    }

    let new_path = {
        let mut paths = vec![
            bin_path,
            env_path.join("bin"),
        ];
        if let Some(current) = env::var_os("PATH") {
            paths.extend(env::split_paths(&current));
        }
        env::join_paths(paths)?
    };
    command.env("PATH", new_path);

    // RUBYLIB: ポータブルRuby環境での標準ライブラリ解決
    if let Some(rubylib) = build_rubylib_path(&env_path) {
        command.env("RUBYLIB", rubylib);
    }

    Ok(())
}
