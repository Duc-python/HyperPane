//! PTY sessions for HyperPane.
//!
//! One [`PaneSession`] owns one pseudo-terminal and the child process running
//! inside it. Output is decoded incrementally, so a multi-byte character split
//! across two reads is never corrupted, and forwarded to a caller-supplied sink.
//!
//! Platform behaviour is delegated to `portable-pty`: ConPTY on Windows,
//! `openpty` elsewhere.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;

use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type PaneId = String;

/// Byte budget for one read from the PTY master.
const READ_BUF: usize = 16 * 1024;

/// Cap on bytes held back while waiting for an incomplete UTF-8 sequence to
/// finish. A sequence is at most 4 bytes; anything larger is a broken producer.
const PENDING_LIMIT: usize = 8;

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("pty backend error: {0}")]
    Backend(String),
    #[error("pane not found: {0}")]
    NotFound(PaneId),
    #[error("pane already exists: {0}")]
    AlreadyExists(PaneId),
    #[error("pane is closing: {0}")]
    Closing(PaneId),
}

/// One event emitted by a pane, in order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PtyEvent {
    /// Decoded terminal output. Already valid UTF-8.
    Data { chunk: String },
    /// The child exited. `code` is absent when the platform does not report one.
    Exit { code: Option<i32> },
    /// The reader failed; the pane is no longer producing output.
    Error { message: String },
    /// The shell reported its working directory with an OSC 7 sequence.
    Cwd { path: String },
    /// The shell reported a prompt boundary with an OSC 133 sequence.
    ///
    /// The field is `mark` and not `kind`, because the variant tag already owns
    /// that name: two fields called `kind` would produce a duplicate JSON key.
    ///
    /// `rename_all` is repeated here on purpose. At the container level it
    /// renames variants only, so without this the field would go out as
    /// `exit_code` and the frontend, which reads `exitCode`, would silently see
    /// nothing.
    #[serde(rename_all = "camelCase")]
    Prompt {
        mark: PromptKind,
        exit_code: Option<i32>,
    },
}

/// Which boundary of a prompt an `OSC 133` mark announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PromptKind {
    /// The prompt starts here.
    PromptStart,
    /// The prompt ends, so a typed command starts here.
    PromptEnd,
    /// The command began executing.
    CommandStart,
    /// The command finished.
    CommandEnd,
}

/// Everything needed to open one pane.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneSpec {
    pub id: PaneId,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_cols")]
    pub cols: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
    /// Whether the shell should load HyperPane's prompt hooks. Applied by the
    /// app layer, which knows where the scripts live; the PTY itself does not
    /// care either way.
    #[serde(default = "default_true")]
    pub integration: bool,
    /// Catalogue id when this pane runs an agent, so the app can watch for its
    /// report. `None` for a plain shell.
    #[serde(default)]
    pub agent: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_cols() -> u16 {
    120
}

fn default_rows() -> u16 {
    34
}

/// Where pane events go. The Tauri layer passes a channel-backed closure.
pub type EventSink = Arc<dyn Fn(PtyEvent) + Send + Sync + 'static>;

