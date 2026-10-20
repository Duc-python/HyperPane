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
- [x] Command palette (`mod+shift+p`) with fuzzy filtering. A row's label keeps
      the room it needs and a long hint gives way: "Update from source…" was
      showing as "Up…" beside its explanation.
- [x] OSC 7 working directory in the pane header
- [x] Pane titles from OSC 0/2, with the profile name as a fallback
- [x] Shell integration: generated hooks (PowerShell via `-EncodedCommand`,
      bash via `--rcfile`) that make OSC 7 and OSC 133 fire
- [x] Exit status of the last command, shown in the pane header
- [x] Command duration from the `C` mark, for shells that emit one
      (bash does, through `PS0`)
- [x] Jump to the previous or next prompt in the scrollback. Every OSC 133 mark
      in a read is delivered, not only the last: a shell emits the end of one
      command and the start of the next prompt back to back, so a coalesced read
      used to lose the exit status and leave a finished command looking like it
      was still running. Found by an opencode run inside the app.
- [x] A pane's shell and scrollback survive a layout change. A session lives
      outside the view that draws it, so splitting a pane no longer restarts the
      pane beside it. Do not put a PTY back inside a component: the element tree
      mirrors the layout tree, so a split unmounts and remounts panes.
- [ ] PowerShell command duration: needs a PSReadLine Enter handler, which
      replaces a key people rely on. Not shipped while it cannot be verified.
- [x] Profile editor in the UI: `mod+,`, the `Profiles…` palette entry, or the
      profile name in the title bar
- [x] A tab shrinks before the strip cuts one off. Three panes at 900px wide used
      to end in a half-drawn tab at the right edge, with no ellipsis and nothing
      to say it scrolled; the tabs now give way down to a floor, and only then
      does the strip scroll.

## P2 — agents

- [x] Agent catalogue as data, with per-agent launch rules
- [x] Open an installed agent in a pane from the palette, task typed there
- [x] Read the child's own `RESULT` line and show it as a claim. A run started
      on its own is now asked for that line - the task ends with it, on one line
      so an agent whose task is typed into its prompt does not submit at the
      first newline. Team briefs always asked; single runs never did, so their
      pane had nothing to show.
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
- [x] The superseded `agent_launch` and `agent_worktree` commands are gone, with
      their frontend wrappers: `agent_open` does both behind the guard, and two
      ways to start an agent is one way too many.
- [x] Every file Settings names can be shown in the file manager. The button says
      "reveal" and the row's path is still selectable text, because the reason to
      look at these files is usually to copy a line out of one. The file manager
      selects the file rather than opening it: these are the app's records, and
      "open" on a JSON file is a program nobody chose.
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
- [x] The dock can pair two machines. One side shows a code and the address it
      listens on; the other types or pastes them - a pasted
      `192.168.1.4:51234 ABCD-EFGH-IJKL` fills both fields - and both write each
      other into `peers.json` beside `trust.json`. The wait is polled once a
      second, because the other machine is what ends it. Three things are worth
      naming:
      - the listener is polled, not blocked in `accept`, so a panel that is closed
        stops the wait; a generation counter drops an answer that arrives after a
        stop, instead of writing it into a panel that has moved on;
      - the code is stretched on its own thread (600,000 rounds is seconds of
        work), and the code itself is never written to the activity log - the
        listening line names the port, nothing more;
      - a wrong code from the other side does not end the wait: it is logged and
        the hint says so. A mistyped code costs the guest a retry, not the host a
        new code.
      The four-message framing moved out of the socket test and into
      `hp_identity::pairing::{host_over, join}`, so there is one implementation of
      it rather than two.
- [x] Verified by pairing two instances of the real app on one machine, each with
      its own identity: `HYPERPANE_DATA_DIR` moves the data directory, which is
      how a second instance gets a second key (two instances sharing one identity
      would be refused as the same machine). The code was shown on one side,
      pasted into the other, and both `peers.json` files ended up pinning the
      other's id with both activity logs saying `pairing.paired` at the same
      millisecond; the dock showed the peer and the list on both sides.
      Two real defects came out of it. Windows hands a socket accepted from a
      non-blocking listener the same non-blocking mode, so the host's opening
      message failed and the guest saw a reset (`os error 10053`) - `host_over`
      now sets the accepted socket to block before it waits, and a test polls a
      non-blocking listener so the bug cannot come back. And a broken exchange
      used to end the host's wait; it is now logged and the host keeps listening.
