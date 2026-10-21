# Working in HyperPane

Instructions for an agent or a person changing this repository.

## The rule that matters most

HyperPane is an independent implementation. It shares no code with any existing
terminal product.

- Build from public specifications and public documentation: the MCP spec,
  xterm.js, ConPTY and `openpty`, QUIC (RFC 9000), SSH, the OSC escape
  sequences.
- Never copy code, assets, prompts, preset text or UI strings out of another
  product's binary, package or repository.
- Never patch, bypass or reimplement another product's licence check. No
  feature here exists to unlock someone else's paid tier.

If a task asks for either of the last two, stop and say so instead of doing it.

## Language

Everything committed to this repository is written in **English**: code,
identifiers, comments, doc comments, commit messages, test names, error
messages, `TODO.md`, `AGENTS.md`, and UI strings.

Localised text in `presets/` carries English first; other languages are
additional keys, never replacements.

Conversation with the person you work for is separate: reply in the language
they write in. This rule covers what lands in the repository, not chat.

## Start here

1. Read `TODO.md` for the phase in progress and the gaps that are already known.
2. Read `docs/architecture.md` for the data flow.
3. Run `.\scripts\check.ps1` once before changing anything, so you know the baseline.

## What it is

A local-first desktop terminal. Each pane can host a shell, a remote session or
a coding agent, and a lead agent can open, brief and collect results from other
agents in panes beside it. Teams run in waves; a room shares panes with another
instance on the same machine (loopback only today). No account, no server,
nothing leaves the machine unless a person opens it.

## Commands

```bash
pnpm install
pnpm dev                                   # frontend only, on :5173
pnpm tauri dev                             # the full app
pnpm --filter @hyperpane/desktop typecheck
pnpm --filter @hyperpane/desktop build     # also required before cargo compiles the app crate
```

```powershell
# Windows: cargo needs link.exe, which a plain shell does not have.
# These run in Windows PowerShell 5.1; pwsh is not installed on every machine.
.\scripts\with-msvc.ps1 -- cargo test --workspace
.\scripts\with-msvc.ps1 -- cargo clippy --workspace --all-targets -- -D warnings
.\scripts\with-msvc.ps1 -- cargo fmt --all

# One crate, or one test by name
.\scripts\with-msvc.ps1 -- cargo test -p hp-teams
.\scripts\with-msvc.ps1 -- cargo test -p hp-teams a_three_case_debate
pnpm --filter @hyperpane/desktop exec vitest run src/lib/panes.test.ts

# The whole gate: fmt, clippy -D warnings, cargo test, typecheck, vitest, build.
.\scripts\check.ps1
```

```bash
# Linux and macOS
cargo test --workspace
```

The `--` before the command is required: cargo's own flags (`-p`, for one) look
like PowerShell parameter names and would otherwise be consumed by the script.

## Architecture

| Crate | Responsibility |
|---|---|
| `hp-pty` | PTY sessions, pane registry, incremental UTF-8 decoding, event streaming |
| `hp-core` | Domain: pane ids, split layout tree, workspaces, profiles, session |
| `hp-agents` | Agent catalogue, launch adapters, the `RESULT` claim reader |
| `hp-teams` | Team presets, plans, five-part briefs, waves, gates, router |
| `hp-shell` | Generated shell integration (OSC 7 / OSC 133) |
| `hp-store` | Session persistence, written atomically |
| `hp-mcp` | MCP client over stdio, both protocol eras |
| `hp-room` | Room rules, host loop and client (TCP, loopback) |
| `hp-identity` | Device key pair, pairing exchange, pinned peers |
| `apps/desktop` | Tauri shell (Rust commands) and the React UI |

Four invariants. Breaking one is a bug even if the tests pass:

1. **The frontend owns identity, the backend owns state.** The UI generates pane
   ids; Rust holds the layout. A layout command returns the whole workspace, so
   the UI never rebuilds the tree itself.
2. **Focus is optimistic, layout is authoritative.** A click must feel instant;
   a split must be correct.
3. **A drag commits once.** Dividers update locally while dragging and round-trip
   to the backend on release. IPC per pointer move makes resizing feel like a
   distributed system.
4. **Ratios are clamped on both sides** of the IPC boundary, so no drag can
   produce a pane that cannot be clicked.

## Agents and teams

A child agent gets one text and no conversation history, so the launch path is
where silent failures live:

- **The command line is the limit.** A `.cmd`/`.bat` shim is started through
  `cmd.exe`, which ends the command at the first line break and at 8191
  characters. Arguments that go through a shim are reduced to one line, and a
  brief is capped at 7 KiB (`hp-teams`). Without both, an agent received the
  single word "GOAL", and a three-case judge never started at all.
- **A brief is five parts**: goal, scope, specification, what earlier waves
  said, verification, and the `RESULT status=... files=... tests=...` line the
  app reads back. That line is a claim about the work, not a verified result.
- **A later wave reads an earlier one.** `team_advance` hands every member the
  previous waves' claims and findings, clipped, each with the file it came from.
  Do not drop that: a challenger's job is to answer findings it was given.
- **A member's turn ends at its report, not its process.** opencode returns to
  its own prompt instead of exiting, so a wave waits for a report plus five
  seconds of quiet. `running_panes` and the claim snapshot share that rule.
