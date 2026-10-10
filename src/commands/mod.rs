mod runner;

use anyhow::{Context, Result};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::{env, fs};

use crate::config::ArcConfig;
use crate::display;
use crate::gemfile;
use crate::signals::{FluxProject, SignalType};
use uuid::Uuid;
use runner::{ArcEnv, build_ld_library_path, gem_home, inject_isolated_env, ruby_bin};

// ─────────────────────────────────────────────
// 定数
// ─────────────────────────────────────────────

/// Gem が格納されるサブディレクトリ名。
/// `gems/`: ソース本体, `specifications/`: メタデータ, `extensions/`: C拡張バイナリ
const GEM_SUBDIRS: [&str; 3] = ["gems", "specifications", "extensions"];

// ─────────────────────────────────────────────
// 低レベルヘルパー
// ─────────────────────────────────────────────

/// `Path` を UTF-8 文字列に変換する。非 UTF-8 パスでは `Err` を返す。
fn path_str(p: &Path) -> Result<&str> {
    p.to_str().context("パスが UTF-8 ではありません")
}

/// `src` を `dest` へハードリンク優先でコピーする。
/// `cp -al` が失敗した場合（ファイルシステムが異なる等）は `cp -r` にフォールバックする。
fn cp_link_or_copy(src: &Path, dest: &Path) -> Result<()> {
    // Windows に `cp` は無い（Git for Windows の物に頼らない）＝ Rust の中で同じことをする
    if cfg!(windows) {
        return link_or_copy_tree(src, dest);
    }
    let ok = matches!(
        std::process::Command::new("cp")
            .args(["-al", path_str(src)?, path_str(dest)?])
            .status(),
        Ok(s) if s.success()
    );

    if !ok {
        let _ = fs::remove_dir_all(dest); // 失敗した残骸を掃除
        let status = std::process::Command::new("cp")
            .args(["-r", path_str(src)?, path_str(dest)?])
            .status()
            .context("cp -r の起動に失敗しました")?;

        if !status.success() {
            anyhow::bail!("コピーに失敗しました: {:?} → {:?}", src, dest);
        }
    }

    Ok(())
}

/// `cp -al src dest` と同じ形: 木を辿り、ファイルは 1 本ずつハードリンク（別のボリューム等で
/// 失敗したらコピー）、ディレクトリは作る。`dest` は無い前提（在れば中へ足す）。
fn link_or_copy_tree(src: &Path, dest: &Path) -> Result<()> {
    if fs::symlink_metadata(src)?.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            link_or_copy_tree(&entry.path(), &dest.join(entry.file_name()))?;
        }
    } else if fs::hard_link(src, dest).is_err() {
        fs::copy(src, dest).with_context(|| format!("コピーに失敗しました: {:?} → {:?}", src, dest))?;
    }
    Ok(())
}

