# ruby-arc（旧 arc）引き継ぎメモ

> **ruby-arc = Ruby 版 uv**。⚠️ **リポ名は 2026-08-26 に `arc` → `ruby-arc` へ改名**
> （両フォージとも旧 URL がリダイレクトで生きているので、古い clone は気づけない）。
> **打つコマンドは `arc` のまま**（バイナリ名は変えていない）。Rust 製 CLI（`src/` 約 1,900 行）。二枚看板で、
> ①Ruby のツールチェーン（隔離環境・グローバル hardlink キャッシュ・shim 拒否）と
> ②**Flux Core**（全操作を append-only NDJSON で記録し状態を再構築する event sourcing）。
> ⭐ **この repo が Flux Protocol の親**。詳しくは下の「系譜」。

- remote: `gitea:takaki2/ruby-arc.git`（SSH alias 経由）**＋ `github.com:takakix2/ruby-arc`**。
  ⚠️ **remote が 2 つある**（`main` の追跡先は `github`）。片方だけに push すると、
  次に潜る人が**古い方から枝を切る**。実際にそれが起きた（下の 2026-08-26 の節）。branch `main` のみ。
- ビルド: `cargo build`（edition 2024・cargo 1.95.0 で緑・clippy 警告 0）。
- 🚨 **m4air では Ruby を要する経路が試せない**。このマシンは system ruby 2.6.10 のみ
  （rbenv 無し）で、arc は 3.2+ 相当を前提にする。`bootstrap` / `sync` / `add` / `remove`
  は「Ruby runtime not found」で止まる。`init` / `exec` / `state` / `undo` の Gemfile 編集までは動く。

## 系譜 — arc は Flux Protocol の親であり、いま子の言葉を話す

```
arc (Rust, 2026-02)  ──→ docs/FLUX_CORE.md ──→ flux-protocol v1 ──→ hsh / Doxa / watchtower / Tabula
  └─ ruby/ に同居 ────→ rsh (Ruby) ──→ hsh-hermes の dot-chain
```

arc の ROADMAP Phase 4.4 に「Flux Protocol v1 specification document」という deliverable が
挙がっていた。**それが独立したのが `~/dev/flux-protocol`**（`spec/PROTOCOL_v1.md` +
`schema/flux-envelope.v1.schema.json` + Signal Registry）。

🚨 **独立した先で envelope が変わり、親の arc だけが取り残されていた。**
2026-08-22 にそれを解消した（下記）。

## 2026-08-22 セッション — envelope 移行 + 腐り掃除（`flux-v1-envelope` ブランチ・4 commit）

6 か月休眠（最終コミット `45e2fcb` 2026-02-24）からの再開。

### 1. Flux Protocol v1 envelope へ移行

```
移行前  {"id":"019c…","type":"add_start","payload":{"gem":"rails"},"timestamp":"…+09:00"}
移行後  {"v":1,"source":"arc","type":"gem","event":"added","ts":"…+00:00","id":"019c…","gem":"rails"}
```

非互換は **4 つ**あった（時刻だけではなかった）:

1. **`v` / `source` が無い** — 必須 4 フィールドのうち 2 つが欠けていた。
2. **`type` に動作が入っていた** — v1 §3 は `type`=名詞、§4 は `event`=過去分詞。
   `add_start` は 2 つを 1 フィールドに畳んでいた。
3. 🚨 **アンダースコアが §3/§4 の `^[a-z0-9-]+$` に違反** — `add_start` / `shell_enter` が全滅。
   **hsh が `job:job_started` で踏んだのと同じ罠**（registry にその顛末が残っている）。
   → 自由形式の口だった **`SignalType::Custom(String)` を削除**した。enum が唯一の入口。
4. **`payload` が入れ子だった** — §2「他の全フィールドが payload」＝トップレベルに平す。

**時刻は `ts`**（§2.05）。旧 `timestamp` は §2.05.3 のフォールバック鎖
`ts ?? modified ?? timestamp` に名指しで載っている旧綴りだった。UTC・マイクロ秒・
明示的オフセットで書く（hsh と同じ時計・同じ精度）。

**`type:event` の対応表**（`src/signals.rs` の `SignalType` が正本）:

| 旧 | 新 |
|---|---|
| `init` | `project:created` |
| `exec_start` / `install_start` / `run_start` | `cmd:run`（+ `verb` で入口を区別） |
| `exec_end` / `install_end` / `run_end` | `cmd:end` |
| `add` / `remove` | `gem:added` / `gem:removed` |
| `bootstrap` | `runtime:linked` |
| `undo` | `operation:reverted` |
| `shell_enter` / `shell_exit` | `session:start` / `session:end`（`sid` で綴じる） |

