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
- [x] OSC 7 working directory in the pane header
- [x] Pane titles from OSC 0/2, with the profile name as a fallback
- [x] Shell integration: generated hooks (PowerShell via `-EncodedCommand`,
      bash via `--rcfile`) that make OSC 7 and OSC 133 fire
- [x] Exit status of the last command, shown in the pane header
- [x] Command duration from the `C` mark, for shells that emit one
      (bash does, through `PS0`)
- [x] Jump to the previous or next prompt in the scrollback
- [ ] PowerShell command duration: needs a PSReadLine Enter handler, which
      replaces a key people rely on. Not shipped while it cannot be verified.
- [ ] Profile editor in the UI (the model and persistence are in place)

## P2 — agents

- [x] Agent catalogue as data, with per-agent launch rules
- [x] Open an installed agent in a pane from the palette, task typed there
- [x] Read the child's own `RESULT` line and show it as a claim
- [x] Worktrees: create on `agent/<8>`, reuse, remove without deleting the branch
- [x] Caps: live agent panes and spawn rate, enforced behind one command
- [x] Activity log: append-only JSON lines, refusals included
- [x] Run phase from observation: starting, working, quiet, reported, exited,
      failed. `quiet` is deliberately not `idle` and never `blocked`, because a
      thinking agent and a waiting one are identical from outside a terminal.
- [ ] Token meter and a budget guard (blocks above 85% of the 5-hour window).
      Needs each agent CLI's own usage readout, so it is per-agent work.
- [ ] Spawn approval card. Only meaningful once a *child* can ask for a pane;
      today the person clicking the palette is already the approver.
- [ ] Remove the superseded `agent_launch` and `agent_worktree` commands, now
      that `agent_open` does both behind the guard.
- [ ] Worktree housekeeping: offer to remove a finished run's worktree, and
      tolerate a directory that is still held instead of failing silently.
      One leftover empty directory remains from the verification run: a handle
      was still open on it after the app closed, and no process could be
      identified as the holder without guessing among unrelated ones.
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