/// `src_root` 内の各エントリを `dest_root` へハードリンク優先でコピーする。
/// 既に存在するエントリはスキップする（べき等）。
fn sync_gem_dirs(src_root: &Path, dest_root: &Path) -> Result<()> {
    if !src_root.exists() {
        return Ok(());
    }
    fs::create_dir_all(dest_root)?;

    for entry in fs::read_dir(src_root)? {
        let entry = entry?;
        let dest = dest_root.join(entry.file_name());
        if !dest.exists() {
            // ベストエフォート: 個別エントリの失敗は無視して続行
            let _ = cp_link_or_copy(&entry.path(), &dest);
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────
// arc init
// ─────────────────────────────────────────────

pub fn init(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path).context("プロジェクトディレクトリの作成に失敗しました")?;
    }

    let project = FluxProject::init(path)?;

    // デフォルト config.toml を生成
    let config = ArcConfig::default();
    config.save(&project.flux_dir)
        .context("config.toml の初期化に失敗しました")?;

    let signal = project.record(
        SignalType::ProjectCreated,
        json!({
            "path": path,
            "version": env!("CARGO_PKG_VERSION"),
            "ruby_version": config.ruby.version,
        }),
    )?;

    eprintln!("✨ Flux project initialized at {:?}", path);
    eprintln!("   Signal: {} ({})", signal.id(), signal.name());
    eprintln!("   Ruby:   {} (change with `arc bootstrap <version>`)", config.ruby.version);

    Ok(())
}

// ─────────────────────────────────────────────
// arc state
// ─────────────────────────────────────────────

pub fn state(json_output: bool, raw: bool, diff: bool, type_filter: Option<String>) -> Result<()> {
    let cwd = env::current_dir()?;
    let project = FluxProject::open(&cwd)?;
    let signals = project.read_signals()?;

    let filtered: Vec<_> = match &type_filter {
        Some(t) => signals.iter().filter(|s| s.name() == *t).collect(),
        None    => signals.iter().collect(),
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
        return Ok(());
    }

    if raw {
        return display::render_raw(&filtered, &project.flux_dir);
    }

    if diff {
        return display::render_diff(&signals);
    }

    display::render_full(&signals, &cwd)
}

// ─────────────────────────────────────────────
// arc exec
// ─────────────────────────────────────────────

pub fn exec(args: &[String]) -> Result<()> {
    if args.is_empty() {
        anyhow::bail!("コマンドを指定してください。Usage: arc exec <command> [args...]");
    }
    let cwd = env::current_dir()?;
    let project = FluxProject::open(&cwd)?;
    let (cmd, cmd_args) = (&args[0], &args[1..]);

    eprintln!("🚀 arc exec: {}", display::fmt_cmd(cmd, cmd_args));

    runner::run_with_flux(
        &project,
        "exec",
        cmd,
        cmd_args,
        &cwd,
        ArcEnv::System,
    )
}

// ─────────────────────────────────────────────
// arc sync
// ─────────────────────────────────────────────

pub fn sync() -> Result<()> {
    let cwd = env::current_dir()?;
    let project = FluxProject::open(&cwd)
        .context("Flux プロジェクトが見つかりません。`arc init` を実行してください。")?;
    install_with(&project, &cwd)
}

/// `FluxProject` を受け取って bundle install を実行する内部ヘルパー。
/// `add`/`remove`/`undo` から再利用することで `FluxProject::open()` の二重呼び出しを防ぐ。
/// 実行前にキャッシュから Gem を復元し、実行後にキャッシュへ保存する。
fn install_with(project: &FluxProject, cwd: &Path) -> Result<()> {
    if !cwd.join("Gemfile").exists() {
        anyhow::bail!("Gemfile が見つかりません。");
    }

    // config.toml から Ruby API バージョンを取得
    let config = ArcConfig::load(&project.flux_dir)?;
    let ruby_api_ver = crate::config::ruby_api_version(&config.ruby.version);

    // 1. キャッシュから既存の Gem を復元 (Binary Install 相当)
    let _ = restore_gems(cwd, &ruby_api_ver);

    eprintln!("⚡ arc: bundle install → {}", crate::signals::ARC_ENV_DIR);

    let args = vec!["install".to_string()];
    runner::run_with_flux(
        project,
        "sync",
        "bundle",
        &args,
        cwd,
        ArcEnv::Isolated,
    )?;

    // 2. 新しく入った Gem をキャッシュに保存 (将来のプロジェクト用)
    let _ = harvest_gems(cwd, &ruby_api_ver);

    Ok(())
}

// ─────────────────────────────────────────────
// Gem キャッシュ (Harvest & Restore)
// ─────────────────────────────────────────────

/// プロジェクト内の Gem をグローバルキャッシュに保存する（ベストエフォート）。
fn harvest_gems(cwd: &Path, ruby_api_ver: &str) -> Result<()> {
    let gem_cache = crate::signals::get_global_gems_dir();
    let local_base = cwd
        .join(crate::signals::ARC_ENV_DIR)
        .join("ruby")
        .join(ruby_api_ver);

    if !local_base.exists() {
        return Ok(());
    }

    for subdir in GEM_SUBDIRS {
        let _ = sync_gem_dirs(&local_base.join(subdir), &gem_cache.join(subdir));
    }
    Ok(())
}

/// グローバルキャッシュからプロジェクト内へ Gem を復元する（ベストエフォート）。
fn restore_gems(cwd: &Path, ruby_api_ver: &str) -> Result<()> {
    let gem_cache = crate::signals::get_global_gems_dir();
    if !gem_cache.exists() {
        return Ok(());
    }

    let local_base = cwd
        .join(crate::signals::ARC_ENV_DIR)
        .join("ruby")
        .join(ruby_api_ver);

    for subdir in GEM_SUBDIRS {
        let _ = sync_gem_dirs(&gem_cache.join(subdir), &local_base.join(subdir));
    }
    Ok(())
}

// ─────────────────────────────────────────────
// arc run
// ─────────────────────────────────────────────

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        anyhow::bail!("実行するコマンドを指定してください。");
    }
    let cwd = env::current_dir()?;
    let project = FluxProject::open(&cwd)
        .context("Flux プロジェクトが見つかりません。`arc init` を実行してください。")?;

    let (cmd, cmd_args) = (&args[0], &args[1..]);
    runner::run_with_flux(
        &project,
        "run",
        cmd,
        cmd_args,
        &cwd,
        ArcEnv::Isolated,
    )
}