⭐ **相関モデルを hsh に合わせた**のが一番効いた変更。旧: 終端の `payload.ref_id` が
始端を指す有向参照。新: **`cmd:run` と `cmd:end` が同じ `id` を名乗る**（hsh の journal と同じ）。
片方を拾ったとき、相方は group by で取れる。ペイロードの綴りも家族に寄せた
（`duration_ms` → **`ms`**、`command`+`args` → **`cmd`**（1 本の行）＋ `args`（argv の忠実な形は残す））。

**旧形式は読めるまま**（`Signal::from_legacy`）。**改名は歴史を直さない**ので、これは
一時的な措置ではない（§2.05.3 が明記）。未知の旧綴りは `legacy:recorded` に落とし、
元の文字列を `legacy_type` として payload に残す。新旧が同じファイルに混在しても読める（実測）。

### 2. 🚨 測って分かったこと — **registry が線から 12 日遅れている**

`ts` に寄せる判断の裏取りで hsh の実物を数えたところ、**仕様本文と registry が食い違っている**:

| | 綴り | 件数 |
|---|---|---|
| registry が **required** にしている | `modified` | **18**（`cmd:*` / `session:*` / `env:*` / `job:*` = hsh 系すべて） |
| 〃 | `ts` | 12（vault / Tabula 系） |
| **hsh が実際に線に出している** | **`ts`** | **447 / 447**（`modified` は 0） |

hsh のローテーション 3 本（6,798 行）に移行がそのまま残っていた: `journal.2`(〜08-02) は
`modified` のみ → `journal.1` は混在 → `journal.jsonl`(08-10〜) は `ts` のみ。
signal 別に見ると `cmd:run` / `cmd:end` / `session:*` / `alias:set` が揃って
**2026-08-10T00:06** に切り替わっている。**§2.05 の段階 2 を hsh は完了済み、registry は未反映。**

∴ 仕様 §7 の手順どおりの検証器を書いて両方に当てると:

```
arc (移行後): ok=0  warn(未登録)=1  reject=4
hsh (同じ器): ok=81 warn=0          reject=366   ← 447 行中 366 行
```

**arc は新しい欠陥を持ち込んでいない。旗艦と同じ場所に立っただけ**で、
落ちる理由も同じ「required の `modified` が無い」。緑の 81 行は `ts` を required にしている
vault レーン。⚠️ **この赤は arc の HANDOFF にだけ書くと消える**ので、flux-protocol 側に
issue を立てること（下の follow-up 1）。

### 3. 腐り掃除

- **`ruby/` は壊れた gitlink だった** — mode `160000` → commit `08d269d`＝**rsh のコミット**。
  `.gitmodules` が無いので clone すると空ディレクトリだけができる。`93ce642`(2/21) で
  リポを分離したのに index に残骸が残っていた。→ untrack。
- **`arc_description.txt` / `implementation_plan.md` / `ruby版uv_arc正式版.md`** は
  **tracked かつ .gitignore 済み**という矛盾状態だった（`6481deb` が「# Local Design Docs」として
  除外を書いたが、既に commit 済みなので効かない）。**退役の意思は .gitignore に既に記録されていた**
  ので実行に移した。→ untrack（ディスク上と git 履歴には残る）。
- **ROADMAP Phase 2 が「🔜 Next」のままだった** — `578f23c` と `src/state.rs` が実在。→ ✅ に。
- **ROADMAP Phase 4.4「Flux Protocol v1 specification document」が未着手のままだった** —
  独立リポとして**とっくに出荷されている**。→ ✅ に、かつ「envelope が変わった」ことを明記。
- `docs/FLUX_CORE.md` の Signal 仕様節は**仕様の写しをやめ**、正本を flux-protocol に譲って
  「arc が何を出すか」だけを書く形に（写しを置くと必ず片方が腐る）。
- README / README(J) / cli.rs の `--type` ヘルプも新しい綴りに追随。

## 2026-08-26 セッション — ブランチを main へ着地 ＋ `ruby-arc` へ改名追随

**`flux-v1-envelope` は main に入った**（`2dfb6a0`・`--no-ff`・衝突ゼロ・14 ファイル / +659 −480）。
⚠️ それまで **`main` に `HANDOFF.md` が存在しなかった** ＝ この文書ごとブランチに隠れていた。

**検証**: `cargo build` exit 0 / `cargo test` 8 passed 0 failed / `cargo clippy --all-targets` 警告 0。

