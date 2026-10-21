# HyperPane

A local-first terminal workspace where each pane can be a shell, a coding agent,
or another machine — and where agents can work as a team without a server.

HyperPane runs on your own machine. There is no account, no subscription, no
telemetry and no server: panes, agent runs and control rooms are coordination
between processes you started, on machines you can reach.

[Tiếng Việt](README.vi.md)

![HyperPane with a shell pane and an agent pane side by side](docs/screenshot.png)

## What works today

- **Panes and layout.** Tabs, splits in either direction, draggable dividers,
  focus by keyboard or click, per-pane shell profiles, and a session that
  survives a restart. Layout is a ratio tree, so it reflows to any window size.
- **Shell integration.** OSC 7 and OSC 133 give the pane header its working
  directory, the exit status and duration of the last command, and let a
  keybinding jump between prompts in the scrollback.
- **Agents in panes.** HyperPane starts the CLIs you already have — Claude Code,
  Codex, opencode, Gemini CLI, Qwen Code, Cursor Agent — and gives each one a
  pane, a task, and a worktree when the task writes. The child's own `RESULT`
  line is read back as a **claim**, shown beside the pane, and kept after the
  pane closes. Every spawn is capped and recorded in an append-only audit log.
- **Teams.** Presets — *Independent review*, *Build a feature*, *Debate a
  decision* — run agents in waves behind gates. Each member gets a brief with
  its role, its scope and where to write findings; a later wave is handed the
  earlier members' claims and findings, so a reviewer answers a finding instead
  of a description of one. A checker wave holds the run until a person says
  whether it passed.
- **Worktrees.** A writing agent runs on its own `agent/<id>` branch in a
  worktree; a clean entry can be removed in one click, a dirty one needs a
  second confirmation, and a worktree a live pane is using cannot be removed at
  all. Branches are never deleted.
- **MCP plugins.** A client for both protocol eras; a plugin is a program you
  already have. Consent is explicit and re-asked when a server's tools change.
- **Control rooms.** Host-authoritative rules over TCP: participants, which
  panes they may see, what they may do. **Loopback only and unauthenticated**;
  the panel says so.
- **Pairing groundwork.** An ed25519 identity whose name is its public key, a
  pairing code exchanged out of band, and a `peers.json` of pinned machines.
  Nothing consumes it yet — the room still admits anyone who can connect.
- **Works on itself.** `Work on HyperPane…` opens the app's own source in a
  pane; `Update from source…` runs `git pull --ff-only`, `pnpm install` and a
  build in a visible pane, stopping if a step fails. On Windows the final link
  cannot finish while the app is running, and the pane names the command that
  finishes it after quitting.
- **Reload without losing work.** Reloading the UI reattaches to live PTYs and
  replays their recent output; it does not restart shells or agents.

## Not built yet

- QUIC rooms and admission by pinned identity (the room is TCP and loopback).
- Token meter, budget guard, spawn approval card.
- Standing access password, temporary grants, *My computers*, self-hosted relay.
- Signed self-update. It needs a release channel and a signing key kept forever;
  until there is one, updating means pulling and building the source.
- A declared `pastedAfterStart` agent delivery has no side that types.

`TODO.md` is the authoritative list, including the gaps above and why they exist.

## Stack

| Layer | Choice |
|---|---|
| Shell | Tauri v2 (Rust) |
| PTY | `portable-pty` (ConPTY on Windows, `openpty` on Unix) |
| Frontend | React 19 + TypeScript + Vite |
| Styling | Tailwind v4 + OKLCH design tokens |
| Terminal | `@xterm/xterm` + WebGL renderer |
| IPC | Tauri commands + `Channel` streaming, types generated from Rust |
| Teams | Data-only presets (`presets/teams.json`) + a wave router |
| Rooms | TCP, newline-delimited JSON, host-authoritative (QUIC planned) |
| Identity | ed25519 key pair; the device name is the public key |

## Layout

```
hyperpane/
├─ apps/desktop/       Tauri app: React UI + Rust commands and state
├─ crates/
│  ├─ hp-pty/          PTY sessions, pane registry, incremental UTF-8 decoding
│  ├─ hp-core/         Pane ids, split layout tree, workspaces, profiles
│  ├─ hp-agents/       Agent catalogue, launch adapters, claim reader
│  ├─ hp-teams/        Team presets, plans, briefs, waves, gates, router
│  ├─ hp-shell/        Generated shell integration (OSC 7 / OSC 133)
│  ├─ hp-store/        Session persistence, written atomically
│  ├─ hp-mcp/          MCP client over stdio (both protocol eras)
│  ├─ hp-room/         Room rules, host loop and client
│  └─ hp-identity/     Device key pair, pairing exchange, pinned peers
├─ presets/            Team presets + JSON schema
├─ scripts/            Check gate, MSVC wrapper, window capture, icons
├─ docs/               Architecture notes
└─ .github/workflows/  CI: fmt, clippy, tests, typecheck, build
```