// ─────────────────────────────────────────────
// arc env
// ─────────────────────────────────────────────

pub fn env() -> Result<()> {
    let cwd = env::current_dir()?;
    let env_dir = cwd.join(crate::signals::ARC_ENV_DIR);
    let ruby_bin_path = ruby_bin(&env_dir);

    eprintln!("⚡ arc env");
    eprintln!();
    eprintln!("  Project:   {}", cwd.display());
    eprintln!("  ARC_ENV:   {}", env_dir.display());
    eprintln!("  GEM_HOME:  {}", gem_home(&env_dir).display());
    eprintln!("  Ruby:      {}",
        if ruby_bin_path.exists() { ruby_bin_path.display().to_string() }
        else { "(not bootstrapped — run `arc bootstrap`)".to_string() }
    );

    // Ruby バージョンを実際に走らせて表示（共有ライブラリを解決してから実行）
    if ruby_bin_path.exists() {
        let mut cmd = std::process::Command::new(&ruby_bin_path);
        cmd.arg("--version");

        // LD_LIBRARY_PATH を設定 (runner と同じロジックを共有)
        if let Some(ld_path) = build_ld_library_path(&env_dir) {
            cmd.env("LD_LIBRARY_PATH", ld_path);
        }

        if let Ok(o) = cmd.output() {
            let ver = if !o.stdout.is_empty() {
                String::from_utf8_lossy(&o.stdout).to_string()
            } else {
                String::from_utf8_lossy(&o.stderr).to_string()
            };
            eprintln!("  Version:   {}", ver.trim());
        }
    }

    eprintln!();
    Ok(())
}

// ─────────────────────────────────────────────
// arc shell
// ─────────────────────────────────────────────

pub fn shell() -> Result<()> {
    let cwd = env::current_dir()?;
    let project = FluxProject::open(&cwd)
        .context("Flux プロジェクトが見つかりません。`arc init` を実行してください。")?;

    // 起動するシェルを決定: $SHELL > /bin/bash（Windows は $SHELL > %COMSPEC% > cmd.exe）
    let shell_bin = env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
            env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        } else {
            "/bin/bash".to_string()
        }
    });

    eprintln!("🐚 arc shell: entering isolated environment");
    eprintln!("   Shell:   {}", shell_bin);
    eprintln!("   GEM_HOME: {}", gem_home(&cwd.join(crate::signals::ARC_ENV_DIR)).display());
    eprintln!("   Type 'exit' to leave the arc environment.");
    eprintln!();

    let mut command = std::process::Command::new(&shell_bin);
    inject_isolated_env(&mut command, &cwd)?;

    // ARC_SHELL=1 をセットしておくと、PS1 等でカスタマイズできる
    command.env("ARC_SHELL", "1");

    // `arc shell` は寿命を持つサブシェル ⇒ session。sid が start と end を綴じる。
    let sid = Uuid::now_v7().to_string();
    project.record(
        SignalType::SessionStart,
        json!({
            "sid":   &sid,
            "shell": &shell_bin,
            "cwd":   cwd.to_string_lossy(),
        }),
    )?;

    // インタラクティブシェルを起動。ユーザーが exit するまでブロック。
    let status = command
        .status()
        .map_err(|e| anyhow::anyhow!("シェル '{}' の起動に失敗しました: {}", shell_bin, e))?;

    let exit_code = status.code().unwrap_or(0);
    project.record(
        SignalType::SessionEnd,
        json!({
            "sid":       &sid,
            "exit_code": exit_code,
            "how":       "clean",
        }),
    )?;

    eprintln!();
    eprintln!("🐚 arc shell: exited (code: {})", exit_code);

    Ok(())
}

