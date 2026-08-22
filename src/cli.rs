use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// arc — Flux Core / Ruby 版 uv
#[derive(Parser)]
#[command(name = "arc")]
#[command(about = "Flux Core — Ruby 版 uv / 操作ログ記録・再生エンジン", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 新しい Flux プロジェクトを初期化する
    Init {
        /// プロジェクトパス（省略時はカレントディレクトリ）
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// 現在のプロジェクト状態を表示する（Flux State）
    State {
        /// JSON 形式で出力する
        #[arg(long)]
        json: bool,
        /// Signal ログの生データをテーブル表示する
        #[arg(short, long)]
        raw: bool,
        /// 直近の操作による差分を表示する
        #[arg(short, long)]
        diff: bool,
        /// 指定した種別の Signal のみを抽出する。Flux の `type:event` 表記 (例: gem:added, cmd:end)
        #[arg(short, long, name = "TYPE")]
        r#type: Option<String>,
    },
    /// 任意のコマンドを実行し、結果を Flux ログに記録する
    Exec {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Gemfile.lock と環境を同期する (bundle install のラップ)
    Sync,
    /// Gem を追加する
    Add {
        /// 追加する Gem 名
        gem: String,
        /// バージョン指定 (オプション)
        #[arg(short, long)]
        version: Option<String>,
    },
    /// Gem を削除する
    Remove {
        /// 削除する Gem 名
        gem: String,
    },
    /// 直前の Add/Remove 操作を取り消す
    Undo,
    /// プリコンパイル済み Ruby をプロジェクトに導入する
    Bootstrap {
        /// 使用する Ruby バージョン (例: 3.4.0)。省略時は .arc/config.toml の値を使用。
        version: Option<String>,
    },
    /// Flux 管理下の環境でコマンドを実行する
    Run {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// 現在の arc 環境情報を表示する (Ruby パス・GEM_HOME 等)
    Env,
    /// arc 管理下の隔離環境でインタラクティブシェルを起動する
    Shell,
}