### 🚨 なぜ枝が古い所から生えていたか — remote が 2 つで、Gitea 側が 5 か月古かった

8/22 のセッションは **Gitea の `origin/main`（`45e2fcb`・2026-02-24）から枝を切った**。
`main` の追跡先は `github/main`（`27acece`・7/03）だったので、**`ruby` gitlink の untrack を
二重にやっていた**（`27acece` と `967fcb5`）。マージでは no-op になったので実害は出ていない。

⭐ **両 remote を `8347411` で揃えた**（Gitea 側は 5 か月ぶんの断絶が解消）。

### 改名（`arc` → `ruby-arc`）

push したら GitHub が `This repository moved` と答えた —— **旧 URL がリダイレクトで生きており、
push は成功していた**。⇒ ディレクトリ（`~/dev/ruby-arc`）・両 remote URL・README ×2 の clone 手順・
`docs/RELEASE.md`・この文書の remote 行を揃えた。旧名の残骸は全文検索で **0 件**。

📏 **名乗り方が非対称**（同じ日に 4 通り実測）:

| | ref が動く push | no-op push |
|---|---|---|
| **GitHub** | ⭐ 移動を名乗る（新 URL 付き） | 黙る |
| **Gitea** | 🚨 黙る（`To …` も旧 URL のまま） | 黙る |

⚠️ **`git ls-remote` は両方黙る**（旧 URL で新しい SHA を返す）＝ **取る側は最後まで無警告**。
∴ `git remote -v` を目で見るか、新旧 URL の `ls-remote` が同じ SHA を返すかで突き合わせる。

⚠️ **バイナリ名は `arc` のまま**。README も `arc init .` のまま ——
**人が打つ名前はリポ名とは別の決定**なので、改名に巻き込んでいない。

## 2026-10-11 セッション（dyna の Claude 席）—— 隔離環境が使えなかった 4 つを直し、Windows で焼いて撃った（`3af2a74`）

Rapi（Ruby の小さな Web フレームワーク）のテストを `arc run` で撃とうとして、arc 側で 4 つ踏んだ。
🚨 **どれも「Ruby を要する経路」**で、m4air では試せない（上の注記）ため長く見えていなかった。

| | 何が起きていたか | 直し |
|---|---|---|
| ① | 子の PATH の先頭が **`ruby_runtime/bin/ruby`（ファイル）** —— `b9b6997`（2026-02-19「clippy の警告を直す」）で `ruby_runtime_bin` が `ruby_bin` に化けた。子の中から `bundle` / `rake` が 1 つも引けない | `isolated_bin_dirs()`（`ruby_runtime/bin` と `.arc/env/bin`）を先頭に |
| ② | `Command::new("bundle")` は**子に渡す PATH ではなく arc 自身の PATH** で探す ＝ 素の PATH に `bundle` が無い台（blackcube）では `arc sync` が `No such file or directory` | 裸の名前は隔離環境の bin で先に解決（`resolve_isolated_program()`）|
| ③ | ruby-builder の Ruby は CI で焼かれており、`bin/` のスクリプトの 1 行目が **`#!/opt/hostedtoolcache/Ruby/3.3.6/x64/bin/ruby`**（どの台にも無い）＝ 絶対パスで起こしても ENOENT | `bootstrap` が `#!/usr/bin/env ruby` に書き直す（`fix_ruby_shebangs()`）。⚠️ `ruby_runtime` はキャッシュと**ハードリンク**なので、**外してから書き直す**（共有キャッシュを汚さない・テストで固定）。既に bootstrap 済みでも撃ち直せば直る（べき等）|
| ④ | `GEM_HOME` が `.arc/env`、bundler が入れるのは `.arc/env/ruby/<API 版>/` ＝ `arc sync` で入れた gem が `arc run ruby x.rb` から見えない | `gem_home()` で bundler と同じ所を指す（`BUNDLE_PATH` は `.arc/env` のまま）。`arc env` / `arc shell` の表示も同じ関数から |

- 📏 Rapi の `test_router.rb` を `arc run` で → **117 件・rc=0**（Ruby 3.3.6）。直す前は `require "rack"` で止まった
- 📏 `cargo test` 12 件（③④①② の検体を足した）・clippy 0
- 🚚 **blackcube の `~/.local/bin/arc` を `3af2a74` に入れ替えた**。前の版（2026-02-19・①を踏む前の焼き）は `arc.bak-20260219`
- ✅ Gitea と GitHub の両方へ push（`main` = `origin/main` = `github/main` = `3af2a74` を実測）

### Windows で焼いて撃った（dyna・msvc）

