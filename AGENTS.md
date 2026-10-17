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

1. Read `TODO.md` for the phase in progress.
2. Read `docs/architecture.md` for the data flow.
3. Run the checks once before changing anything, so you know the baseline.

## What it is

A local-first desktop terminal. Each pane can host a shell, a remote session or
a coding agent, and a lead agent can open, brief and collect results from other
agents in panes beside it. No account, no server, nothing leaves the machine
unless a person opens it.

## Commands

```bash
pnpm install
pnpm dev                                   # frontend only, on :5173
pnpm tauri dev                             # the app (the only way to run it)
pnpm --filter @hyperpane/desktop typecheck
pnpm --filter @hyperpane/desktop build
```

```powershell
# Windows: cargo needs link.exe, which a plain shell does not have.
pwsh -File scripts/with-msvc.ps1 -- cargo test --workspace
pwsh -File scripts/with-msvc.ps1 -- cargo clippy --workspace --all-targets -- -D warnings
pwsh -File scripts/with-msvc.ps1 -- cargo fmt --all
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
| `hp-agents` | Agent catalogue and launch adapters |
| `hp-store` | Session persistence, written atomically |
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

## Definition of done

A change is not finished until all of these hold:

1. `cargo fmt --all --check` is clean.
2. `cargo clippy --workspace --all-targets -- -D warnings` is clean.
3. `cargo test --workspace` passes, with a test for the behaviour you added.
4. `pnpm --filter @hyperpane/desktop typecheck` and `build` pass.
5. The app runs and the change is visible in it. For UI work, capture it:

```powershell
pwsh -File scripts/capture-window.ps1 -Exe target\debug\hyperpane.exe -Out shot.png
```

Then tick the line in `TODO.md` and commit. Small commits that each end green
beat one large commit that does not.

The app can be driven and photographed without a person at the screen:

```powershell
# Split twice, then photograph the result
pwsh -File scripts/capture-window.ps1 -Exe target\debug\hyperpane.exe `
  -Out shot.png -Keys @("^+e", "^+o") -ErrorLog app.err
```

`-Repeat` turns that into a soak run, `-ErrorLog` catches a panic, and the
script reports handle and thread counts so a leak shows up as a climbing
number. It refuses to send a keystroke unless the app holds the foreground,
because the alternative is typing `{ENTER}` into somebody else's terminal.

## Reporting

State what was observed and what is inference, and keep them apart. Cite the
command output that proves a claim. When something was not verified, say so
plainly rather than implying it works — an unverified claim is worth less than a
named gap.
