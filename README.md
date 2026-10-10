
# ⚡ arc

**The Ruby package manager that looks into the future.**

[![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Flux Core](https://img.shields.io/badge/Powered_by-Flux_Core-blueviolet)](docs/FLUX_CORE.md)

*The Flagship Showcase of [Flux Core](docs/FLUX_CORE.md) architecture.*

---

> **"Tools should remember."**

While other tools focus on being a faster shell, **Arc** focuses on being a **smarter engine**.
It combines the speed of `uv` with the time-traveling capabilities of **Event Sourcing**.

</div>

---

## The Problem

Every Ruby developer has been here:

```
"It was working yesterday."
"What changed?"
"I ran some commands... I think bundle install?"
```

Shell history gives you *commands*. Git gives you *file diffs*. But neither tells you **what your environment looked like** at any point in time — which gems were installed, which Ruby was active, what failed and why.

**arc** records that story. Every operation. Every outcome. Every change.

---

## What is arc?

arc is a **Ruby project manager** built on **[Flux Core](#architecture-flux-core)** — an event sourcing engine for your terminal.

Think of it as `uv` for Ruby:
- **Isolated environments** per project (no more gem conflicts)
- **Global binary cache** (install once, link everywhere — like `uv`'s hardlink cache)
- **Blazing fast** Ruby bootstrap via pre-compiled binaries
- **Full operation history** — every `add`, `remove`, `install` is a structured, immutable event

But arc goes further than `uv`:
- **`arc undo`** — reverse any `add` or `remove` with a single command
- **`arc state --diff`** — see exactly what changed since the last operation
- **Zero PATH pollution** — arc never touches your system PATH

---

## Quick Start

```bash
# Build from source
git clone https://github.com/yourname/ruby-arc.git
cd ruby-arc && cargo build --release
cp target/release/arc ~/.local/bin/

# Start a new Ruby project
mkdir my_app && cd my_app
arc init .
arc bootstrap          # Download & link Ruby 3.3.6 in ~0.07s (from cache)

# Manage gems
arc add rails
arc add rspec --version "~> 3.0"
arc sync               # bundle install with binary cache

# Run your code
arc run ruby app.rb
arc run rails server

# Enter an interactive isolated shell (NEW)
arc shell              # bash/zsh with arc env loaded — type 'exit' to leave

# See what happened
arc state
arc state --diff
```

---

## Commands

| Command | Description |
|---|---|
| `arc init [path]` | Initialize a new Flux project (creates `.flux/` and `.arc/env/`) |
| `arc bootstrap [version]` | Download & link Ruby to the project (uses global cache) |
| `arc add <gem> [--version]` | Add a gem to Gemfile and install |
| `arc remove <gem>` | Remove a gem from Gemfile and sync |
| `arc sync` | Sync environment with Gemfile.lock (like `uv sync`) |
| `arc run <cmd> [args...]` | Run a command in the isolated project environment |
| `arc shell` | **Start an interactive shell inside the isolated environment** |
| `arc exec <cmd> [args...]` | Run any command with Flux logging (system env) |
| `arc env` | Show current environment info (Ruby path, GEM_HOME, version) |
| `arc undo` | Reverse the last `add` or `remove` operation |
| `arc state` | Show full operation history and statistics |
| `arc state --diff` | Show what changed in the last operation |
| `arc state --json` | Machine-readable output (pipe to `jq`) |
| `arc state --type <type:event>` | Show only one kind of signal (e.g. `cmd:end`, `gem:added`) |

---

## Why Not shims?

Tools like `rbenv` and `rvm` intercept every `ruby` call via **shims** — thin wrapper scripts placed at the front of your `PATH`.

**arc never does this.** Here's why:

### The shim problem

```bash
# With rbenv shims:
$ ruby script.rb
# → Which ruby? rbenv's? rvm's? The system one?
# → Depends on PATH order, .ruby-version location, shell init order...
# → In CI/CD scripts, .bashrc isn't sourced → wrong Ruby
# → In VSCode, the terminal PATH differs from the integrated terminal
# → rbenv + rvm together? Officially "don't do this"
```

Shims are **implicit**. When they work, they're magic. When they break, they're a nightmare.

### The arc way

```bash
# With arc (run mode):
arc run ruby script.rb
# → Always uses .arc/env/ruby_runtime/bin/ruby
# → No PATH manipulation. No shell hooks. No surprises.
# → Works identically in terminal, CI/CD, cron, VSCode, Docker

# With arc (shell mode — for interactive development):
arc shell
# → Drops you into your $SHELL with .arc/env fully loaded
# → ruby, gem, bundle all resolve to the project's isolated binaries
# → Type 'exit' to return to your normal environment
```

> **"arc never touches your PATH. What runs is always what you see."**

This is the same philosophy `uv` chose: `uv run python script.py` instead of relying on a shim-managed `python`.

---

## Global Binary Cache

arc uses a **global cache** at `~/.arc/cache/` — shared across all your projects.

```
~/.arc/cache/
  rubies/
    3.3.6-linux-x86_64/   ← Downloaded once, linked to every project
  gems/
    gems/                  ← Compiled gems shared via hardlinks
    specifications/
    extensions/            ← C extension binaries (never recompile)
```

### How it works

```bash
# Project A: first time
arc bootstrap    # Downloads Ruby 3.3.6 → ~30s
arc add nokogiri # Compiles C extension → ~20s

# Project B: same versions
arc bootstrap    # Hardlinks from cache → 0.07s ⚡
arc sync         # Restores nokogiri from cache → instant ⚡
```

This is identical to how `uv` achieves its legendary speed — hardlinks mean **zero copy overhead** and **zero disk duplication**.

---

## Flux Core: The Engine Behind arc

arc is built on **Flux Core** — a general-purpose event sourcing engine for CLI tools.

```
┌─────────────────────────────────────────────────────┐
│                      arc CLI                         │
│  ┌──────┐  ┌──────┐  ┌──────┐  ┌───────┐  ┌──────┐ │
│  │ init │  │ add  │  │ sync │  │ state │  │ undo │ │
│  └──┬───┘  └──┬───┘  └──┬───┘  └───┬───┘  └──┬───┘ │
│     │         │          │          │          │     │
├─────┴─────────┴──────────┴──────────┴──────────┴────┤
│                   Flux Core Engine                   │
│  ┌─────────────────┐  ┌──────────────────────────┐  │
│  │  FluxProject    │  │  Signal (Flux v1 NDJSON)  │  │
│  │  .init()        │  │  - v: 1                   │  │
│  │  .open()        │  │  - source: "arc"          │  │
│  │  .record()      │  │  - type / event           │  │
│  │  .read()        │  │  - ts: RFC 3339           │  │
│  └─────────────────┘  └──────────────────────────┘  │
├─────────────────────────────────────────────────────┤
│              .flux/signals.jsonl                     │
│           (Append-only NDJSON log)                   │
└─────────────────────────────────────────────────────┘
```

### What is a Signal?

Every operation arc performs emits one or more **Signals** — structured, immutable events written to an append-only log. A signal is one line of [Flux Protocol v1](https://git.navii.online/takaki2/flux-protocol) NDJSON.

```json
{"v":1,"source":"arc","type":"gem","event":"added","ts":"2026-08-22T07:30:15.177395+00:00","id":"019c70b2-5c33-7d03-9911-112233445566","gem":"rails","version":"~> 8.0"}
{"v":1,"source":"arc","type":"cmd","event":"run","ts":"2026-08-22T07:30:15.186032+00:00","id":"019c70b2-3a11-7f01-b178-bd89a26ed073","cmd":"bundle install","args":["install"],"cwd":"/proj","verb":"sync","env_mode":"isolated"}
{"v":1,"source":"arc","type":"cmd","event":"end","ts":"2026-08-22T07:30:19.386032+00:00","id":"019c70b2-3a11-7f01-b178-bd89a26ed073","cmd":"bundle install","exit_code":0,"ms":4200,"success":true,"verb":"sync","env_mode":"isolated"}
```

Key properties:
- **Four required fields** — `v`, `source`, `type`, `event`. Everything else is payload, flat on the same line.
- **`type` is a noun, `event` is what happened to it** — `gem:added`, `cmd:run`, `session:end`. Lowercase kebab-case; the pair is how the Signal Registry is keyed.
- **UUID v7 `id`** — time-sortable, globally unique.
- **Correlated by a shared `id`** — `cmd:run` and `cmd:end` carry the **same** `id`, so a pair is a group-by rather than a pointer chase. This is the same correlation model `hsh` uses.
- **`ts` is when it happened**, RFC 3339 with an explicit offset, on the producer's clock ([`PROTOCOL_v1.md` §2.05](https://git.navii.online/takaki2/flux-protocol)).
- **Append-only** — signals are never modified or deleted.

> **Reading older logs.** Journals written before 2026-08-22 use arc's pre-protocol
> shape (`{id, type, payload, timestamp}`, with names like `add_start`). arc reads
> those transparently — the old `type` is translated to a `type:event` pair and the
> time is taken as `ts ?? modified ?? timestamp`, the fallback order the protocol
> specifies for exactly this migration. **Renaming does not fix history**, so this
> is not a temporary measure.

### Why Event Sourcing?

Traditional tools mutate state and discard history. Flux Core treats every operation as a **fact** that happened at a specific time.

This enables:

| Capability | How |
|---|---|
| `arc undo` | Walk the signal log backwards, find the last `gem:added`/`gem:removed`, reverse it |
| `arc state --diff` | Compare Gemfile snapshots stored in consecutive signals |
| `arc state` statistics | Aggregate `cmd:end` signals by command line, count success/failure |
| Future: replay | Re-emit signals on a new machine to reproduce an environment |
| Future: AI audit | Feed signal log to an LLM to explain what happened and why |

### Flux Core as a Standalone Crate

Flux Core is designed to be extracted as a standalone `flux-core` crate — usable by **any Rust CLI tool**, not just arc.

```rust
// Any CLI tool can use Flux Core:
let project = FluxProject::open(&cwd)?;
project.record(SignalType::CmdRun, json!({ "cmd": "deploy", "env": "production" }))?;
```

---

## How arc Compares

### vs. rvm / rbenv

| | rvm | rbenv | arc |
|---|:---:|:---:|:---:|
| Ruby version management | ✅ | ✅ | ✅ (per-project) |
| Gem management | ❌ | ❌ | ✅ |
| Global binary cache | ❌ | ❌ | ✅ |
| Gem binary cache | ❌ | ❌ | ✅ |
| Operation history | ❌ | ❌ | ✅ |
| Undo | ❌ | ❌ | ✅ |
| PATH pollution | ✅ (heavy) | ✅ (shims) | ❌ (none) |
| Written in | Bash | Bash | Rust |

### vs. rv (uv-inspired Ruby manager)

| | rv | arc |
|---|:---:|:---:|
| Ruby version management | ✅ multi-version | ✅ per-project |
| Gem management | ❌ | ✅ |
| Gem binary cache | ❌ | ✅ |
| Operation history | ❌ | ✅ |
| Undo | ❌ | ✅ |
| Shims | ✅ | ❌ (by design) |
| Philosophy | "rbenv, but fast" | "uv, but for Ruby" |

### vs. uv (Python)

| | uv | arc |
|---|:---:|:---:|
| Global binary cache | ✅ | ✅ |
| Hardlink sharing | ✅ | ✅ |
| Isolated environments | ✅ | ✅ |
| `add` / `remove` / `sync` | ✅ | ✅ |
| Operation history | ❌ | ✅ |
| Undo | ❌ | ✅ |
| Multi-version management | ✅ | ⚠️ (roadmap) |

> **arc is `uv` for Ruby, with an event sourcing engine that `uv` doesn't have.**

---

## Project Structure

```
my_project/
├── Gemfile
├── Gemfile.lock
├── .flux/
│   ├── signals.jsonl    ← Append-only operation log (Flux Core)
│   └── config.toml      ← arc configuration
└── .arc/
    └── env/
        ├── ruby_runtime/ ← Linked from ~/.arc/cache/rubies/
        ├── bin/          ← Gem executables
        ├── gems/         ← Installed gems
        └── ...
```

```toml
# .flux/config.toml
[ruby]
version = "3.3.6"
```

Change Ruby version anytime:
```bash
arc bootstrap 3.4.0   # Updates config.toml and re-links Ruby
```

---

## Philosophy

> **"Tools should remember."**

We live in an era where AI agents execute dozens of commands in seconds. Shell history isn't enough. Git diffs don't capture the *process*.

arc fills that gap — it gives every terminal operation **identity**, **structure**, and **permanence**.

Three principles guide arc's design:

1. **Explicit over implicit** — `arc run ruby` instead of shims. You always know what runs.
2. **Record everything** — every operation is a Signal. Nothing is lost.
3. **Project-complete** — everything lives in `.arc/env/` and `.flux/`. Clone a repo, run `arc bootstrap && arc sync`, and you're done.

---

## Tech Stack

- **Language**: Rust (2024 edition)
- **CLI Framework**: clap v4
- **Serialization**: serde + serde_json + toml
- **IDs**: UUID v7 (time-sortable, monotonic)
- **Log Format**: [Flux Protocol v1](https://git.navii.online/takaki2/flux-protocol) NDJSON
- **Ruby Source**: [ruby/ruby-builder](https://github.com/ruby/ruby-builder) pre-compiled binaries

---

## Roadmap

| Phase | Status | Description |
|---|---|---|
| **1. Core Engine** | ✅ Done | Flux Core, Signal recording, FluxProject API |
| **2. Ruby Bootstrap** | ✅ Done | Global cache, hardlink sharing, `arc bootstrap` |
| **3. Gem Management** | ✅ Done | `add`, `remove`, `sync`, gem binary cache |
| **4. Undo & Diff** | ✅ Done | `arc undo`, `arc state --diff` |
| **5. Config** | ✅ Done | `.flux/config.toml`, dynamic Ruby version |
| **6. arc shell** | ✅ Done | Interactive sub-shell with isolated env, Flux signal recording |
| **7. Multi-version** | 📋 Planned | `arc use 3.4.0` alongside existing versions |
| **8. Time Machine** | 🚀 Planned | `arc checkout <id>` / `arc reset` / Environment Replay |
| **9. Windows Support** | ✅ (2026-10-11) | `bootstrap` fetches RubyInstaller's portable `.7z`; `sync` / `run` / `undo` work. `arc bootstrap --devkit` adds MSYS2 + the MINGW toolchain (~1GB) inside the cached Ruby, for gems that compile a C extension (Pharos: puma, io-event, bigdecimal … on Ruby 4.0.7) |
| **10. flux-core crate** | 📦 Planned | Extract Flux Core as a standalone crate |
| **11. macOS support** | 📋 Planned | ARM64 binary support |

---

## ❤️ Support the Project

Arc aims to end the *"it works on my machine"* era by making environments reproducible and operations reversible.

If you resonate with the **Flux Philosophy** (tools that remember), consider supporting the development.
Funds will go towards **Windows hardware for porting** and server costs.

[**GitHub Sponsors**](#) | [**Buy Me a Coffee**](#)

---

## License

MIT

---

<div align="center">

*Built on [Flux Core](docs/FLUX_CORE.md) — Event Sourcing for the Terminal.*

**arc** — Every operation has meaning. Every state tells a story.

</div>