/// Returns a sensible interactive shell for the current platform.
pub fn resolve_shell() -> String {
    #[cfg(windows)]
    {
        for candidate in ["pwsh.exe", "powershell.exe"] {
            if which(candidate) {
                return candidate.to_string();
            }
        }
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

#[cfg(windows)]
fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

struct PaneSession {
    /// Kept alive so the master side of the PTY is not dropped while the child
    /// runs. Dropping it closes the terminal, which is why `drop` takes it
    /// before waiting on anything.
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    reader: Option<thread::JoinHandle<()>>,
    waiter: Option<thread::JoinHandle<()>>,
}

impl Drop for PaneSession {
    fn drop(&mut self) {
        let _ = self.killer.kill();

        // Order matters, and getting it wrong froze the app. A custom `Drop`
        // runs *before* the struct's fields are dropped, so a join here waits
        // for a read that can only end once the master side is gone, and the
        // master is still very much alive at this point. Close our end first.
        self.master.take();
        self.writer.take();

        // Even with the master closed, a descendant of the child can hold the
        // pty open, and the read would not return for as long as it lives.
        // Waiting for that on the caller's thread blocks whoever asked for the
        // close - and a window close runs here, which is exactly how an app
        // ends up "not responding". So the wait happens to the side, and
        // nobody waits for the wait.
        if let Some(handle) = self.reader.take() {
            reap(handle);
        }
        if let Some(handle) = self.waiter.take() {
            reap(handle);
        }
    }
}

/// Waits for a pane thread without making anyone else wait for it.
fn reap(handle: thread::JoinHandle<()>) {
    let _ = thread::Builder::new()
        .name("hp-pty-reap".to_string())
        .spawn(move || {
            let _ = handle.join();
        });
}

/// Owns every live pane in the process.
#[derive(Default)]
pub struct PaneManager {
    panes: Mutex<HashMap<PaneId, PaneSession>>,
}

impl PaneManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a pane and start streaming its output to `sink`.
    pub fn spawn(&self, spec: PaneSpec, sink: EventSink) -> Result<(), PtyError> {
        let mut panes = self.panes.lock();
        if panes.contains_key(&spec.id) {
            return Err(PtyError::AlreadyExists(spec.id));
        }

        let size = PtySize {
            rows: spec.rows,
            cols: spec.cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = native_pty_system()
            .openpty(size)
            .map_err(|e| PtyError::Backend(e.to_string()))?;

        let program = spec.shell.clone().unwrap_or_else(resolve_shell);
        let mut command = CommandBuilder::new(&program);
        for arg in &spec.args {
            command.arg(arg);
        }
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        // Inherit the pane's configured directory, else the app's own.
        match &spec.cwd {
            Some(dir) => command.cwd(dir),
            None => {
                if let Ok(dir) = std::env::current_dir() {
                    command.cwd(dir);
                }
            }
        }
        command.env("TERM", "xterm-256color");
        command.env("TERM_PROGRAM", "HyperPane");
        command.env("COLORTERM", "truecolor");

        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| PtyError::Backend(e.to_string()))?;
        // The slave handle must be released, or the PTY never reports EOF.
        drop(pair.slave);

        let killer = child.clone_killer();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Backend(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Backend(e.to_string()))?;

        let reader_sink = Arc::clone(&sink);
        let reader_handle = thread::Builder::new()
            .name(format!("hp-pty-read-{}", spec.id))
            .spawn(move || {
                let mut decoder = Utf8Stream::default();
                let mut directories = Osc7Scanner::default();
                let mut prompts = Osc133Scanner::default();
                let mut buf = [0u8; READ_BUF];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = decoder.push(&buf[..n]);
                            if chunk.is_empty() {
                                continue;
                            }
                            // Reports travel beside the output rather than
                            // replacing it: the terminal still needs the bytes
                            // it was sent.
                            if let Some(path) = directories.push(&chunk) {
                                reader_sink(PtyEvent::Cwd { path });
                            }
                            if let Some(mark) = prompts.push(&chunk) {
                                let (prompt_mark, exit_code) = match mark {
                                    PromptMark::Start => (PromptKind::PromptStart, None),
                                    PromptMark::End => (PromptKind::PromptEnd, None),
                                    PromptMark::CommandStart => (PromptKind::CommandStart, None),
                                    PromptMark::CommandEnd(code) => (PromptKind::CommandEnd, code),
                                };
                                reader_sink(PtyEvent::Prompt {
                                    mark: prompt_mark,
                                    exit_code,
                                });
                            }
                            reader_sink(PtyEvent::Data { chunk });
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| PtyError::Backend(e.to_string()))?;

        let waiter_sink = Arc::clone(&sink);
        let waiter_handle = thread::Builder::new()
            .name(format!("hp-pty-wait-{}", spec.id))
            .spawn(move || {
                let code = match child.wait() {
                    Ok(status) => Some(status.exit_code() as i32),
                    Err(_) => None,
                };
                waiter_sink(PtyEvent::Exit { code });
            })
            .map_err(|e| PtyError::Backend(e.to_string()))?;

        panes.insert(
            spec.id,
            PaneSession {
                master: Some(pair.master),
                writer: Some(writer),
                killer,
                reader: Some(reader_handle),
                waiter: Some(waiter_handle),
            },
        );
        Ok(())
    }

    /// Send input to a pane's stdin.
    pub fn write(&self, id: &str, data: &str) -> Result<(), PtyError> {
        let mut panes = self.panes.lock();
        let pane = panes
            .get_mut(id)
            .ok_or_else(|| PtyError::NotFound(id.to_string()))?;
        let writer = pane
            .writer
            .as_mut()
            .ok_or_else(|| PtyError::Closing(id.to_string()))?;
        writer
            .write_all(data.as_bytes())
            .and_then(|_| writer.flush())
            .map_err(|e| PtyError::Backend(e.to_string()))
    }

    /// Tell the PTY the viewport changed so full-screen programs can reflow.
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        let panes = self.panes.lock();
        let pane = panes
            .get(id)
            .ok_or_else(|| PtyError::NotFound(id.to_string()))?;
        pane.master
            .as_ref()
            .ok_or_else(|| PtyError::Closing(id.to_string()))?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Backend(e.to_string()))
    }

    /// Close a pane and end its child process.
    pub fn kill(&self, id: &str) -> Result<(), PtyError> {
        let pane = self
            .panes
            .lock()
            .remove(id)
            .ok_or_else(|| PtyError::NotFound(id.to_string()))?;
        drop(pane);
        Ok(())
    }

    pub fn list(&self) -> Vec<PaneId> {
        let mut ids: Vec<PaneId> = self.panes.lock().keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn len(&self) -> usize {
        self.panes.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// End every pane. Called on app shutdown.
    pub fn shutdown(&self) {
        let panes: Vec<PaneId> = self.list();
        for id in panes {
            let _ = self.kill(&id);
        }
    }
}

/// Reassembles UTF-8 across read boundaries.
///
/// A PTY read can end in the middle of a multi-byte character. Decoding each
/// read independently would emit a replacement character and corrupt
/// box-drawing and emoji output, so incomplete tails are held until the next
/// read completes them.
#[derive(Default)]
struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    fn push(&mut self, chunk: &[u8]) -> String {
        self.pending.extend_from_slice(chunk);
        let mut out = String::with_capacity(self.pending.len());

        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    out.push_str(valid);
                    self.pending.clear();
                    break;
                }
                Err(err) => {
                    let valid_up_to = err.valid_up_to();
                    if valid_up_to > 0 {
                        if let Ok(valid) = std::str::from_utf8(&self.pending[..valid_up_to]) {
                            out.push_str(valid);
                        }
                        self.pending.drain(..valid_up_to);
                    }
                    match err.error_len() {
                        // Genuinely invalid bytes: replace once and skip them.
                        Some(len) => {
                            out.push('\u{FFFD}');
                            self.pending.drain(..len);
                        }
                        // Incomplete tail: wait for the rest of the character.
                        None => break,
                    }
                }
            }
        }

        // Never let a stalled producer grow the buffer without bound.
        if self.pending.len() > PENDING_LIMIT {
            let extra = self.pending.len() - PENDING_LIMIT;
            self.pending.drain(..extra);
            out.push('\u{FFFD}');
        }
        out
    }
}

