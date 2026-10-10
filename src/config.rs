//! `.arc/config.toml` の読み書きを担当するモジュール。
//!
//! ```toml
//! [ruby]
//! version = "4.0.7"
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const CONFIG_FILE: &str = "config.toml";
/// `arc init` と `arc x` の既定の Ruby。
/// 🚚 2026-10-11 に 3.3.6 → 4.0.7（その日の最新・Linux の ruby-builder と Windows の RubyInstaller の両方に在る）。
/// 3.3.6 は 2026-02 の v1 で決め打ちから引き継いだ値で、選んだ理由は記録に無い。
/// ⚠️ Windows で C 拡張を使うなら 4 系 —— MSYS2 の GCC 16 と 3.3.6 のヘッダは合わない（bigdecimal で実測）。
/// init のたびに最新を調べには行かない（オフラインで止まる・GitHub API の回数制限）＝ ここを上げる。
pub const DEFAULT_RUBY_VERSION: &str = "4.0.7";

// ─────────────────────────────────────────────
// 設定構造体
// ─────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ArcConfig {
    pub ruby: RubyConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RubyConfig {
    /// 使用する Ruby のバージョン (例: "3.3.6")
    pub version: String,
}

impl Default for ArcConfig {
    fn default() -> Self {
        Self {
            ruby: RubyConfig {
                version: DEFAULT_RUBY_VERSION.to_string(),
            },
        }
    }
}

impl ArcConfig {
    /// `flux_dir` (.arc/) 内の config.toml を読み込む。
    /// ファイルが存在しない場合はデフォルト値を返す。
    pub fn load(flux_dir: &Path) -> Result<Self> {
        let path = flux_dir.join(CONFIG_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("config.toml の読み込みに失敗しました: {:?}", path))?;
        toml::from_str(&content)
            .with_context(|| format!("config.toml のパースに失敗しました: {:?}", path))
    }

    /// `flux_dir` (.arc/) 内の config.toml に書き込む。
    pub fn save(&self, flux_dir: &Path) -> Result<()> {
        let path = flux_dir.join(CONFIG_FILE);
        let content = toml::to_string_pretty(self)
            .context("config.toml のシリアライズに失敗しました")?;
        std::fs::write(&path, content)
            .with_context(|| format!("config.toml の書き込みに失敗しました: {:?}", path))
    }
}

// ─────────────────────────────────────────────
// ユーティリティ
// ─────────────────────────────────────────────

/// Ruby バージョン文字列 (例: "3.3.6") から
/// 内部ライブラリパス用の API バージョン (例: "3.3.0") を導出する。
pub fn ruby_api_version(ruby_version: &str) -> String {
    let parts: Vec<&str> = ruby_version.splitn(3, '.').collect();
    match parts.as_slice() {
        [major, minor, _patch] => format!("{}.{}.0", major, minor),
        _ => ruby_version.to_string(), // パースできなければそのまま
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ruby_api_version() {
        assert_eq!(ruby_api_version("3.3.6"), "3.3.0");
        assert_eq!(ruby_api_version("3.4.0"), "3.4.0");
        assert_eq!(ruby_api_version("3.2.10"), "3.2.0");
    }

    #[test]
    fn test_config_serialize() {
        let config = ArcConfig::default();
        let s = toml::to_string_pretty(&config).unwrap();
        println!("{}", s);
        assert!(s.contains("[ruby]"));
        assert!(s.contains("version"));
    }

    #[test]
    fn test_config_save_load() {
        let dir = std::env::temp_dir().join("arc_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        let config = ArcConfig::default();
        config.save(&dir).unwrap();
        let loaded = ArcConfig::load(&dir).unwrap();
        assert_eq!(loaded.ruby.version, DEFAULT_RUBY_VERSION);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
