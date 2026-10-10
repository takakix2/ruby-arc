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

/// `ruby_runtime/bin/ruby` パスを返す（Windows は `ruby.exe`）
pub fn ruby_bin(env_path: &Path) -> PathBuf {
    ruby_bin_in(&ruby_runtime_root(env_path))
}

/// Ruby の木（`bin/` を持つ所）の中の ruby の実体
pub fn ruby_bin_in(ruby_root: &Path) -> PathBuf {
    ruby_root.join("bin").join(if cfg!(windows) { "ruby.exe" } else { "ruby" })
}

/// 裸の名前に付けて探す拡張子（順に）。Windows の RubyInstaller は `bundle.bat` / `gem.cmd` 等を持ち、
/// 拡張子なしの `bundle` は Ruby のスクリプトで直接は起こせない ＝ 拡張子付きを先に、素の名前は最後。
fn program_suffixes() -> &'static [&'static str] {
    if cfg!(windows) { &[".exe", ".bat", ".cmd", ""] } else { &[""] }
}

/// 隔離環境で先に探す bin ディレクトリ。子の PATH の先頭に置く順と同じ。
pub fn isolated_bin_dirs(env_path: &Path) -> Vec<PathBuf> {
    vec![ruby_runtime_bin(env_path), env_path.join("bin")]
}

