<p align="center"><img src="assets/logo.png" alt="aitrash-cleaner" width="640"></p>

<h1 align="center">aitrash-cleaner</h1>

<p align="center"><b>English</b> · <a href="README.pt-BR.md">Português (Brasil)</a></p>

**Free up disk space used by AI coding agents.** `aitrash` finds and cleans the junk that Claude Code, OpenAI Codex CLI, Gemini CLI/Antigravity, Cursor, Windsurf and Kiro leave on your machine: old session transcripts in `~/.claude/projects` and `~/.codex/sessions`, editor caches, leftover debug/test scripts in your projects, orphaned dev servers, stale `claude/*` git worktrees, and API keys sitting in plain text inside transcripts.

Single binary (~2 MB), no runtime, for Linux, macOS and Windows. On WSL it also scans the Windows profile under `/mnt/c/Users`. Everything goes to the OS trash by default, so nothing is lost by accident.

## Install

**Linux / macOS / WSL:**
```bash
curl -LsSf https://github.com/EduardoRocha234/aitrash-cleaner/releases/latest/download/aitrash-installer.sh | sh
```

**Windows (PowerShell):**
```powershell
irm https://github.com/EduardoRocha234/aitrash-cleaner/releases/latest/download/aitrash-installer.ps1 | iex
```

The installer detects your OS and architecture, downloads the right binary into `~/.local/bin` and adds that folder to your PATH. Open a new terminal and run `aitrash`.

**Via npm (if you already have Node):**
```bash
npx aitrash            # run without installing
npm i -g aitrash       # install the aitrash command
```

