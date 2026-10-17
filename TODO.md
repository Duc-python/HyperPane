# HyperPane TODO

Checklist for the build. One phase in progress at a time; each phase ends
green (tests, clippy, typecheck, and the app actually running).

## P0 — skeleton ✅ done

- [x] Rust workspace, pnpm workspace, CI on three platforms
- [x] `hp-pty`: PTY sessions, incremental UTF-8 decoding, reader + waiter threads
- [x] `hp-core`: split layout tree, focus, ratio clamping
- [x] `hp-agents`: agent catalogue and launch adapters
- [x] Tauri shell: workspace + pane commands
- [x] React UI: recursive layout, draggable dividers, xterm with WebGL
- [x] Verified: 21 unit tests, clippy clean, app runs a real ConPTY session

## P1 — workspace, profiles, palette ← in progress

- [x] Session persistence: layout + profiles survive a restart
- [x] Profile model: shell, args, cwd, env
- [x] Keybinding registry: one table, no hard-coded chords
- [x] Command palette (`mod+shift+p`) with fuzzy filtering
- [ ] Shell integration: OSC 133 prompt marks, exit code, command duration
- [ ] OSC 7 working directory in the pane header
- [ ] Pane titles from OSC 0/2, with the process name as a fallback
- [ ] Profile editor in the UI (the model and persistence are in place)

## P2 — agents

- [ ] `pane.spawn` / `send` / `list` / `read` / `close` from the UI
- [ ] Worktree per agent, branch `agent/<8>`
- [ ] Status detection: working / blocked / done / exited
- [ ] `RESULT` line parsing
- [ ] Token meter + budget guard (blocks above 85% of the 5-hour window)
- [ ] Spawn approval card and audit log
- [ ] Caps: live children, spawn rate, wall clock

## P3 — teams

- [ ] Preset loader with schema validation
- [ ] Run engine: waves, gates, roles, stop conditions
- [ ] Router: pick an agent for a task
- [ ] Team run view: wave timeline, live status, gates as checkpoints

## P4 — plugins

- [ ] MCP server host, `plugins.call`
- [ ] Plugin SDK + signed manifest

## P5 — control rooms

- [ ] Host-authoritative room over QUIC, LAN first
- [ ] Share pane / tab / computer, per-pane ACL
- [ ] Presence and follow mode

## P6 — mesh

- [ ] Device identity (ed25519), pairing by code/QR, TOFU pinning
- [ ] Standing access password, temporary grants (≤ 7 days)
- [ ] My computers: open a pane on another machine
- [ ] Self-hosted relay