**焼ける**（`cargo build --release` が通る・unix 専用のコードは無い）。📏 撃った結果:

| 動く | 止まる |
|---|---|
| `init` / `state` / `exec`（`cmd /c echo hello`）/ `env` | `bootstrap` —— 「未対応のプラットフォームです: windows / x86_64」（`resolve_ruby_url` が Linux 2 種しか持たない）→ Ruby が無いので `run` / `add` / `shell` も「Ruby runtime not found」 |

▶ **土台は動き、止まっているのは「Ruby をどこから取り、どう置くか」だけ**（🧑「整備すると動くようですね」）。移植で要る物（📌 読んだ範囲・未着手）:
- Ruby の出所: RubyInstaller の持ち運べる版（`.7z`）—— Windows 標準の `tar.exe`（bsdtar）で展開できるはず（**未確認**）
- `cp -al` / `cp -r`（外部コマンド）→ Rust の中でのハードリンク・コピー（Windows に `cp` は無い。Git for Windows の `cp` に頼らない）
- `bin/ruby` → `ruby.exe`、`LD_LIBRARY_PATH` は Windows では意味が無い、`RUBYLIB` の arch ディレクトリ名（`x64-mingw-ucrt`）
- `arc shell` は `$SHELL`（dyna では Git の bash を起こした）—— hsh を起こしたいなら `SHELL` を hsh に
- ⚠️ **macOS も同じ所で止まる**（`resolve_ruby_url` に darwin が無い）—— m4air で Ruby の経路が試せない理由の半分はこれ

### 同日の続き —— Windows 移植（`fd3533f`）✅ Rapi のテストが dyna で通った

📏 **dyna: `arc bootstrap` → `arc sync` → `arc run ruby test_router.rb`（Rapi）で 117 件・rc=0**（Ruby 3.3.6 x64-mingw-ucrt）。dyna にシステムの Ruby は入れていない（`.arc/` の中だけ）。

| 変えた所 | 中身 |
|---|---|
| Ruby の出所 | `resolve_ruby_urls()`: Windows は **RubyInstaller の持ち運べる `.7z`**。名前の**パッケージの版 `-N` が版ごとに違う**（3.3.6 は `-1` と `-2`）ので `-5`→`-1` の順に試し、最初に取れた物（3.3.6 は `-2`・15MB）|
| 取り寄せ・展開 | `fetch_tool()`: Windows は **System32 の `curl.exe` / `tar.exe` を絶対パスで** —— bsdtar は `.7z` を読める（📏 確認）が、PATH に Git for Windows が居ると GNU tar（読めない）が先に当たりうる。展開は `-xf`（Linux も gzip を中身から見分ける）。⚠️ curl は `-s`（進捗バーを出さなくなった・候補の 404 を黙らせるため）|
| 置き方 | `link_or_copy_tree()`: Windows は `cp -al` の代わりに Rust の中で 1 ファイルずつハードリンク（失敗したらコピー）。Linux は今までどおり `cp -al` |
| 名前 | `ruby_bin_in()` が `ruby.exe`。裸の名前は `.exe` → `.bat` → `.cmd` → 素の順（`bundle.bat`。拡張子なしの `bundle` は Ruby のスクリプトで直接は起こせない）|
| 環境変数 | Windows では `LD_LIBRARY_PATH` と `RUBYLIB` を足さない —— 📏 RubyInstaller は自分の置き場所から標準ライブラリを読む。`bundle.bat` は `%~dp0ruby.exe` を起こすので、Linux の「CI の 1 行目」問題は起きない |
| `arc shell` | `$SHELL` が無ければ `%COMSPEC%` |

**Windows 以外にも効くバグを 2 つ直した**:
- 🚨 **キャッシュの当たりが「ディレクトリが在る」だった** —— 取り寄せが途中で止まる（や、`resolve` で bail する前に `create_dir_all` していた旧版）と**空のディレクトリが残り、次の bootstrap がそれを「Cache Hit」として中身の無い Ruby を置いた**（📏 dyna で実際に起きた: 未対応で止まった回の空ディレクトリ）。→ 当たりは「`bin/ruby` の実体が在る」。半端なキャッシュ・ruby の無いプロジェクトの `ruby_runtime` は消して置き直す
- **`HOME` が無いとキャッシュが `/tmp`** —— Windows の素の席（cmd / hsh）は `HOME` を持たないことがあり、`/tmp` は今のドライブの `\tmp`。→ `USERPROFILE` を先に見る