// ─────────────────────────────────────────────
// arc add
// ─────────────────────────────────────────────

pub fn add(gem_name: &str, version: Option<&str>) -> Result<()> {
    let cwd = env::current_dir()?;
    let project = FluxProject::open(&cwd)
        .context("Flux プロジェクトが見つかりません。`arc init` を実行してください。")?;

    let gemfile_path = cwd.join("Gemfile");
    let added = gemfile::add_gem(&gemfile_path, gem_name, version)?;

    if added {
        eprintln!("➕ Added '{}' to Gemfile", gem_name);
    } else {
        eprintln!("ℹ️  '{}' は既に Gemfile に存在します。スキップします。", gem_name);
        return Ok(()); // 変更なし → install 不要
    }

    project.record(
        SignalType::GemAdded,
        json!({ "gem": gem_name, "version": version }),
    )?;

    install_with(&project, &cwd)
}

// ─────────────────────────────────────────────
// arc remove
// ─────────────────────────────────────────────

pub fn remove(gem_name: &str) -> Result<()> {
    let cwd = env::current_dir()?;
    let project = FluxProject::open(&cwd)
        .context("Flux プロジェクトが見つかりません。`arc init` を実行してください。")?;

    let gemfile_path = cwd.join("Gemfile");
    if !gemfile_path.exists() {
        anyhow::bail!("Gemfile が見つかりません。");
    }

    let removed = gemfile::remove_gem(&gemfile_path, gem_name)?;

    if removed {
        eprintln!("➖ Removed '{}' from Gemfile", gem_name);
    } else {
        eprintln!("ℹ️  '{}' は Gemfile に見つかりませんでした。スキップします。", gem_name);
        return Ok(()); // 変更なし → install 不要
    }

    project.record(
        SignalType::GemRemoved,
        json!({ "gem": gem_name }),
    )?;

    install_with(&project, &cwd)
}

// ─────────────────────────────────────────────
// arc undo (Time Machine)
// ─────────────────────────────────────────────