/// 裸の名前（`bundle` 等）を、隔離環境の bin から実体に解決する。
///
/// `Command::new("bundle")` は子に渡す PATH ではなく **arc 自身の PATH** で探すので、
/// 素の PATH に `bundle` が無い台では `arc sync` が `No such file or directory` で落ちていた。
/// パスを含む名前と、隔離環境に無い名前は `None`（呼び手が今までどおり `cmd` で起こす）。
pub fn resolve_isolated_program(cmd: &str, env_path: &Path) -> Option<PathBuf> {
    if cmd.contains('/') || cmd.contains(std::path::MAIN_SEPARATOR) {
        return None;
    }
    isolated_bin_dirs(env_path)
        .into_iter()
        .flat_map(|dir| program_suffixes().iter().map(move |s| dir.join(format!("{cmd}{s}"))))
        .find(|p| p.is_file())
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
/// `ruby_runtime/lib/ruby/` の中の API 版ディレクトリ名（例: "3.3.0"）。数字で始まる物を探す。
pub fn ruby_api_dir_name(env_path: &Path) -> Option<String> {
    std::fs::read_dir(ruby_runtime_lib(env_path).join("ruby"))
        .ok()?
        .flatten()
        .find(|e| {
            e.path().is_dir()
                && e.file_name()
                    .to_str()
                    .is_some_and(|n| n.chars().next().is_some_and(|c| c.is_numeric()))
        })
        .and_then(|e| e.file_name().to_str().map(str::to_string))
}

/// 子に渡す GEM_HOME。bundler は `BUNDLE_PATH`（`.arc/env`）の下の `ruby/<API 版>/` に入れるので、
/// GEM_HOME もそこを指す ＝ `bundle exec` を通さない素の `ruby` からも gem が見える。
/// ⚠️ `.arc/env` を指していた頃は、`arc sync` で入れた gem が `arc run ruby x.rb` から見えなかった。
/// Ruby がまだ無い（API 版が分からない）ときは `.arc/env` のまま。
pub fn gem_home(env_path: &Path) -> PathBuf {
    match ruby_api_dir_name(env_path) {
        Some(ver) => env_path.join("ruby").join(ver),
        None => env_path.to_path_buf(),
    }
}

pub fn build_rubylib_path(env_path: &Path) -> Option<OsString> {
    let ruby_lib_dir = ruby_runtime_lib(env_path).join("ruby");
    let ver_name = ruby_api_dir_name(env_path)?;
    let ver_name = ver_name.as_str();
    let ver_path = ruby_lib_dir.join(ver_name);

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

    let program = match env_mode {
        ArcEnv::Isolated => resolve_isolated_program(cmd, &cwd.join(ARC_ENV_DIR))
            .unwrap_or_else(|| PathBuf::from(cmd)),
        ArcEnv::System => PathBuf::from(cmd),
    };
    let mut command = Command::new(&program);
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
    command.env("GEM_HOME",    gem_home(&env_path));
    command.env("BUNDLE_PATH", &env_path);

    // LD_LIBRARY_PATH: 共有ライブラリの解決（Windows は DLL を exe の隣から読むので要らない）
    if !cfg!(windows)
        && let Some(ld_path) = build_ld_library_path(&env_path)
    {
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

    // 先頭に置くのは bin の**ディレクトリ**。`b9b6997` から `bin/ruby`（ファイル）を
    // 置いていたので、子の中から `bundle` / `rake` 等が 1 つも引けなかった。
    let new_path = {
        let mut paths = isolated_bin_dirs(&env_path);
        if let Some(current) = env::var_os("PATH") {
            paths.extend(env::split_paths(&current));
        }
        env::join_paths(paths)?
    };
    command.env("PATH", new_path);

    // RUBYLIB: ポータブルRuby環境での標準ライブラリ解決。
    // Windows の RubyInstaller は自分の置き場所から標準ライブラリを読める（📏 dyna で確認）ので足さない
    // —— 足すと arch のディレクトリ名（x64-mingw-ucrt）を外した順の $LOAD_PATH を押し付けることになる。
    if !cfg!(windows)
        && let Some(rubylib) = build_rubylib_path(&env_path)
    {
        command.env("RUBYLIB", rubylib);
    }

    Ok(())
}

// ─────────────────────────────────────────────
// テスト
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// `.arc/env/ruby_runtime/bin/{ruby,bundle}` を持つ使い捨てのプロジェクト
    fn fake_project() -> PathBuf {
        let root = env::temp_dir().join(format!("arc-runner-test-{}", Uuid::now_v7()));
        let bin = ruby_runtime_bin(&root.join(ARC_ENV_DIR));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(ruby_bin(&root.join(ARC_ENV_DIR)), "").unwrap(); // Windows は ruby.exe
        std::fs::write(bin.join("bundle"), "").unwrap();
        std::fs::create_dir_all(ruby_runtime_lib(&root.join(ARC_ENV_DIR)).join("ruby").join("3.3.0")).unwrap();
        root
    }

    #[test]
    fn gem_home_is_where_bundler_installs() {
        let root = fake_project();
        let env_path = root.join(ARC_ENV_DIR);
        // bundler は BUNDLE_PATH（.arc/env）の下の ruby/<API 版>/ に入れる
        assert_eq!(gem_home(&env_path), env_path.join("ruby").join("3.3.0"));
        let mut command = Command::new("true");
        inject_isolated_env(&mut command, &root).unwrap();
        let get = |name: &str| command.get_envs().find(|(k, _)| *k == name).and_then(|(_, v)| v.map(PathBuf::from));
        assert_eq!(get("GEM_HOME"), Some(env_path.join("ruby").join("3.3.0")));
        assert_eq!(get("BUNDLE_PATH"), Some(env_path.clone()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_child_path_starts_with_the_runtime_bin_directory() {
        let root = fake_project();
        let mut command = Command::new("true");
        inject_isolated_env(&mut command, &root).unwrap();
        let path = command
            .get_envs()
            .find(|(k, _)| *k == "PATH")
            .and_then(|(_, v)| v.map(|v| v.to_os_string()))
            .unwrap();
        let first = env::split_paths(&path).next().unwrap();
        // b9b6997 から `bin/ruby`（ファイル）が先頭に入っていた
        assert_eq!(first, ruby_runtime_bin(&root.join(ARC_ENV_DIR)));
        assert!(first.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_bare_name_resolves_to_the_isolated_bin() {
        let root = fake_project();
        let env_path = root.join(ARC_ENV_DIR);
        assert_eq!(
            resolve_isolated_program("bundle", &env_path),
            Some(ruby_runtime_bin(&env_path).join("bundle"))
        );
        // 隔離環境に無い名前と、パスを含む名前は解決しない（呼び手がそのまま起こす）
        assert_eq!(resolve_isolated_program("sh", &env_path), None);
        assert_eq!(resolve_isolated_program("./bundle", &env_path), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