- 📏 テスト 13 件（`a_tree_is_linked_like_cp_al` を足した）・clippy 0 —— dyna（msvc）と blackcube の両方。blackcube は新しいプロジェクトと Rapi を arc で撃ち直して変わらず
- 🚚 blackcube の `~/.local/bin/arc` を `fd3533f` に。⏸ **dyna の `~/.local/bin` には入れていない**（scratchpad で焼いた物で撃った・入れるかは 🧑 の判断待ち）　🚚 **同日に置いた**（Follow-Ups 9）
- ✅ Gitea / GitHub とも `fd3533f`。README の Windows の行を「Planned」→「🔶 一部」に

⏸ **残る壁: C 拡張をその場でコンパイルする gem**。📏 Rapi の `Gemfile.lock` で `bigdecimal 4.1.3`（sequel の依存）が `MSYS2 could not be found. Please run 'ridk install'` で入らず、**`arc sync` は rc=1**（rack と sqlite3 はビルド済みの物が配られているので入る）。RubyInstaller の持ち運べる版には DevKit（MSYS2）が無い。∴ **sequel を使う Pharos は Windows ではここで止まるはず**（撃っていない）。案: ① arc が MSYS2 も取ってくる（数百 MB〜GB）② ビルド済みの物が在る版に lock を合わせる

📌 **別件（Linux で見た）**: blackcube の新しいプロジェクトで `Ignoring json-3.0.2 because its extensions are not built` が出た —— 同日に `arc add json` / `arc undo` を試したプロジェクトから、グローバルの gem キャッシュ（`harvest_gems` / `restore_gems`）に C 拡張の無い json が入って戻されたように見える（**未調査**）

### 同日の続き —— `--devkit`（C 拡張）と Ruby 4.0.7（`9f3116c`）✅ Pharos が Windows で 15/15

🧑 の方針: 古い同梱の bigdecimal に lock を合わせる案は**採らない**（「Ruby はもう 4 系、離れすぎ」）。**Windows は `--devkit` 方式**。

- **`arc bootstrap --devkit`**（Windows）: キャッシュの Ruby の中に MSYS2 の素の版（`msys2-x86_64-latest.tar.xz`・43MB）を **`msys64`** として展開し、`ridk install 3`（MINGW toolchain）→ プロジェクトへハードリンク
  - ⭐ RubyInstaller は MSYS2 を**自分の置き場所の中の `msys64` から最初に探す**（`msys2_installation.rb`）＝ `.arc/` の中で完結。with Devkit 版の `.exe`（138MB）はインストーラーで、アンインストール情報を登録するので使わない
  - 用意できたかは `msys64/ucrt64/bin/gcc.exe` の実体で判定。半端な `msys64` は作り直す。bootstrap 済みのプロジェクトにも後から足せる。Linux / macOS では何もしない（「システムのコンパイラ」と言う）
  - 📏 約 1GB（`msys64` 994MB）・初回 115 秒（Ruby ＋ MSYS2 ＋ toolchain）。MSYS2 のミラーで 404 / タイムアウトが数回出たが pacman が別のミラーで取り直した
  - **既定では入れない** —— コンパイル済みの物が配られている gem（rack・sqlite3）は要らない。Rapi は要らず、Pharos は要る
- **GCC 16 × Ruby 3.3.6 は通らない** —— 📏 MSYS2 の今の GCC（16.2）で、3.3.6 のヘッダと bigdecimal 4.1.3 が `incompatible-pointer-types` でエラー（`-Wno-error=…` を付ければ通る）。**4.0.7 では出ない**（Pharos の 6 本がそのまま通った）。∴ Windows で C 拡張を使うなら 4 系
- **ruby-builder が配り方を変えていた** —— 新しい版（4.0.x・3.4.7+）は版ごとのリリース `ruby-<v>/ruby-<v>-ubuntu-24.04-x64.tar.gz`（x64 に `-x64`）で、`toolcache` には無い（4.0.7 / 3.4.11 は旧い形で 404）。→ 新しい形を先に、無ければ `toolcache`。**macOS（`darwin-arm64` / `darwin-x64`）も新しい形に在る**ので足した（Mac では未撃ち）

📏 撃った結果:

| | Ruby | 結果 |
|---|---|---|
| dyna（Windows・`--devkit`）| 4.0.7（RubyInstaller `-1`）| Pharos の `arc sync` が io-event / json / openssl / bigdecimal / nio4r / puma をコンパイル → `test_pharos.rb` **15/15** |
| blackcube（Linux）| 4.0.7（新しい形から）| Rapi **117/117** ／ bigdecimal 4.1.3 も入る |
| 両方 | — | テスト 13 件・clippy 0 |

