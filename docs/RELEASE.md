# arc + rsh: 配布統合戦略

## リポジトリ構成の方針

`arc` と `rsh` は**明確に分離された独立リポジトリ**として管理する。

| リポジトリ | 言語 | 役割 | 更新頻度 |
| :--- | :--- | :--- | :--- |
| `arc` | Rust | Ruby環境の管理・プロビジョニング・Flux Protocol | 低（インフラ層） |
| `rsh` | Ruby | シェル本体・REPL・The Hermes Way パイプライン | 高（シェル層） |

**依存方向**: `rsh` → `arc` が管理するRuby環境を前提とする（逆はない）。

---

## 配布時の統合イメージ

ユーザーには**一体に見える**が、開発・管理は独立させる。

### ディレクトリ構成（インストール後）

```
~/.arc/
  bin/
    arc        ← Rustバイナリ（arc本体）
    rsh        ← rsh.rb のラッパースクリプト or シンボリックリンク
  ruby/
    <version>/
      bin/ruby  ← arc が管理するRubyランタイム
  rsh/          ← arc install rsh で展開される rsh のソース
    rsh.rb
    lib/rsh/...
  config.toml   ← ruby version等の設定
```

### インストールフロー

```bash
# ユーザーは arc だけインストールすれば rsh も使える
curl -sSf https://arc.example.com/install.sh | sh
# → arc バイナリを /usr/local/bin に配置
# → arc bootstrap  (Ruby環境をプロビジョニング)
# → arc install rsh  (rsh の最新タグをダウンロードして ~/.arc/rsh/ に展開)
```

### `arc install rsh` の実装イメージ（将来）

1. `rsh` の GitHub Releases から指定バージョンの tarball をダウンロード
2. `~/.arc/rsh/` に展開
3. `~/.arc/bin/rsh` にラッパースクリプトを生成:
   ```sh
   #!/bin/sh
   exec ~/.arc/ruby/<version>/bin/ruby ~/.arc/rsh/rsh.rb "$@"
   ```

---

## Git管理ルール

- `arc/.gitignore` に `ruby/` を追加済み → サブモジュールとして登録しない
- 配布のバージョン紐付けは `arc` のリリーススクリプト（または `config.toml`）で `rsh` の対応タグを明示する
- `git submodule` は管理コストが高いため**使わない**

---

## 参照リポジトリ

- `ruby-arc`: `ssh://git@192.168.1.10:222/takaki2/ruby-arc.git` / `github.com:takakix2/ruby-arc`
- `rsh`: `ssh://git@192.168.1.10:222/takaki2/rsh.git` / `github.com:takakix2/rsh`