/// The start of an OSC 7 report: `ESC ] 7 ;`.
const OSC7_PREFIX: &str = "\x1b]7;";

/// The start of an OSC 133 prompt mark: `ESC ] 133 ;`.
const OSC133_PREFIX: &str = "\x1b]133;";

/// Pulls complete OSC sequences with one prefix out of the output stream.
///
/// Reports arrive mixed into ordinary output and can be split across two reads,
/// so an incomplete sequence is held until the next chunk completes it.
///
/// The bytes are not removed from the stream: the terminal still receives
/// everything the child wrote.
#[derive(Default)]
struct OscExtractor {
    pending: String,
}

impl OscExtractor {
    /// A report is a URL or a marker, not a document; anything longer is not.
    const MAX_PENDING: usize = 8 * 1024;

    /// Feed decoded output; return the payload of every complete report.
    fn drain(&mut self, chunk: &str, prefix: &str) -> Vec<String> {
        self.pending.push_str(chunk);
        let mut payloads = Vec::new();

        loop {
            let Some(start) = self.pending.find(prefix) else {
                // Keep only a tail that could still become the marker.
                let keep = partial_marker_len(&self.pending, prefix);
                let cut = self.pending.len() - keep;
                self.pending.drain(..cut);
                break;
            };

            let after = start + prefix.len();
            let Some((payload_len, terminator_len)) = find_terminator(&self.pending[after..])
            else {
                // The report has not finished arriving yet.
                self.pending.drain(..start);
                break;
            };

            payloads.push(self.pending[after..after + payload_len].to_string());
            self.pending.drain(..after + payload_len + terminator_len);
        }

        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
        }
        payloads
    }
}

/// Reads working-directory reports (`OSC 7`).
#[derive(Default)]
struct Osc7Scanner {
    extractor: OscExtractor,
}