pub fn undo() -> Result<()> {
    let cwd = env::current_dir()?;
    let project = FluxProject::open(&cwd)?;
    let signals = project.read_signals()?;

    // 既に取り消し済みのシグナル ID を収集する（所有型 String で保持）
    let already_undone: std::collections::HashSet<String> = signals.iter()
        .filter(|s| s.name() == "operation:reverted")
        .filter_map(|s| s.payload.get("target_id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    // 最新の「未取り消し」の add/remove を探す
    let target = signals.iter().rev().find(|s| {
        matches!(s.name().as_str(), "gem:added" | "gem:removed")
            && !already_undone.contains(s.id())
    });

    let target = match target {
        Some(s) => s,
        None    => anyhow::bail!("取り消し可能な操作（add/remove）が見つかりません。"),
    };

    let gem_name = target.payload.get("gem").and_then(|v| v.as_str())
        .context("シグナルに gem 名が含まれていません。")?;

    eprintln!("⏪ Undo: {}", target.name());

    let gemfile_path = cwd.join("Gemfile");
    match target.name().as_str() {
        "gem:added" => {
            eprintln!("   Removing '{}' from Gemfile...", gem_name);
            gemfile::remove_gem(&gemfile_path, gem_name)?;
        }
        "gem:removed" => {
            let version = target.payload.get("version").and_then(|v| v.as_str());
            eprintln!("   Restoring '{}' to Gemfile...", gem_name);
            gemfile::add_gem(&gemfile_path, gem_name, version)?;
        }
        _ => unreachable!(),
    }

    project.record(
        SignalType::OperationReverted,
        json!({
            "target_id":   target.id(),
            "target_type": target.name(),
            "gem":         gem_name,
        }),
    )?;

    install_with(&project, &cwd)
}

// ─────────────────────────────────────────────
// arc bootstrap (Global Cache 対応)
// ─────────────────────────────────────────────

fn resolve_ruby_id(version: &str) -> String {
    format!("{}-{}-{}", version, env::consts::OS, env::consts::ARCH)
}

/// Ruby の取り寄せ先の候補（先頭から試し、最初に取れた物を使う）。
///
/// - Linux: ruby-builder（GitHub Actions のツールキャッシュ）の `.tar.gz` が 1 つ
/// - Windows: RubyInstaller の持ち運べる版（`.7z`）。📏 2026-10-11 dyna: インストールせずに
///   その場から動く（`3.3.6 [x64-mingw-ucrt]`・標準ライブラリも自分の場所から読む）。
///   ⚠️ 名前に**パッケージの版** `-N` が付き、版ごとに違う（3.3.6 は `-1` と `-2`）＝ 大きい方から試す
fn resolve_ruby_urls(version: &str) -> Result<Vec<String>> {
    let builder = |suffix: &str| {
        format!(
            "https://github.com/ruby/ruby-builder/releases/download/toolcache/ruby-{}-{}.tar.gz",
            version, suffix
        )
    };
    Ok(match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64")  => vec![builder("ubuntu-24.04")],
        ("linux", "aarch64") => vec![builder("ubuntu-24.04-arm64")],
        ("windows", "x86_64") => (1..=5)
            .rev()
            .map(|n| {
                format!(
                    "https://github.com/oneclick/rubyinstaller2/releases/download/RubyInstaller-{v}-{n}/rubyinstaller-{v}-{n}-x64.7z",
                    v = version,
                    n = n
                )
            })
            .collect(),
        (os, arch) => anyhow::bail!("未対応のプラットフォームです: {} / {}", os, arch),
    })
}

/// 取り寄せと展開に使う外部コマンド。
/// Windows は **System32 の `curl.exe` / `tar.exe` を絶対パスで**起こす —— `tar.exe` は bsdtar で `.7z` を読めるが、
/// PATH に Git for Windows が居ると同名の GNU tar（`.7z` を読めない）が先に当たりうる。
fn fetch_tool(name: &str) -> PathBuf {
    if cfg!(windows) {
        let root = env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        Path::new(&root).join("System32").join(format!("{name}.exe"))
    } else {
        PathBuf::from(name)
    }
}

/// `version`: CLI 引数で指定されたバージョン。None の場合は config.toml を参照する。
pub fn bootstrap(version_arg: Option<&str>) -> Result<()> {
    let cwd = env::current_dir()?;
    let project = FluxProject::open(&cwd)
        .context("Flux プロジェクトが見つかりません。`arc init` を実行してください。")?;

    // バージョン解決: 引数 > config.toml の順で優先
    let mut config = ArcConfig::load(&project.flux_dir)?;
    let ruby_version = if let Some(v) = version_arg {
        // 引数で指定された場合は config.toml を更新して永続化
        config.ruby.version = v.to_string();
        config.save(&project.flux_dir)?;
        eprintln!("📝 Ruby version set to {} in .arc/config.toml", v);
        v.to_string()
    } else {
        config.ruby.version.clone()
    };

    let cache_dir = crate::signals::get_global_cache_dir()
        .join("rubies")
        .join(resolve_ruby_id(&ruby_version));
    let ruby_dest = cwd.join(crate::signals::ARC_ENV_DIR).join("ruby_runtime");

    // 中身の無い（半端な）ruby_runtime は、在っても無いのと同じ ＝ 消して置き直す
    if ruby_dest.exists() && !runner::ruby_bin_in(&ruby_dest).is_file() {
        eprintln!("🧹 プロジェクトの ruby_runtime に ruby が無いので置き直します: {:?}", ruby_dest);
        fs::remove_dir_all(&ruby_dest).context("半端な ruby_runtime を消せませんでした")?;
    }
    if ruby_dest.exists() {
        eprintln!("ℹ️  Ruby 実行環境は既にプロジェクト内に存在します: {:?}", ruby_dest);
        eprintln!("   バージョンを変更する場合は ruby_runtime を削除してから再実行してください。");
        // 既に置いた環境も、撃ち直せば 1 行目が直る（べき等）
        report_fixed_shebangs(fix_ruby_shebangs(&ruby_dest.join("bin")));
        return Ok(());
    }

    // 1. グローバルキャッシュにあるか確認
    // 「ディレクトリが在る」ではなく「ruby の実体が在る」で判定する —— 取り寄せが途中で止まると
    // 空（や半端）のディレクトリが残り、それを当たりとして中身の無い Ruby を置いていた
    // （📏 2026-10-11 dyna: 未対応で止まった回の空ディレクトリを「Cache Hit」と読んだ）
    let cache_hit = runner::ruby_bin_in(&cache_dir).is_file();
    if cache_hit {
        eprintln!("✨ Cache Hit: Ruby {} found in global cache.", ruby_version);
    } else {
        if cache_dir.exists() {
            eprintln!("🧹 キャッシュに半端な Ruby が残っていたので取り直します: {:?}", cache_dir);
            fs::remove_dir_all(&cache_dir).context("半端なキャッシュを消せませんでした")?;
        }
        download_ruby_to_cache(&cache_dir, &ruby_version)?;
    }

    // 2. キャッシュからプロジェクトへリンク/コピー
    eprintln!("⚡ Linking Ruby to project environment...");
    let ruby_env_dir = ruby_dest.parent()
        .context("ruby_dest の親ディレクトリが取得できません")?;
    fs::create_dir_all(ruby_env_dir)?;
    cp_link_or_copy(&cache_dir, &ruby_dest)?;
    report_fixed_shebangs(fix_ruby_shebangs(&ruby_dest.join("bin")));

    project.record(
        SignalType::RuntimeLinked,
        json!({
            "ruby_version": ruby_version,
            "cache_hit":    cache_hit,
            "dest":         ruby_dest.to_string_lossy(),
        }),
    )?;

    eprintln!("✨ Ruby {} bootstrap complete!", ruby_version);
    Ok(())
}

/// ruby-builder の Ruby は CI（`/opt/hostedtoolcache/…`）で焼かれているので、`bin/` の
/// Ruby スクリプト（`bundle` / `gem` / `rake` …）の 1 行目が**その台に無い ruby** を指す ＝
/// 起こすと ENOENT（`arc sync` が `bundle` を起こせなかった原因の 1 つ）。
///
/// `#!/usr/bin/env ruby` に書き直す（arc の子は PATH の先頭が `ruby_runtime/bin`）。
/// ⚠️ `ruby_runtime` はキャッシュとハードリンクしているので、**その場で書き換えず、
/// 外してから書き直す** ＝ キャッシュ（他のプロジェクトと共有）を汚さない。
/// 存在する ruby を指す行・`env ruby` の行・Ruby 以外のファイルは触らない。返り値は直した本数。
fn fix_ruby_shebangs(bin_dir: &Path) -> Result<usize> {
    let mut fixed = 0;
    for entry in fs::read_dir(bin_dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Ok(content) = fs::read(&path) else { continue };
        let Some(eol) = content.iter().position(|&b| b == b'\n') else { continue };
        let Some(interp) = content[..eol].strip_prefix(b"#!") else { continue };
        let interp = String::from_utf8_lossy(interp).trim().to_string();
        if !interp.ends_with("/ruby") || Path::new(&interp).exists() {
            continue;
        }
        let perms = fs::metadata(&path)?.permissions();
        let mut rewritten = b"#!/usr/bin/env ruby".to_vec();
        rewritten.extend_from_slice(&content[eol..]);
        fs::remove_file(&path)?;
        fs::write(&path, &rewritten)?;
        fs::set_permissions(&path, perms)?;
        fixed += 1;
    }
    Ok(fixed)
}

fn report_fixed_shebangs(result: Result<usize>) {
    match result {
        Ok(0) => {}
        Ok(n) => eprintln!("🔧 bin/ の Ruby スクリプト {} 本の 1 行目を `#!/usr/bin/env ruby` に直しました。", n),
        Err(e) => eprintln!("⚠️  bin/ の 1 行目を直せませんでした: {}", e),
    }
}

/// Ruby バイナリをダウンロードしてキャッシュディレクトリに展開する。
/// 失敗した場合はキャッシュディレクトリを削除してエラーを返す。
fn download_ruby_to_cache(cache_dir: &Path, ruby_version: &str) -> Result<()> {
    let urls = resolve_ruby_urls(ruby_version)?;
    let source = if cfg!(windows) { "RubyInstaller" } else { "ruby-builder" };
    eprintln!("🚀 Cache Miss: Downloading Ruby {} from {}...", ruby_version, source);
    fs::create_dir_all(cache_dir).context("キャッシュディレクトリの作成に失敗しました")?;

    // `.tar.gz`（Linux）も `.7z`（Windows）も、tar（Windows は bsdtar）が中身から形を見分ける
    let ext = if cfg!(windows) { "7z" } else { "tar.gz" };
    let tmp_archive = cache_dir.join(format!("download.{ext}"));

    // 候補を順に試し、最初に取れた物を使う（RubyInstaller はパッケージの版 `-N` が版ごとに違う）
    let mut fetched = None;
    for url in &urls {
        let ok = std::process::Command::new(fetch_tool("curl"))
            .args(["-fsL", "-o", path_str(&tmp_archive)?, url])
            .status()
            .context("curl の起動に失敗しました")?
            .success();
        if ok {
            fetched = Some(url);
            break;
        }
    }

    let Some(url) = fetched else {
        let _ = fs::remove_dir_all(cache_dir);
        anyhow::bail!("Ruby バイナリのダウンロードに失敗しました（試した先: {}）。", urls.join(" / "));
    };
    eprintln!("   ← {}", url);

    let tar_ok = std::process::Command::new(fetch_tool("tar"))
        .args([
            "-xf",  path_str(&tmp_archive)?,
            "-C",   path_str(cache_dir)?,
            "--strip-components=1",
        ])
        .status()
        .context("tar の起動に失敗しました")?
        .success();

    let _ = fs::remove_file(&tmp_archive);

    if !tar_ok {
        let _ = fs::remove_dir_all(cache_dir);
        anyhow::bail!("アーカイブの展開に失敗しました。");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_is_linked_like_cp_al() {
        let root = env::temp_dir().join(format!("arc-tree-test-{}", uuid::Uuid::now_v7()));
        let src = root.join("src");
        fs::create_dir_all(src.join("bin")).unwrap();
        fs::create_dir_all(src.join("lib").join("ruby").join("3.3.0")).unwrap();
        fs::write(src.join("bin").join("ruby.exe"), "r").unwrap();
        fs::write(src.join("lib").join("ruby").join("3.3.0").join("json.rb"), "j").unwrap();
        let dest = root.join("dest");
        link_or_copy_tree(&src, &dest).unwrap();
        assert_eq!(fs::read_to_string(dest.join("bin").join("ruby.exe")).unwrap(), "r");
        assert_eq!(fs::read_to_string(dest.join("lib").join("ruby").join("3.3.0").join("json.rb")).unwrap(), "j");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_ci_shebang_is_rewritten_without_touching_the_hardlinked_cache() {
        let root = env::temp_dir().join(format!("arc-shebang-test-{}", uuid::Uuid::now_v7()));
        let (cache, bin) = (root.join("cache"), root.join("bin"));
        fs::create_dir_all(&cache).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let ci = "#!/opt/hostedtoolcache/Ruby/3.3.6/x64/bin/ruby\nputs 1\n";
        fs::write(cache.join("bundle"), ci).unwrap();
        fs::hard_link(cache.join("bundle"), bin.join("bundle")).unwrap();
        fs::write(bin.join("ok"), "#!/usr/bin/env ruby\nputs 2\n").unwrap();
        fs::write(bin.join("sh-script"), "#!/bin/sh\necho 3\n").unwrap();

        assert_eq!(fix_ruby_shebangs(&bin).unwrap(), 1);
        assert_eq!(fs::read_to_string(bin.join("bundle")).unwrap(), "#!/usr/bin/env ruby\nputs 1\n");
        // キャッシュ側（ハードリンクの相手）は元のまま
        assert_eq!(fs::read_to_string(cache.join("bundle")).unwrap(), ci);
        assert_eq!(fs::read_to_string(bin.join("sh-script")).unwrap(), "#!/bin/sh\necho 3\n");
        // べき等: 2 回目は何もしない
        assert_eq!(fix_ruby_shebangs(&bin).unwrap(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}