- 🚚 blackcube の `~/.local/bin/arc` を `9f3116c` に。✅ Gitea / GitHub とも `9f3116c`。README の Windows の行を ✅ に
- 📌 計器: 最初に bigdecimal を手で撃ったとき「`ruby/ruby.h` が無い」と出たのは、**撃つ側が付けた `MSYS_NO_PATHCONV=1` が `make` に継がれ、MSYS の `/C/…` → `C:/…` 変換を止めた**ため。▶ DevKit を撃つときは git-bash を挟まない（PowerShell から）
- 📌 dyna の `~/.arc/cache` に Ruby 4.0.7 ＋ `msys64` で約 1.1GB（プロジェクト側はハードリンク）

### 同日の続き —— `arc x` と既定の Ruby 4.0.7（`6d667f5`）

🧑「Windows で Ruby の管理が uv みたいに使えたら楽」→ uv と比べて足りない物のうち **1（init の既定の版）と 2（プロジェクト無しで 1 回撃つ口）** をやった。

**1. 既定の Ruby を 4.0.7 に**（`config::DEFAULT_RUBY_VERSION`・`pub` にして `arc x` と共有）
- 3.3.6 は 2026-02-18 の v1（`6ef83da`「決め打ちの `RUBY_VERSION` 定数を外す」）で決め打ちから引き継いだ値で、**選んだ理由は記録に無い**（🧑「なんで 3.3.6 選んだか忘れた」）
- init のたびに最新を調べには行かない（オフラインで止まる・GitHub API の回数制限）＝ 上げるときはこの値を上げる

**2. `arc x [--ruby V] [--with GEM]... [--devkit] -- <ruby の引数>`**（uvx / npx に当たる）
- キャッシュの Ruby を直接起こす。**プロジェクトも `.arc/` も記録（Flux）も作らない**
- `--with` の gem（`name` / `name:version`）は「Ruby の版 × gem の組」ごとのキャッシュ（`~/.arc/cache/x/<ruby>/<組>`・印は `.arc-x-ready`）に入れて使い回す。📏 rack: 1 回目 1.6 秒 → 2 回目 0.2 秒
- gem install の出力は **stderr へ**（stdout は台本の物 ＝ `arc x … | jq` が 1 回目だけ汚れていた）。終了コードはそのまま返す
- 🚨 **`--with` の gem は有効にしてから `$LOAD_PATH` の先頭へ**（`gem_activations()`・bundler と同じ扱い）—— 📏 `--with json:2.7.2` で、① Ruby に同梱の json 2.18.0 が勝った（RubyGems は手元で一番新しい版を選ぶ）② Linux では `gem "json", "2.7.2"` で有効にしても `require` が `RUBYLIB` の標準ライブラリの json.rb を先に読んだ（RubyGems は gem の場所を `RUBYLIB` / `-I` の**後ろ**に差し込む）。📏 プロジェクト（`arc run bundle exec` / `-rbundler/setup`）は bundler が先頭に置くので 2.7.2 が効いていた ＝ 影響は `arc x` だけだった
- 環境変数の組み立てを `apply_ruby_root_env()`（Ruby の木を受け取る）に 1 本化し、プロジェクトの `inject_isolated_env` もそこを通す。📏 キャッシュの Linux の Ruby は **`LD_LIBRARY_PATH` が無いと起動しない**（`libruby.so.4.0`）、**`RUBYLIB` が無いと標準ライブラリを見つけられない**（ruby-builder は CI のパスでビルド ＝ 置き場所からは読めない）—— ⚠️ ∴ Linux の `RUBYLIB` は外せない
- 📏 dyna / blackcube の両方: 4.0.7・`--ruby 3.3.6`・`--with rack` / `rack:3.1.16` / `json:2.7.2`（→ 2.7.2）・`exit 7`（→ 7）・台本の引数（`"b c"` が 1 つ）・カレントに何も作らない。Rapi 117・Pharos 15/15（`arc run`）・テスト 15 件・clippy 0
- 🚚 blackcube / dyna の `arc` を `6d667f5` に。✅ Gitea / GitHub とも `6d667f5`。README（EN / JA）のコマンド表に `arc x`、例の版を 4.0.7 に
- 📌 計器: Python をヒアドキュメントで渡して Rust を書き換えたら **`\n` が本物の改行に化けた**（2 か所・テストが赤で気づいた）。▶ Rust の文字列を含む書き換えは Edit ツールで

### 同日の続き —— arc を止めたら子も止まる（`ecc3c7b`・Follow-Ups 14）