impl Osc7Scanner {
    /// The directory from the last complete report in this chunk, if any.
    fn push(&mut self, chunk: &str) -> Option<String> {
        self.extractor
            .drain(chunk, OSC7_PREFIX)
            .into_iter()
            .filter_map(|payload| file_url_to_path(&payload))
            .next_back()
    }
}

/// A prompt boundary reported with `OSC 133`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptMark {
    /// `A`: the prompt starts here.
    Start,
    /// `B`: the prompt ends, so the command starts here.
    End,
    /// `C`: the command began executing.
    CommandStart,
    /// `D;<status>`: the command finished.
    CommandEnd(Option<i32>),
}

/// Reads prompt marks (`OSC 133`).
#[derive(Default)]
struct Osc133Scanner {
    extractor: OscExtractor,
}

impl Osc133Scanner {
    /// The last mark in this chunk, if any.
    fn push(&mut self, chunk: &str) -> Option<PromptMark> {
        self.extractor
            .drain(chunk, OSC133_PREFIX)
            .into_iter()
            .filter_map(|payload| parse_prompt_mark(&payload))
            .next_back()
    }
}

/// Parses `A`, `B`, `C`, or `D;<status>`.
fn parse_prompt_mark(payload: &str) -> Option<PromptMark> {
    let (kind, rest) = payload.split_once(';').unwrap_or((payload, ""));
    match kind {
        "A" => Some(PromptMark::Start),
        "B" => Some(PromptMark::End),
        "C" => Some(PromptMark::CommandStart),
        "D" => Some(PromptMark::CommandEnd(rest.trim().parse::<i32>().ok())),
        _ => None,
    }
}

/// Length of the longest proper prefix of `prefix` that `text` ends with.
fn partial_marker_len(text: &str, prefix: &str) -> usize {
    let bytes = text.as_bytes();
    let marker = prefix.as_bytes();
    for len in (1..marker.len()).rev() {
        if bytes.len() >= len && bytes[bytes.len() - len..] == marker[..len] {
            return len;
        }
    }
    0
}

/// Position and width of the earliest terminator in `text`.
fn find_terminator(text: &str) -> Option<(usize, usize)> {
    let bell = text.find('\x07').map(|index| (index, 1));
    let string_terminator = text.find("\x1b\\").map(|index| (index, 2));
    match (bell, string_terminator) {
        (Some(bell), Some(st)) => Some(if bell.0 <= st.0 { bell } else { st }),
        (Some(bell), None) => Some(bell),
        (None, Some(st)) => Some(st),
        (None, None) => None,
    }
}

/// Turn a `file://` URL from an OSC 7 report into a filesystem path.
fn file_url_to_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    // The authority runs to the first slash and is empty for a local report.
    let slash = rest.find('/')?;
    let path = &rest[slash..];
    if path.is_empty() {
        return None;
    }

    let decoded = percent_decode(path);

    // Windows shells report `/C:/Users/name`; the leading slash is not part of
    // the path.
    let bytes = decoded.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return Some(decoded[1..].to_string());
    }
    Some(decoded)
}