- **The claim reader is deliberately forgiving** (`hp-agents::result`): block
  form, box borders, ANSI and cursor-move rows, a row the terminal wrapped
  mid-path, `files=none`. Its tests are the specification; a real report shape
  that fails should become one.

## Conventions

**Rust.** Edition 2021, no `unsafe`. Errors are typed with `thiserror` in the
crate that owns them; commands convert to `String` at the Tauri boundary.
Tests live in the module under `#[cfg(test)]` and are named as sentences about
behaviour (`the_last_pane_cannot_be_closed`), not as `test_foo`.

**TypeScript.** Strict, plus `noUncheckedIndexedAccess` and
`exactOptionalPropertyTypes`. Optional props need a default at the call site
(`disabled={disabled ?? false}`), because `undefined` is not assignable.

**No StrictMode.** A `TerminalPane` effect owns a real PTY and a WebGL context.
The development double-invoke would open a second shell inside one pane. Pane
lifecycle is explicit rather than idempotent.

**Closing a pane ends its console's processes.** A ConPTY close reaches the
processes attached to it, so an agent's helpers exit with the pane. Verified by
comparing process start times after a run, not assumed: nothing to clean up by
hand, and no need for a job object yet.

**Comments say why.** A comment that restates the code is noise. A comment that
records a constraint, a platform quirk or a rejected alternative is the point of
having it.

## Platform traps

Each of these cost real debugging time. They are not hypothetical.

1. **Windows needs the Windows SDK, not just the C++ tools.** Without it,
   linking fails with `LNK1181: cannot open input file 'kernel32.lib'`.
2. **`MainWindowHandle` returns the console window** of a debug build, not the
   app window. Enumerate the process's top-level windows instead.
3. **`GetWindowRect` includes the drop shadow**, so a screen capture picks up
   pixels of whatever is behind the window. Use the DWM frame bounds.
4. **`Path.GetFullPath` anchors to the process working directory**, not to where
   PowerShell `cd`'d. Resolve against `(Get-Location).ProviderPath`.
5. **A debug build loads `devUrl`.** Running `target\debug\hyperpane.exe`
   directly shows `ERR_CONNECTION_REFUSED` unless Vite is up. Use `pnpm tauri
   dev`. Note also that Vite may bind `::1` only, so a `127.0.0.1` reachability
   check gives a false negative.
6. **A Vietnamese input method rewrites injected keystrokes.** With a TELEX IME
   active, `SendKeys.SendWait("worktree")` arrives as `ioktree`: `w` and `r` are
   modifier keys in TELEX, and they transform the text on the way in. This is
   the IME behaving correctly, not a bug in the app. When driving the app for a
   screenshot, choose query text without `s f r x j z w` next to a vowel, or the
   test will chase a character-loss ghost that does not exist. `tree` is a good
   query; `worktree` is not.
7. **`apps/desktop/dist` must exist before cargo compiles the app crate.**
   `tauri-build` reads `frontendDist` at compile time and fails when the
   directory is missing. CI builds the frontend first for that reason; locally,
   run `pnpm --filter @hyperpane/desktop build` before `cargo test` or `build`.
8. **A release needs `--features custom-protocol`.** Without it the binary keeps
   the development `devUrl` and opens `localhost:5173`, so the window shows a
   connection error even though the frontend was built. `pnpm tauri build` adds
   the feature itself; a plain `cargo build --release` must spell it out.
9. **The running executable is locked on Windows.** Stop the app before
   rebuilding, or cargo fails at the last step with `Access is denied`.

## Definition of done

A change is not finished until all of these hold:

1. `.\scripts\check.ps1` passes — format, clippy with `-D warnings`, Rust tests,
   web typecheck, vitest and the web build, in that order, stopping at the first
   failure.
2. There is a test for the behaviour you added, in the module that owns it.
3. The app runs and the change is visible in it. For UI work, capture it:

```powershell
.\scripts\capture-window.ps1 -Exe target\debug\hyperpane.exe -Out shot.png -KeepRunning
```

`-KeepRunning` leaves the app up for a person or a driver to use it. Then tick
the line in `TODO.md` and commit. Commits are `type(scope): summary`, imperative,
with the reasoning in the body; small green commits beat one big one.

The app can be driven and photographed without a person at the screen:

```powershell
# Split twice, then photograph the result
.\scripts\capture-window.ps1 -Exe target\debug\hyperpane.exe `
  -Out shot.png -Keys @("^+e", "^+o") -ErrorLog app.err
```

`-Repeat` turns that into a soak run, `-ErrorLog` catches a panic, and the
script reports handle and thread counts so a leak shows up as a climbing
number. It refuses to send a keystroke unless the app holds the foreground,
because the alternative is typing `{ENTER}` into somebody else's terminal.

## Watching a real run

The data directory is the ground truth: on Windows
`%APPDATA%\app.hyperpane.desktop`, moved by `HYPERPANE_DATA_DIR`. `audit.log` has
every spawn, refusal and team wave; `findings/` has what each member wrote;
`session.json` has the layout and which pane runs which agent. A second instance
for pairing tests needs its own `HYPERPANE_DATA_DIR`, or both share one identity.

## Reporting

State what was observed and what is inference, and keep them apart. Cite the
command output that proves a claim. When something was not verified, say so
plainly rather than implying it works — an unverified claim is worth less than a
named gap.
