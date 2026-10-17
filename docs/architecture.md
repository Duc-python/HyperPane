# Architecture

HyperPane is a local-first terminal workspace. Every feature is designed to
work with no server, no account and no network access.

## Processes

```
┌──────────────────────────────────────────────────────────┐
│ hyperpane (Tauri)                                        │
│                                                          │
│  ┌────────────────────┐        ┌──────────────────────┐  │
│  │ webview (React)    │◄──IPC──►│ Rust core            │  │
│  │  xterm.js per pane │        │  hp-pty / hp-core    │  │
│  └────────────────────┘        └──────────┬───────────┘  │
│                                           │              │
└───────────────────────────────────────────┼──────────────┘
                                            │ ConPTY / openpty
                              ┌─────────────▼─────────────┐
                              │ shell · agent CLI · ssh   │
                              └───────────────────────────┘
```

MCP servers (P4) and the mesh daemon (P6) are separate processes that the app
supervises, matching how the plugin hub starts its children on demand.

## Crates

| Crate | Responsibility |
|---|---|
| `hp-pty` | PTY sessions, pane registry, incremental UTF-8 decoding, event streaming |
| `hp-core` | Domain: pane ids, split layout tree, workspaces |
| `hp-agents` | Agent catalogue and launch adapters |
| `hp-teams` (P3) | Team presets → runs: waves, gates, roles, stop conditions |
| `hp-mesh` (P6) | Device identity, pairing, QUIC transport, grants |
| `hp-mcp` (P4) | MCP servers: hub, team, router |

## Data flow for one pane

1. The UI generates a pane id and calls `pane_spawn` with a `PaneSpec` and a
   Tauri `Channel`.
2. `hp_pty::PaneManager` opens the PTY, spawns the child, and starts a reader
   thread and a waiter thread.
3. The reader decodes bytes incrementally — a UTF-8 sequence split across two
   reads is held, not replaced — and pushes `PtyEvent::Data` into the channel.
4. The waiter pushes `PtyEvent::Exit` when the child ends, which is what lets
   the UI show `exited` rather than guessing from silence.
5. Keystrokes go back with `pane_write`; geometry changes go with
   `pane_resize`.

## Layout model

The layout is a binary tree of leaves and splits, serialised as JSON. Geometry
is a ratio per split, so a saved layout reflows to any window size. Ratios are
clamped to `[0.1, 0.9]` on both sides of the IPC boundary, so no drag can
produce an unreachable pane.

A divider addresses its own split by **path** — the sequence of
first/second choices from the root — rather than by an id, which keeps the tree
free of bookkeeping state that could drift from the UI.

## Conventions

- The frontend owns **identity** (it generates pane ids); the backend owns
  **state**. Layout-changing commands return the whole workspace so the UI never
  reconstructs the tree itself.
- Focus changes are optimistic because a click must feel instant; layout
  changes are authoritative because they must be correct.
- Every drag is local and commits once on release. IPC per pointer move would
  make resizing feel like a distributed system.

## Non-goals for P0

Persistence, sessions, profiles, shell integration, agent panes, teams, rooms
and mesh are deliberately out of scope. The skeleton exists to prove the PTY
path end to end on all three platforms.