/// Decode `%XX` escapes. Invalid escapes are kept as they arrived.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 3 <= bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    index += 3;
                    continue;
                }
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_ascii_in_one_pass() {
        let mut stream = Utf8Stream::default();
        assert_eq!(stream.push(b"hello"), "hello");
        assert!(stream.pending.is_empty());
    }

    #[test]
    fn holds_incomplete_multibyte_then_completes_it() {
        let mut stream = Utf8Stream::default();
        // "ế" is E1 BA BF; split after the first two bytes.
        let bytes = "ế".as_bytes();
        let first = stream.push(&bytes[..2]);
        assert_eq!(first, "");
        let second = stream.push(&bytes[2..]);
        assert_eq!(second, "ế");
    }

    #[test]
    fn keeps_text_before_an_incomplete_tail() {
        let mut stream = Utf8Stream::default();
        let bytes = "box┌".as_bytes();
        let split = bytes.len() - 2;
        assert_eq!(stream.push(&bytes[..split]), "box");
        assert_eq!(stream.push(&bytes[split..]), "┌");
    }

    #[test]
    fn replaces_genuinely_invalid_bytes() {
        let mut stream = Utf8Stream::default();
        assert_eq!(stream.push(&[0xFF, 0xFE, b'a']), "\u{FFFD}\u{FFFD}a");
    }

    #[test]
    fn splits_a_longer_run_across_many_reads() {
        let mut stream = Utf8Stream::default();
        let text = "⠋⠙⠹⠸ building… done ✅";
        let bytes = text.as_bytes();
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            out.push_str(&stream.push(chunk));
        }
        assert_eq!(out, text);
    }

    #[test]
    fn reads_a_bell_terminated_directory_report() {
        let mut scanner = Osc7Scanner::default();
        assert_eq!(
            scanner
                .push("\x1b]7;file://host/home/name/project\x07")
                .as_deref(),
            Some("/home/name/project")
        );
    }

    #[test]
    fn reads_a_string_terminated_directory_report() {
        let mut scanner = Osc7Scanner::default();
        assert_eq!(
            scanner
                .push("\x1b]7;file://host/home/name\x1b\\")
                .as_deref(),
            Some("/home/name")
        );
    }

    #[test]
    fn finds_a_report_among_ordinary_output() {
        let mut scanner = Osc7Scanner::default();
        let output = "prompt$ \x1b]7;file://host/tmp\x07 more output\r\n";
        assert_eq!(scanner.push(output).as_deref(), Some("/tmp"));
    }

    #[test]
    fn completes_a_report_split_across_two_reads() {
        let mut scanner = Osc7Scanner::default();
        assert_eq!(scanner.push("output\x1b]7;file://host/va"), None);
        assert_eq!(scanner.push("r/log\x07rest").as_deref(), Some("/var/log"));
    }

    #[test]
    fn completes_a_report_split_inside_the_marker() {
        let mut scanner = Osc7Scanner::default();
        assert_eq!(scanner.push("noise\x1b"), None);
        assert_eq!(scanner.push("]7;file://host/a\x07").as_deref(), Some("/a"));
    }

    #[test]
    fn reports_the_last_directory_when_several_arrive() {
        let mut scanner = Osc7Scanner::default();
        let output = "\x1b]7;file://h/one\x07\x1b]7;file://h/two\x07";
        assert_eq!(scanner.push(output).as_deref(), Some("/two"));
    }

    #[test]
    fn decodes_percent_escapes_in_a_path() {
        let mut scanner = Osc7Scanner::default();
        let report = "\x1b]7;file://h/home/name/my%20project\x07";
        assert_eq!(
            scanner.push(report).as_deref(),
            Some("/home/name/my project")
        );
    }

    #[test]
    fn strips_the_leading_slash_before_a_windows_drive() {
        let mut scanner = Osc7Scanner::default();
        let report = "\x1b]7;file://h/C:/Users/name/repo\x07";
        assert_eq!(scanner.push(report).as_deref(), Some("C:/Users/name/repo"));
    }

    #[test]
    fn ignores_a_url_that_is_not_a_local_file_report() {
        let mut scanner = Osc7Scanner::default();
        assert_eq!(scanner.push("\x1b]7;https://example.com/x\x07"), None);
        assert_eq!(scanner.push("\x1b]7;file://host\x07"), None);
    }

    #[test]
    fn ignores_other_osc_sequences() {
        let mut scanner = Osc7Scanner::default();
        assert_eq!(scanner.push("\x1b]0;a window title\x07"), None);
        assert_eq!(scanner.push("\x1b]133;A\x07"), None);
    }

    #[test]
    fn does_not_keep_an_unbounded_buffer() {
        let mut scanner = Osc7Scanner::default();
        for _ in 0..100 {
            scanner.push(&"x".repeat(1024));
        }
        assert!(scanner.extractor.pending.len() <= OscExtractor::MAX_PENDING);
    }

    /// Closing a pane must not wait on a descendant that holds the pty open.
    ///
    /// This is the defect that made the window show "not responding". A custom
    /// `Drop` runs before the struct's fields are dropped, so joining the reader
    /// there waits for a read that can only finish once the master side is gone.
    /// It was still open. A grandchild holding the pty keeps the read alive even
    /// after the child is killed, so the wait had no end.
    #[cfg(windows)]
    #[test]
    fn killing_a_pane_returns_even_when_a_grandchild_holds_the_pty() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let manager = Arc::new(PaneManager::new());
        let sink: EventSink = Arc::new(|_| {});
        manager
            .spawn(
                PaneSpec {
                    id: "p".to_string(),
                    cwd: None,
                    shell: Some("cmd.exe".to_string()),
                    // `start /b` detaches a process that inherits the pty.
                    args: vec![
                        "/c".to_string(),
                        "start /b cmd /c ping -n 15 127.0.0.1 >nul".to_string(),
                    ],
                    env: Default::default(),
                    cols: 80,
                    rows: 24,
                    integration: false,
                    agent: None,
                },
                sink,
            )
            .expect("spawn");

        // Let the grandchild start and inherit the handles.
        std::thread::sleep(Duration::from_millis(1500));

        let worker = Arc::clone(&manager);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let started = Instant::now();
            let outcome = worker.kill("p");
            let _ = sender.send((started.elapsed(), outcome.is_ok()));
        });

        match receiver.recv_timeout(Duration::from_secs(6)) {
            Ok((elapsed, succeeded)) => {
                assert!(succeeded, "kill reported a failure");
                assert!(
                    elapsed < Duration::from_secs(5),
                    "closing a pane took {elapsed:?}; it must not wait on the pty"
                );
            }
            Err(_) => panic!("closing a pane did not return within 6 s: it is blocking"),
        }
    }

    #[test]
    fn reads_each_prompt_mark() {
        let mut scanner = Osc133Scanner::default();
        assert_eq!(scanner.push("\x1b]133;A\x07"), Some(PromptMark::Start));
        assert_eq!(scanner.push("\x1b]133;B\x07"), Some(PromptMark::End));
        assert_eq!(
            scanner.push("\x1b]133;C\x1b\\"),
            Some(PromptMark::CommandStart)
        );
    }

    #[test]
    fn reads_the_status_from_a_command_end_mark() {
        let mut scanner = Osc133Scanner::default();
        assert_eq!(
            scanner.push("\x1b]133;D;0\x07"),
            Some(PromptMark::CommandEnd(Some(0)))
        );
        assert_eq!(
            scanner.push("\x1b]133;D;130\x07"),
            Some(PromptMark::CommandEnd(Some(130)))
        );
    }

    #[test]
    fn a_command_end_mark_without_a_status_is_still_a_mark() {
        let mut scanner = Osc133Scanner::default();
        assert_eq!(
            scanner.push("\x1b]133;D\x07"),
            Some(PromptMark::CommandEnd(None))
        );
    }

    #[test]
    fn completes_a_prompt_mark_split_across_two_reads() {
        let mut scanner = Osc133Scanner::default();
        assert_eq!(scanner.push("out\x1b]13"), None);
        assert_eq!(
            scanner.push("3;D;1\x07more").as_ref(),
            Some(&PromptMark::CommandEnd(Some(1)))
        );
    }

    #[test]
    fn a_directory_report_is_not_a_prompt_mark() {
        let mut scanner = Osc133Scanner::default();
        assert_eq!(scanner.push("\x1b]7;file://h/tmp\x07"), None);
    }

    #[test]
    fn an_unknown_mark_is_ignored() {
        let mut scanner = Osc133Scanner::default();
        assert_eq!(scanner.push("\x1b]133;Z\x07"), None);
    }

    /// The event crosses a JSON boundary into TypeScript, which reads
    /// `exitCode`. A snake_case field there fails silently: the frontend sees
    /// `undefined` and draws an empty badge.
    #[test]
    fn a_prompt_event_serialises_with_camel_case_fields() {
        let event = PtyEvent::Prompt {
            mark: PromptKind::CommandEnd,
            exit_code: Some(7),
        };
        let json = serde_json::to_string(&event).expect("serialise");

        assert!(json.contains(r#""kind":"prompt""#), "{json}");
        assert!(json.contains(r#""mark":"commandEnd""#), "{json}");
        assert!(json.contains(r#""exitCode":7"#), "{json}");
        assert!(
            !json.contains("exit_code"),
            "field kept its Rust name: {json}"
        );
    }

    #[test]
    fn a_prompt_end_event_has_no_status() {
        let event = PtyEvent::Prompt {
            mark: PromptKind::PromptEnd,
            exit_code: None,
        };
        let json = serde_json::to_string(&event).expect("serialise");
        assert!(json.contains(r#""mark":"promptEnd""#), "{json}");
        assert!(json.contains(r#""exitCode":null"#), "{json}");
    }

    #[test]
    fn a_directory_event_keeps_its_field_names() {
        let event = PtyEvent::Cwd {
            path: "C:/src".to_string(),
        };
        let json = serde_json::to_string(&event).expect("serialise");
        assert_eq!(json, r#"{"kind":"cwd","path":"C:/src"}"#);
    }
}