📏 踏んだ形: Rapi #7 を `arc x --with puma … -- -S puma …` で撃ち、`arc` の PID に `kill` → **Puma（PPID 1）が残ってポートを握り続けた**。arc は子を `status()` で待つだけで、受けたシグナルを子に渡していなかった。直しは `src/commands/child.rs` に集めた:

| | 直し |
|---|---|
| `arc x`（Linux / macOS）| 記録を残さないので **`exec` で ruby に置き換わる**（uvx と同じ）＝ 止めた PID が ruby |
| `arc run` / `arc shell`（Linux / macOS）| 終わりを記録するので置き換えられない ＝ **SIGTERM / SIGHUP / SIGQUIT を子へ渡して待つ**（`signal-hook`）。⚠️ **SIGINT は渡さず受け流す** —— 端末の Ctrl+C はプロセスグループ全員に届くので、渡し直すと子が 2 回受け取る（bash が子を待つときと同じ作法）|
| Windows（3 つとも）| 子を **job object（`KILL_ON_JOB_CLOSE`）** に入れる ＝ arc が消えれば子も消える（`windows-sys` 0.59）。arc 自身の Ctrl+C は**子を起こした後で**、継がれない handler 関数で受け流す —— 起こす前の `SetConsoleCtrlHandler(NULL, TRUE)` は子に継がれて、子が Ctrl+C を無視する |
| 記録の終了コード | シグナルで死んだ子は **128 + シグナル番号**（それまで一律 1）|

📏 直す前の arc と並べて撃った:

| | 直す前 | 直した後 |
|---|---|---|
| dyna: `arc x` / `arc run` の arc を強制終了 | 子の ruby が残った | 子も消えた |
| blackcube: `arc x` を TERM | `comm=arc` で ruby が残った | `comm=ruby`（exec）で消えた |
| blackcube: `arc run` を TERM | 子が残り `cmd:end` も無し | 子も止まり `exit_code: 143` |
| blackcube: グループに SIGINT | — | 子は 1 回だけ受け取った（`exit 10 + 回数` → 11）|

- テスト 15 件・clippy 0（両方）。Rapi 134（`arc run`）。終了コードの受け渡し（`exit 7`）も変わらず
- 🚚 blackcube / dyna の `arc` を `ecc3c7b` に。✅ Gitea / GitHub とも `ecc3c7b`
- ⚠️ 撃っていない: **Windows のコンソールで実際に Ctrl+C を押したとき**（確かめたのは「arc を強制終了すると子が道連れになる」まで）
- 📌 計器: 被検体は `setsid` で別のプロセスグループに入れ、PID は起こしたときに控えた物だけを使った（名前で `pkill` しない）。⚠️ この席の `&` は SIGINT の無視を子に継がせる ＝ **SIGINT の行は直す前の版との比較に使えない**（直す前の arc も SIGINT で死ななかった）。直す前の版で残った ruby（dyna 2・blackcube 2）は args を確かめてから止めた

## Follow-Ups