**Manual:** download the archive for your system from [Releases](https://github.com/EduardoRocha234/aitrash-cleaner/releases/latest), extract it and put `aitrash` somewhere on your PATH.

**From source** (requires [Rust](https://rustup.rs)):
```bash
cargo install --git https://github.com/EduardoRocha234/aitrash-cleaner
```

### Update and uninstall

To update, run the install command again.

```bash
rm ~/.local/bin/aitrash        # one-line installer (Windows: del $HOME\.local\bin\aitrash.exe)
npm rm -g aitrash              # npm
cargo uninstall aitrash        # from source
```

## Usage

```
aitrash                    # summary: one group per line (changes nothing)
aitrash -v                 # every item in detail
aitrash scan --home        # scan every project under your home
aitrash clean              # pick groups from a menu; moves them to the trash
aitrash clean -n           # dry run
aitrash secrets --all      # plain-text keys/tokens in agent transcripts
aitrash secrets --clean    # choose transcripts containing secrets to delete
aitrash du                 # how much space each agent and cache uses
aitrash paths              # known agent paths on this system
```

Useful flags: `--only global,project,system,secrets`, `--older-than 30d`, `--json`, `--verbose`, `--no-wsl`, `--hard` (delete permanently, skip the trash).

## What it finds

| Category | Examples |
|---|---|
| global | old transcripts/sessions (`~/.claude/projects`, `~/.codex/sessions`, `~/.gemini/tmp`), Cursor/Windsurf/Antigravity caches, credentials (`auth.json`) |
| project | `*.bak`, `*.orig`, `test_fix*.py`, `debug_*.js`, `check_*.sh`, `scratch/`, summary `.md` files, `CLAUDE.md`/`AGENTS.md`/`.claude/` |
| system | caches (Playwright, npx, npm, pip, uv, pnpm), orphaned dev servers, `claude/*` worktrees/stashes/branches, stopped Docker containers, agent lines in `.bashrc`/`.zshrc` |
| secrets | `sk-ant-`, `sk-`, `ghp_`, `AKIA…`, `xox?-`, `AIza…`, private keys, `*_TOKEN=`/`*_SECRET=` (always masked) |

## Safety

- `scan` never changes anything. `clean` without an interactive terminal becomes a dry run unless you pass `--yes`.
- Removed items go to the OS trash. Use `--hard` to delete permanently.
- Only **high**-confidence items are pre-selected. Files tracked by git are never pre-selected, and an untracked project file gets one confidence level higher.
- Protected items `[!]` (credentials, `.claude.json`, rc files, tracked files) only show up with `--hard`, and each one asks for confirmation.
- Never follows symlinks and never removes anything outside your home or the paths you pass.

## FAQ

### How do I free up disk space used by Claude Code?

Run `aitrash du` to see how much `~/.claude` takes, then `aitrash clean`. Session transcripts in `~/.claude/projects` (`.jsonl` files) are the biggest part, along with `file-history`, `paste-cache`, `debug`, `shell-snapshots`, `session-env` and `todos`. Old plan files in `~/.claude/plans` are offered too, but never pre-selected. By default only files older than 30 days are flagged; change that with `--older-than 7d`.

### Is it safe to delete `~/.claude/projects`?

Those files are the history of past conversations. Deleting old ones only means you can no longer `--resume` those sessions; settings, credentials and `CLAUDE.md` are untouched. `aitrash` only flags transcripts older than `--older-than`, sends them to the trash and never touches `.credentials.json` or `.claude.json` unless you use `--hard` and confirm each one.

### Why is `~/.codex` (OpenAI Codex CLI) so large?

Codex stores every session in `~/.codex/sessions` and `~/.codex/archived_sessions`, plus logs in `~/.codex/log`. They are never pruned automatically. `aitrash clean --only global` lists and removes the old ones.

### How do I clear the Cursor or Windsurf cache?

Cursor, Windsurf, Antigravity and Kiro are VS Code forks, so they pile up `Cache`, `CachedData`, `Code Cache`, `GPUCache`, `logs`, `CachedExtensionVSIXs` and crash dumps in their app folder (`~/.config/Cursor` on Linux, `~/Library/Application Support/Cursor` on macOS, `%APPDATA%\Cursor` on Windows). These are regenerated on demand. `aitrash du` shows their size and `aitrash clean` removes only those cache folders, never your settings, extensions or MCP config.

### What about Gemini CLI and Antigravity?

`aitrash` scans `~/.gemini/tmp` and Antigravity's `~/.gemini/antigravity/brain` and `browser_recordings`. OAuth credentials (`oauth_creds.json`, `google_accounts.json`) are protected.

### My agent left `test_fix.py`, `debug_*.js` and summary `.md` files all over my project. Can it clean those?

Yes. `aitrash scan` (current project) or `aitrash scan --home` (every project) finds scratch scripts, backups (`*.bak`, `*.orig`), `scratch/` folders and AI-written summary markdown. Files tracked by git are never pre-selected.

### Do my AI agent transcripts contain API keys?

Often, yes: anything you pasted or that a tool printed ends up in the transcript in plain text. `aitrash secrets --all` finds Anthropic, OpenAI, GitHub, AWS, Slack and Google keys, private keys and `*_TOKEN=` lines, always masked in the output. `aitrash secrets --clean` lets you delete the affected transcripts. Rotate any key it finds.

### Does it work on Windows and WSL?

Yes. There is a native Windows binary, and on WSL it also scans the Windows user profile under `/mnt/c/Users` (disable with `--no-wsl`).

### How is this different from `rm -rf ~/.claude` or `ncdu`?

| | `aitrash` | `rm -rf` | `ncdu` / disk analyzers |
|---|---|---|---|
| Knows which agent files are safe to remove | yes | no | no |
| Keeps credentials and settings | yes | no | manual |
| Age filter for sessions | yes | no | no |
| Finds leftover files inside projects | yes | no | no |
| Finds leaked secrets in transcripts | yes | no | no |
| Undo (OS trash) | yes | no | no |

## Build

```
cargo build --release                                     # ./target/release/aitrash
cargo build --release --target x86_64-unknown-linux-musl  # fully static binary
```

## Releasing

`.github/workflows/release.yml` is generated by [cargo-dist](https://github.com/axodotdev/cargo-dist) (`dist generate`; do not edit by hand). To release:

1. Bump `version` in `Cargo.toml`.
2. Run `git commit -am "release: v0.2.0" && git tag v0.2.0 && git push --follow-tags`.

CI builds for Linux (x86_64/arm64, static), macOS (Intel/Apple Silicon) and Windows, then publishes binaries, installers and checksums to GitHub Releases and the `aitrash` package to npm. npm uses trusted publishing (OIDC), no token: the `aitrash` package trusts this repository's `release.yml` workflow.

## License

MIT