## Requirements

- **Node 22+** and **pnpm 11** (`corepack enable pnpm`)
- **Rust stable** with `rustfmt` and `clippy` (`rustup` reads `rust-toolchain.toml`)
- **Windows**: Visual Studio Build Tools with the C++ workload **and the Windows
  SDK**. The C++ tools alone are not enough: linking fails with
  `LNK1181: cannot open input file 'kernel32.lib'`. WebView2 is present on
  current Windows; older installs need the runtime.
- **macOS**: Xcode command line tools
- **Linux**: `webkit2gtk-4.1`, `libayatana-appindicator3`, `librsvg2`, `libssl`,
  `libxdo`, `patchelf`, `build-essential`

On Windows, Rust's MSVC target needs `link.exe` on `PATH`, which an ordinary
shell does not have. `scripts/with-msvc.ps1` finds the Visual Studio
installation, imports the environment `vcvars64.bat` sets, and runs a command
inside it. The `--` keeps cargo's own flags (`-p`) from being read as
PowerShell parameters:

```powershell
.\scripts\with-msvc.ps1 -- cargo test --workspace
```

## Develop

```bash
pnpm install
pnpm dev                 # frontend only (Vite) on :5173
pnpm tauri dev           # the full desktop app
```

```powershell
# One crate, or one test by name
.\scripts\with-msvc.ps1 -- cargo test -p hp-teams
.\scripts\with-msvc.ps1 -- cargo test -p hp-teams a_three_case_debate

# One frontend file
pnpm --filter @hyperpane/desktop exec vitest run src/lib/panes.test.ts

# Everything the repository promises: fmt, clippy -D warnings, cargo test,
# typecheck, vitest, frontend build. Fails on the first step that does not pass.
.\scripts\check.ps1
```

Two things to know before the first build:

- **`apps/desktop/dist` must exist before any cargo command that compiles the
  app crate.** `tauri-build` reads `frontendDist` at compile time and fails when
  the directory is absent. CI builds the frontend first for this reason.
- **A debug build loads `devUrl`** (`http://localhost:5173`). Run Vite before
  `target\debug\hyperpane.exe`, or use `pnpm tauri dev`. Vite may bind `::1`
  only, so a `127.0.0.1` reachability check can give a false negative.
- On Windows the running executable is locked: stop the app before rebuilding it.

## Build a release

```powershell
pnpm install
pnpm --filter @hyperpane/desktop build
.\scripts\with-msvc.ps1 -- cargo build --release -p hyperpane --features custom-protocol
```

The `custom-protocol` feature is what embeds the built frontend. Without it a
release binary keeps the development `devUrl` and opens `localhost:5173`, which
is not running, so the window shows a connection error. `pnpm tauri build` turns
the feature on itself and also produces the platform installer; a plain
`cargo build` needs it spelled out.

The result is `target\release\hyperpane.exe` (or the equivalent binary on macOS
and Linux). It starts without Vite.

## Data and privacy

Everything the app keeps lives in one data directory — on Windows
`%APPDATA%\app.hyperpane.desktop`; set `HYPERPANE_DATA_DIR` to move it. It holds
`session.json` (layout, profiles, which pane runs which agent), `audit.log`
(append-only JSON lines), `findings/` from team runs, `plugins.json`,
`trust.json`, `peers.json` and the device identity.

Nothing is sent anywhere. A control room listens on loopback until a person
starts one; pairing exchanges ids out of band, never through a server.

## Roadmap

| Phase | Scope | State |
|---|---|---|
| **P0** | Tauri skeleton, design system, PTY layer, terminal pane, tabs, splits | done |
| **P1** | Workspaces, profiles, shell integration, keybindings, palette | done, one shell-duration gap |
| **P2** | Agent catalogue, claims, worktrees, caps, audit, run phases | done; token meter and budget guard not built |
| **P3** | Team engine: waves, gates, roles, briefs, findings, debate | done |
| **P4** | Plugin hub (MCP) and consent | done; signatures are not invented |
| **P5** | Control rooms | rules + TCP on loopback; QUIC not built |
| **P6** | Mesh: identity, pairing, grants, *My computers* | identity and pairing groundwork only |
| **P7** | The app works on itself | source pane and update-from-source done; signed update not built |

## Design principles

1. **Local-first.** No server is required for any feature. Nothing leaves the
   machine unless a person opens it.
2. **The human holds the wheel.** Agents never get a bypass flag, never close
   panes, never merge or deploy. Every spawn is confirmed by a person, and a
   checker wave waits for one.
3. **A claim is not a fact.** What an agent prints is shown as what it said, with
   the file it wrote beside it. The app does not upgrade a report into a result.
4. **Bounded work.** Every automated run has caps: live children, spawn rate,
   wall clock and, eventually, budget.
5. **Everything is recorded.** If the audit log cannot be written, the action
   does not happen.

## License

MIT
