# HyperPane

A local-first terminal workspace with multi-pane AI agent orchestration.

HyperPane is a desktop terminal where each pane can host a shell, a remote
session, or a coding agent — and where a lead agent can open, brief, watch and
collect results from other agents in visible panes beside it.

Everything runs on your own machine. There is no account, no subscription and
no server: coordination, sharing and multi-computer access are peer-to-peer.

## Status

**P0 — skeleton.** Working: PTY layer, pane registry, split layout tree,
terminal rendering, layout persistence hooks. Not yet built: agents, teams,
rooms, mesh. See the roadmap below.

## Stack

| Layer | Choice |
|---|---|
| Shell | Tauri v2 (Rust) |
| PTY | `portable-pty` (ConPTY on Windows, `openpty` on Unix) |
| Frontend | React 19 + TypeScript + Vite |
| Styling | Tailwind v4 + OKLCH design tokens |
| Terminal | `@xterm/xterm` + WebGL renderer |
| IPC | Tauri commands + `Channel` streaming, types generated from Rust |
| Mesh (P6) | QUIC (`quinn`) + `russh` + ed25519 device identity |

## Layout

```
hyperpane/
├─ apps/desktop/          Tauri app (React UI + Rust core)
│  ├─ src/                React 19 UI
│  └─ src-tauri/          Tauri commands, state, window
├─ crates/
│  ├─ hp-pty/             PTY sessions, pane registry, UTF-8 stream decoding
│  ├─ hp-core/            Domain: pane ids, split layout tree, workspaces
│  └─ hp-agents/          Agent catalogue + delivery adapters
├─ presets/               Team preset definitions + JSON schema
├─ docs/                  Architecture and design notes
└─ .github/workflows/     CI (fmt, clippy, typecheck, build)
```

## Requirements

- **Node 20+** and **pnpm 9+**
- **Rust stable** (`rustup`) — Tauri backend
- **Windows**: MSVC build tools (Visual Studio 2022 Build Tools, C++ workload) + WebView2 runtime
- **macOS**: Xcode command line tools
- **Linux**: `webkit2gtk-4.1`, `libayatana-appindicator3`, `librsvg2`, `libssl`, `build-essential`

## Develop

```bash
pnpm install
pnpm dev            # frontend only (Vite)
pnpm tauri dev      # full desktop app
```

## Roadmap

| Phase | Scope |
|---|---|
| **P0** | Tauri skeleton, design system, PTY layer, terminal pane, tabs, splits |
| **P1** | Workspaces, profiles, shell integration (OSC 133), keybindings, command palette |
| **P2** | Agent catalogue, `pane.spawn/send/list/read/close`, worktrees, status detection, token meter, budget guard, audit log |
| **P3** | Team engine (waves, gates, roles, stop conditions), router, team run view |
| **P4** | Plugin hub (MCP) + plugin SDK |
| **P5** | Control Rooms — host-authoritative over QUIC, LAN first |
| **P6** | Mesh: pairing, device identity, standing/temporary grants, My Computers |

## Design principles

1. **Local-first.** No server is required for any feature. Nothing leaves the
   machine unless the user opens it.
2. **The human holds the wheel.** Agents never get a bypass flag, never close
   panes, never merge or deploy. Every spawn is confirmed by a person.
3. **Bounded work.** Every automated run has caps: live children, spawn rate,
   wall clock and budget.
4. **Everything is recorded.** If the audit log cannot be written, the action
   does not happen.

## License

MIT
