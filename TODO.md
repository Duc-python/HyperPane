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

- [x] `hp-room`: the rules, host-authoritative — participants, which panes are
      shared, and what each may do in each. Access filters the snapshot rather
      than only the actions, so a participant is never told that a pane it may not
      see exists. Presence never names an invisible pane either.
- [x] A stream for the room: newline-delimited JSON over TCP, with the message
      size bounded before parsing. This is what two builds on one machine will
      talk over before QUIC arrives, and it is explicitly not a security
      boundary — identity is whatever the stream is attached to.
- [x] The host loop and a client, in the crate: `Host` accepts, one thread per
      peer, applies what it sends and writes the events back; `Client` says hello
      and listens. Effects come back through a channel, because writing into a
      pane belongs to the app that owns the PTY. Seven tests over real loopback
      sockets, including output that must *not* reach someone without a grant.
- [x] The same in the app: nine commands, a panel, and the hook that mirrors a
      pane's output into the room. Verified with two instances on one machine: A
      hosts and shares its pane, B joins, A grants control, and a command typed in
      B runs in A's shell with the output arriving back in B's transcript. Every
      remote keystroke is a `room.input` line in the activity log; the text is not
      recorded, because a password is text.
- [ ] QUIC, replacing TCP. Until then a room is loopback only, and the panel says
      so rather than pretending it is ready for a network.
- [x] Follow on screen: the visitor's panel lists everyone with the pane they are
      looking at, offers follow and stop following, and marks the followed pane.
      That needed the transcript to be one block per pane rather than one stream.
- [x] The host's panel says what each person may do. It used to show the host's
      own access beside every name, which was a claim about the wrong person; the
      snapshot now carries the grants, and only to the host.
- [ ] The frontend tests do not run the poll: they mock `invoke` and render once.
      A bug caused by a redraw between typing and clicking (the command box being
      reset before send) was therefore invisible to them. Fake timers and a
      poll-driven test would cover that shape.
- [ ] Share pane / tab / computer in the UI, per-pane ACL.
- [ ] Presence and follow in the UI.

## P6 — mesh

- [x] `hp-identity`: this machine's key pair, its name (the public key itself, so
      nothing has to be looked up), signing and verification, and the pairing code
      two machines share — sixty bits of it, stretched before use, because a short
      code can be guessed from a recorded handshake.
- [x] `hp-identity::pairing`: the four-message exchange over any stream — two ids,
      two names, two nonces, and a proof each way, all over one transcript so
      nothing can be reordered, relabelled or replayed. Tested over a real
      loopback socket, including the host refusing and the guest being left with
      nothing to accept.
- [x] `hp-identity::peers`: the machines this one has paired with. A pinned id
      verifies its own signatures, which is why a reconnect needs no code. A new id
      borrowing a pinned name is reported rather than refused: a reinstalled
      machine must be pairable again, and only a person can say which of the two
      they meant.
- [ ] The app still cannot pair. The dock needs a code on one side and an address
      plus the code on the other, and `peers.json` beside `trust.json`. Until then
      a room is loopback and unauthenticated, which the panel says.
- [ ] QUIC for the room, with the certificate bound to the identity above.
- [ ] Standing access password, temporary grants (≤ 7 days)
- [ ] My computers: open a pane on another machine
- [ ] Self-hosted relay

## P7 — the app works on itself

- [x] The app knows its own source. `app_info` walks up from the executable to a
      directory that holds both the workspace and `crates/hp-core` and reports it.
      A packaged build finds neither and says so: the absence is the answer, not a
      failure.
- [x] `Work on HyperPane…` in the palette opens a pane on that source, so the app
      can be edited by an agent running inside it, under the same worktree and
      spawn guard as any other project. The directory travels in an ordinary
      profile (`HyperPane source`), so the pane is a normal pane and the shell it
      uses stays a person's choice; a checkout that moved is followed, because the
      source is where this build came from and a stale path is worse than an edit.
- [x] Settings shows that path with what it is for: pull, build, restart from a
      pane opened there.
- [x] `Update from source…` runs the update in a pane where it can be watched,
      read back and interrupted, rather than behind a person's back: `git pull
      --ff-only`, `pnpm install`, and then the build. The steps are guarded, so a
      failed pull stops the rest instead of leaving a green pane over a revision
      that never arrived.
- [ ] Windows cannot finish that update while the app runs: the executable is
      locked and cargo fails at the last step with "failed to remove file ...
      Access is denied" (measured, not assumed). So the compile is only checked
      there, and the last line of the pane names the command that finishes it once
      the window is closed. macOS and Linux build in place, because a running
      program keeps its inode.
- [ ] Self-update proper. Tauri's updater requires a signing key that must be kept
      forever, the public key in the config, and a published endpoint carrying
      signed artifacts. This app has no release channel, so there is nothing honest
      to point it at — and inventing one would mean a version of "updated" that is
      not true. Until then, updating means pulling and building the source that the
      pane above is open on.
- [ ] Restarting from inside the app (`tauri-plugin-process`), which is what would
      close the loop on Windows: quit, build, come back. Harmless alone, but it
      would read as "I updated myself" while the build is still the old one, so it
      belongs with the signed updater, once a release channel exists.
- [x] A pane comes back with the profile it opened with: the session stores the
      mapping beside the layout, and an entry is dropped when its pane is gone. A
      pane the file does not mention opens with the default profile, as does one
      whose profile has since been deleted - a pane has to open with something.
- [ ] An *agent* pane still comes back as a shell. The session stores the profile,
      not the agent, so a restart forgets which CLI a pane was running (and which
      worktree it was confined to). It needs the catalogue id and the worktree
      stored beside the profile.
- [ ] A webview reload restarts every shell. That is deliberate - on boot the app
      closes the leftover PTYs and lets each pane open its own, so nothing is
      orphaned - but it costs the scrollback and whatever was running. Re-attaching
      instead would need the backend to hand an existing session to a new channel.