- [ ] Pairing is groundwork, not a feature anybody can use yet: nothing consumes
      `peers.json`. Rooms are still loopback and unauthenticated, which the panel
      says in as many words. The next step is QUIC with the certificate bound to
      the identity, then a room that only admits a pinned id.
- [ ] A code is only as strong as the channel it travels on: this is not a PAKE,
      so whoever records the exchange can still guess the code offline. That is
      why it is sixty bits with a stretch, and why the module doc says so rather
      than implying more.
- [ ] Windows may ask once to let the app accept connections on a private
      network. Pairing worked here without it because both ends were on this
      machine; the prompt is expected the first time a real second machine
      connects.
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
- [x] An agent pane comes back as the agent. The launch itself is stored beside
      the profile - program, arguments, and the worktree it was confined to - so a
      reload brings back the same CLI in the same directory, with its branch still
      shown, rather than a shell where a run used to be. The task is deliberately
      *not* stored: sending an order again to an agent nobody is watching is a
      decision, not a default, so a restored run comes back waiting for one. A
      stored launch is dropped as soon as its pane is gone, by the session file and
      again by the app, so a later split that reuses a pane id cannot inherit it.
- [x] A restart proved the task *was* on disk: it travelled inside the stored
      launch arguments, so the next start gave the same order a second time and
      left the sentence in plaintext. A pane keeps two launches now - the live one
      with its task, and the same CLI without a task - and the session store keeps
      the second itself, so no caller can put a task in the file. An older file
      loses its arguments when it loads. Verified by restarting: the pane came
      back as an idle opencode in its worktree, with no `agent.open` in the log.
- [x] The pairing test's read timeout was five seconds, which is less than the
      PBKDF2 stretch it deliberately pays (600,000 rounds). It failed on a loaded
      machine while the protocol was working perfectly. Thirty seconds now, still
      bounded so a real protocol mistake fails rather than hanging.
- [x] A webview reload reconnects instead of restarting. Every event a pane emits
      now passes through one stream that keeps the last half-megabyte of output,
      and `pane_attach` hands that stream to a view that has just been built: the
      history first, then everything new, in order, under one lock, so the window
      cannot show an event twice or miss one. A reload therefore brings back the
      pane's own screen and leaves whatever was running running. Boot closes only a
      session no pane claims (`orphanPanes`) - the leftover of a pane that was
      closed while the window was away - which is the part of the old behaviour
      worth keeping. Verified in the running app: a pane ticking once a second came
      back at the same count with the same process id, its prompt and scrollback
      replayed, no `pane.close` or `pane.open` in the activity log, and a split
      after the reload still opened a *fresh* shell. The cap is honest: half a
      megabyte of output returns; scrollback older than that was the terminal's,
      and the reload took it.
- [x] `Review worktrees…` shows what the app's own worktree directory holds, and
      what each entry would cost to remove. A run that left nothing behind goes in
      one click; a run with uncommitted work needs a second click that says so, and
      the row states the cost in words ("1 uncommitted file would be lost;
      agent/9aab3a6a stays"). A directory that is no longer a worktree is removed
      only when it is empty, unless a person has answered twice. Branches are never
      deleted - only a person deletes work - so commits stay reachable by name.
      The path is held against the app's own directory before anything is touched,
      and every removal is in the activity log. A worktree a pane is running in
      says so - "a run is using this" - and its removal waits for that pane to
      close, because the app already knows which panes are open and an operating
      system's directory-in-use error reads like a bug in the app.
- [x] A directory that is already a worktree of a *different* repository is now
      refused instead of reused. A branch name comes from a run's seed alone, so
      two runs in two repositories can derive the same name, and reusing the
      directory would have handed one checkout to both repositories.
- [ ] Windows can refuse a removal while another process holds the directory: an
      empty directory that can be neither deleted nor renamed, with

          could not remove ...: The process cannot access the file because it is
          being used by another process. (os error 32)

      which is what the panel shows - the operating system's own words rather than
      a guess. The usual reason is a process whose working directory is inside the
      worktree, and this app does not force a deletion it cannot justify. A reload
      retries once that process has gone.
