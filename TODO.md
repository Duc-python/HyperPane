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
- [x] A pane's shell and scrollback survive a layout change. A session lives
      outside the view that draws it, so splitting a pane no longer restarts the
      pane beside it. Do not put a PTY back inside a component: the element tree
      mirrors the layout tree, so a split unmounts and remounts panes.
- [ ] PowerShell command duration: needs a PSReadLine Enter handler, which
      replaces a key people rely on. Not shipped while it cannot be verified.
- [x] Profile editor in the UI: `mod+,`, the `Profiles…` palette entry, or the
      profile name in the title bar

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

- [x] Preset loader with schema validation and problems that serde cannot see
- [x] Plan a run: waves, roles, agent preference, refusal with the names tried
- [x] Preview a plan before anything runs
- [x] Briefs: five parts, ending in the line the app reads back
- [x] Start the first wave, each member through the same reservation as a
      palette run
- [x] Advance to the next wave when a wave finishes, decided in the app and
      asked for by the frontend
- [x] Gates: `humanPlan` is the Start button, `checkerPass` and `humanCommit`
      put a hold on screen with a button
- [x] Router: a role's list is a preference order, and reading roles are spread
      over the installed agents rather than all landing on the first installed
      one — two readings by one agent share that agent's blind spots. Each
      choice carries the reason it was made, and the plan preview shows it. A
      role that writes keeps the preset's order.
- [x] Team run view: a wave timeline with live status, rather than one line in
      the header and a bar when a hold is up
- [x] Show what each member claimed when a wave finishes, and keep it after the
      pane closes. A member that printed no `RESULT` line reads "no report".
- [x] The findings themselves, not just the claim: the brief tells every member
      where to write what it found, the app reads that file back when the wave
      finishes and shows it on the hold, and an empty file is a valid answer.

## P4 — plugins

- [x] `hp-mcp`: the MCP client. Both protocol eras — modern per-request `_meta`
      with `server/discover` as the probe, and the `initialize` handshake for
      `2025-11-25` and earlier — with tools listed and called, pagination
      followed, and refusals, junk on stdout, silence, wrong ids, oversized
      messages and a server that will not exit all reported for what they are.
      Tested against a scripted server and against a real one.
- [x] The app side: `plugins.json` for which servers to run, `plugins_list`,
      `plugins_open`, `plugins_reload` and `plugins_call`, and one audit line per
      open and per call. A plugin is started on first use, kept afterwards, and a
      failure is remembered as a reason rather than retried as a wait. The panel
      takes JSON arguments and shows the tool's own schema: a form can come later,
      once a schema is worth rendering.
- [x] Consent and change visibility, which is what a client can actually do: a
      command nobody has read does not run (shown in full, never truncated, with
      an explicit "run it"), and `trust.json` records the command, its arguments,
      the environment variable *names*, and every tool's name, description and
      schema. A changed argument, command, environment variable or tool
      description is named on screen and has to be accepted again.
- [ ] Signatures and attestations are deliberately not invented here. The
      specification is explicit that they say who you are trusting, not that the
      trust is warranted; verifying them belongs to Sigstore and to registry
      tooling, and an invented format would have nothing to interoperate with.
      Worth doing when the app imports servers from somewhere else - a link, a
      registry, a shared config - which is also when SEP-1024's consent flow stops
      being satisfied by reading your own file.
- [x] A frontend test harness: vitest, jsdom and testing-library, wired into
      `scripts/check.ps1`. The first tests cover the rules that are easy to get
      silently wrong — chord matching, profile ids, argument and environment
      parsing, palette ranking — and two panels, including the consent step: an
      unapproved command offers "run it", approving asks the app to approve, and
      a changed tool list has to be accepted. It found a real defect on its first
      run: "accept it" called `plugins_open`, so a rewritten tool list could
      never be accepted.

## P5 — control rooms

- [ ] Host-authoritative room over QUIC, LAN first
- [ ] Share pane / tab / computer, per-pane ACL
- [ ] Presence and follow mode

## P6 — mesh

- [ ] Device identity (ed25519), pairing by code/QR, TOFU pinning
- [ ] Standing access password, temporary grants (≤ 7 days)
- [ ] My computers: open a pane on another machine
- [ ] Self-hosted relay