1. ✅ **flux-protocol に issue を立てた** —
   [`flux-protocol#2`](https://git.navii.online/takaki2/flux-protocol/issues/2)
   「registry の 18 signals が、生産者が 2026-08-10 に書くのをやめた時刻の綴りを required にしている」。
   測定値・検証器の結果・取りうる 3 案を載せてある。arc 固有 signal
   （`project:created` / `gem:*` / `runtime:linked` / `operation:reverted`）の registry 登録も
   同じ issue に含めた（時刻の判断が決まってから、その形に合わせて登録する）。
   ⚠️ hsh の**旧** journal 3,011 行は逆に `ts` を持たないので、単純に required を `ts` へ
   置換すると今度はそちらが落ちる。歴史は書き換えられないので、どちらかは必ず赤くなる。
2. **`actor` / `actor_kind` / `on_behalf_of`** — hsh は env から身元を推定して全レコードに載せている
   （`$HSH_ACTOR` → `AI_AGENT` / `CLAUDE_CODE_ENTRYPOINT`）。「`arc add rails` を打ったのは
   人間かエージェントか」は arc でも意味を持つ。今回は envelope 合わせの範囲外として見送った。
3. **orphan な `cmd:run`** — Ruby ランタイム不在などで `inject_isolated_env` が
   start 記録の**後**に落ちると、`cmd:end` の無い始端が残る（移行前からの挙動）。
   `state.rs` は orphan として扱うので壊れないが、`cmd:end` を `ensure` 相当で必ず書くべきか要判断。
4. **ROADMAP Phase 2 の残り** — checkpoints（save/list/diff）と custom state reducers、
   `--since` / `--until`。
5. **rsh との統合配布** — `docs/RELEASE.md` に「ユーザーには一体に見せ、開発は独立」
   （依存方向 `rsh → arc`、`arc install rsh` で `~/.arc/rsh/` に展開）という構想が書いてある。
   🚨 **rsh 側の `HANDOFF.md` にはこの話が一切無い**ので、rsh に潜っただけでは見えない。
6. **`.github/` が残っている** — GitHub Sponsors の funding config（`c24d44b`）。
   公開先は Gitea なので、実態と合っているか要確認。

7. ✅ **m4air の clone を合わせた**（2026-08-26）—— `~/dev/arc` → `~/dev/ruby-arc`、
   `origin` を `gitea:takaki2/ruby-arc.git` へ、`github` を追加、`flux-v1-envelope` を削除
   （`origin/main..HEAD` が空＝取り残しゼロを確かめてから）、`main` を `fef1e99` まで ff。
   ✅ **GitHub 鍵も通した**（同日）—— `~/.ssh/config` に `Host github.com` を足し、
   **m4air の公開鍵を GitHub に登録**した（`ssh -T` → `Hi takakix2!`）。`main` の追跡先も
   blackcube と同じ `github/main` に揃え、`origin/main` = `github/main` = `main` = `fef1e99` を実測。
   🚨 **config のコピーだけでは直らなかった** —— `IdentityFile ~/.ssh/id_ed25519` は
   **各機の既定鍵**を指す相対的な綴りで、2 台の `id_ed25519` は**別の鍵**だった
   （`lkDW…`=blackcube / `jKRn…`=m4air）。⇒ 同じ config が機械ごとに違う鍵を意味する。

8. 🚨 **「片方だけに push」は 2 台とも塞がっていない** —— remote が 2 つあるのに
   **どちらのマシンも dual-push を設定していない**（`remote.*.pushurl` は両機とも空・実測）。
   ∴ 上の「次に潜る人が古い方から枝を切る」は**まだ構造的に起きる**。
   塞ぐなら `git remote set-url --add --push github <両方の URL>` で 1 回の push を
   両フォージへ流すか、正本を一本化するかの判断が要る。

9. ✅ **Windows 移植** —— `fd3533f`（bootstrap / sync / run）＋ `9f3116c`（`--devkit`）。Pharos が Ruby 4.0.7 で 15/15。
   ✅ dyna の `~/.local/bin/arc.exe` に置いた（2026-10-11・🧑・`ec323d3` から焼いた）—— git-bash でも hsh でも `arc` で引ける
10. 🔶 **macOS の bootstrap** —— `9f3116c` で `darwin-arm64` / `darwin-x64` を足した。**m4air で撃っていない**（`fix_ruby_shebangs` が要るか・`DYLD_*` が要るかも未確認）。
11. **グローバルの gem キャッシュが C 拡張の無い gem を戻す？** —— `Ignoring json-3.0.2 because its extensions are not built`（2026-10-11・未調査）。
12. ✅ **Rapi / Pharos を Ruby 4.0.7 の arc 管理に揃えた**（2026-10-11・🧑）—— Rapi `747f8f7`・Pharos `d030a7d`（`arc init`・`.flux/config.toml` を git に・lock に `x64-mingw-ucrt`）。blackcube と dyna（Gitea から取り直し）の両方で全部通過、dyna で `sync` の後も lock は書き換わらない。Rapi は `--devkit` 不要、Pharos は要る（Falcon / Puma / bigdecimal）
13. **uv と比べて残っている物**（2026-10-11 に 🧑 と並べた・1 と 2 は済み）:
    ③ Ruby の版の一覧と取り寄せ（`uv python list` / `install` に当たる・今は bootstrap / `arc x` の中でだけ取る）
    ④ arc 自体の配り方（焼いた `arc.exe` を Gitea のリリースへ 等）
    ⑤ gem の解決が bundler 任せ（uv の速さには届かない・大きい話）
14. ✅ **`arc x` / `arc run` を止めても子が残る** —— `ecc3c7b` で直した（2026-10-11 の節「arc を止めたら子も止まる」）。
    ⏸ 残り: Windows のコンソールで実際に Ctrl+C を押したときは未確認

## 関連

- 仕様の正本: `~/dev/flux-protocol/spec/PROTOCOL_v1.md`（§2.05 が時刻、§3/§4 が命名、§7 が検証手順）
- 兄弟の実装: `~/dev/hsh`（journal は `~/.hsh/state/journal.jsonl`）
- 同居していた PoC: `~/dev/rsh`（Ruby Shell・hsh-hermes の前身）
