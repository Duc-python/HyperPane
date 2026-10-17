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
    /// runs; dropping it would close the terminal.
    _master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    reader: Option<thread::JoinHandle<()>>,
    waiter: Option<thread::JoinHandle<()>>,
}

impl Drop for PaneSession {
    fn drop(&mut self) {
        let _ = self.killer.kill();
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.waiter.take() {
            let _ = handle.join();
        }
    }
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
                let mut buf = [0u8; READ_BUF];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = decoder.push(&buf[..n]);
                            if !chunk.is_empty() {
                                reader_sink(PtyEvent::Data { chunk });
                            }
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
                _master: pair.master,
                writer,
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
        pane.writer
            .write_all(data.as_bytes())
            .and_then(|_| pane.writer.flush())
            .map_err(|e| PtyError::Backend(e.to_string()))
    }

    /// Tell the PTY the viewport changed so full-screen programs can reflow.
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        let panes = self.panes.lock();
        let pane = panes
            .get(id)
            .ok_or_else(|| PtyError::NotFound(id.to_string()))?;
        pane._master
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
}
