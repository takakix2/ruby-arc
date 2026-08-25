# arc 引き継ぎメモ

> **arc = Ruby 版 uv**。Rust 製 CLI（`src/` 約 1,900 行）。二枚看板で、
> ①Ruby のツールチェーン（隔離環境・グローバル hardlink キャッシュ・shim 拒否）と
> ②**Flux Core**（全操作を append-only NDJSON で記録し状態を再構築する event sourcing）。
> ⭐ **この repo が Flux Protocol の親**。詳しくは下の「系譜」。

- remote: `gitea:takaki2/ruby-arc.git`（SSH alias 経由）。branch `main`。
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

## 関連

- 仕様の正本: `~/dev/flux-protocol/spec/PROTOCOL_v1.md`（§2.05 が時刻、§3/§4 が命名、§7 が検証手順）
- 兄弟の実装: `~/dev/hsh`（journal は `~/.hsh/state/journal.jsonl`）
- 同居していた PoC: `~/dev/rsh`（Ruby Shell・hsh-hermes の前身）
