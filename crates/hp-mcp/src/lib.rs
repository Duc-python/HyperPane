//! MCP over stdio, as a client.
//!
//! HyperPane hosts MCP servers: it starts one as a child process and speaks the
//! Model Context Protocol to it, so a plugin is a program a person already has
//! rather than something this app implements. This crate is the wire: the
//! handshake, the framing, listing tools and calling one.
//!
//! The protocol has two eras, and a client that only speaks one is unusable with
//! half the servers in the wild, so this one speaks both:
//!
//! - **Modern** revisions carry the protocol version, the client's identity and
//!   its capabilities in every request's `_meta`, and have no handshake at all.
//!   A server that does not speak the offered version answers with
//!   `-32022` and lists what it does speak.
//! - **Legacy** revisions (`2025-11-25` and earlier) open with an `initialize`
//!   request and an `initialized` notification, and carry no `_meta`.
//!
//! Which era a server is in is decided by probing with `server/discover`, as the
//! stdio binding requires: a `DiscoverResult` or a version error means modern, a
//! timeout or any other error means legacy. The answer is cached for the life of
//! the process, because it is a property of the server and not of a request.
//!
//! Everything here is synchronous and one request at a time. The channel is a
//! single pipe, the app calls one tool at a time, and a blocking call is easier
//! to reason about than a multiplexer. The reader runs on its own thread only so
//! that a server which stops answering can be given up on instead of hanging the
//! app that started it.

pub mod host;

pub use host::{Host, Plugin, PluginError, PluginFile, PluginView, ToolView, EMPTY_FILE};

use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

/// The revision this build was written against. It is not a guess: it is the
/// current revision of the specification when this was written.
pub const MODERN_VERSION: &str = "2026-07-28";

/// The last revision that opened with `initialize`. Used only when the probe
/// says the server is legacy, because a legacy server cannot be talked to any
/// other way.
pub const LEGACY_VERSION: &str = "2025-11-25";

const CLIENT_NAME: &str = "HyperPane";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a modern server answers when it does not speak the offered version.
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// Bytes held from one read call.
const READ_BUF: usize = 64 * 1024;

/// The longest single message this client will store. A server that writes more
/// than this without a newline is not speaking the transport, and the honest
/// answer is to stop reading it rather than to grow until the app dies.
const MAX_MESSAGE: usize = 8 * 1024 * 1024;

/// How many lines of the server's `stderr` are kept for diagnosis. The transport
/// says `stderr` is for logging and must not be read as an error signal, which
/// is exactly why it is worth keeping beside a failure.
const STDERR_TAIL: usize = 40;

/// Everything needed to start one server process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSpec {
    /// Stable id, used to name the server in the UI and in the audit log.
    pub id: String,
    /// Program to run. The caller resolves it first: on Windows an npm-installed
    /// server is a `.cmd` shim, which the process API will not start directly.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Which half of the protocol's history the server speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Era {
    /// Version, identity and capabilities travel in every request.
    Modern,
    /// The session was opened with `initialize`.
    Legacy,
}

/// What the server said about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Peer {
    pub era: Era,
    /// The version actually in use, from the server's own answer.
    pub protocol_version: String,
    /// Self-reported and not verified, so it is for showing and for logs and
    /// never for deciding anything.
    pub name: String,
    pub version: String,
    pub instructions: Option<String>,
    /// The server's capabilities, as it sent them.
    pub capabilities: Value,
}

impl Peer {
    /// "name 1.2.3", or the id when the server named itself nothing.
    pub fn label(&self, fallback: &str) -> String {
        if self.name.trim().is_empty() {
            return fallback.to_string();
        }
        if self.version.trim().is_empty() {
            return self.name.clone();
        }
        format!("{} {}", self.name, self.version)
    }
}

/// One tool a server offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The tool's JSON Schema for its arguments, passed to it unchanged.
    pub input_schema: Value,
}

impl Tool {
    /// What to show in a list: the title when there is one, else the name.
    pub fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// What a tool call produced.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    /// The tool ran and reported a failure of its own. This is a result, not a
    /// protocol error: the server is working and is telling the caller what
    /// went wrong.
    pub is_error: bool,
    /// Text parts, in the order they arrived.
    pub text: Vec<String>,
    /// Content types this build does not render, named so that a person can see
    /// that something came back rather than being shown a silent gap.
    pub other: Vec<String>,
    /// The result as data, when the server sends it that way.
    pub structured: Option<Value>,
}

/// The outcome of a call.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Called {
    Done(ToolResult),
    /// The server needs input before it can finish. This build cannot collect
    /// it yet, so it reports what was asked for instead of pretending the call
    /// completed.
    NeedsInput {
        asked: Vec<String>,
    },
}

#[derive(Debug, Error)]
pub enum McpError {
    #[error("could not start {command}: {source}")]
    Spawn {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("talking to the server failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("a message could not be encoded: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("the server wrote something that is not one JSON-RPC message: {line}")]
    Protocol { line: String },
    #[error("the server's output could not be read: {0}")]
    Read(String),
    #[error("asked for request {wanted} and was answered with {got}")]
    OutOfStep { wanted: u64, got: u64 },
    #[error("{method} was refused by the server: {code} {message}")]
    Refused {
        method: String,
        code: i64,
        message: String,
    },
    #[error(
        "no version both sides speak: this build asked for {requested} and the server offered {offered:?}"
    )]
    NoSharedVersion {
        requested: String,
        offered: Vec<String>,
    },
    #[error("{what} did not answer within {seconds:.1}s")]
    Timeout { what: String, seconds: f64 },
    #[error("the server closed its output")]
    Closed,
}

/// How long a request may take, in each of the two phases that behave
/// differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// A single request once the server is running. A call that takes longer
    /// than this is a call that has stopped answering.
    pub call_timeout: Duration,
    /// The first request, and the handshake after it. A server installed through
    /// a package manager is fetched the first time it runs, so the first answer
    /// can take a while and that is not a fault.
    pub startup_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            call_timeout: DEFAULT_TIMEOUT,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
        }
    }
}

/// A running server.
pub struct Server {
    spec: ServerSpec,
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<FromServer>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    peer: Peer,
    next_id: u64,
    options: Options,
}

impl std::fmt::Debug for Server {
    /// A server holds a child process and a channel, so it is printed by what
    /// it is rather than by its handles.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Server")
            .field("id", &self.spec.id)
            .field("era", &self.peer.era)
            .field("protocol_version", &self.peer.protocol_version)
            .finish_non_exhaustive()
    }
}

enum FromServer {
    Line(String),
    /// The reader gave up: the bytes were not readable, or one message grew
    /// past what this client will store.
    Failed(String),
}

/// The longest a single request may take before the caller is told the server
/// has stopped answering.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the first answer may take. A server installed through a package
/// manager is fetched the first time it is run, so this is generous on purpose.
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a server is given to exit on its own before it is killed.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(1500);

impl Server {
    /// Start a server and settle which era it is in.
    pub fn start(spec: ServerSpec) -> Result<Self, McpError> {
        Self::start_with(spec, Options::default())
    }

    pub fn start_with(spec: ServerSpec, options: Options) -> Result<Self, McpError> {
        let mut command = Command::new(&spec.command);
        command.args(&spec.args);
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| McpError::Spawn {
                command: spec.command.clone(),
                source,
            })?;

        let stdin = child.stdin.take().ok_or_else(|| McpError::Spawn {
            command: spec.command.clone(),
            source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "no stdin"),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Spawn {
            command: spec.command.clone(),
            source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "no stdout"),
        })?;

        let (tx, lines) = mpsc::channel();
        thread::Builder::new()
            .name(format!("hp-mcp-read-{}", spec.id))
            .spawn(move || read_messages(stdout, tx))
            .map_err(McpError::Io)?;

        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(pipe) = child.stderr.take() {
            let tail = Arc::clone(&stderr);
            thread::Builder::new()
                .name(format!("hp-mcp-stderr-{}", spec.id))
                .spawn(move || read_stderr(pipe, tail))
                .map_err(McpError::Io)?;
        }

        let mut server = Self {
            spec,
            child,
            stdin: Some(stdin),
            lines,
            stderr,
            peer: Peer {
                era: Era::Modern,
                protocol_version: MODERN_VERSION.to_string(),
                name: String::new(),
                version: String::new(),
                instructions: None,
                capabilities: Value::Null,
            },
            next_id: 1,
            options,
        };

        server.peer = server.identify()?;
        Ok(server)
    }

    pub fn spec(&self) -> &ServerSpec {
        &self.spec
    }

    pub fn peer(&self) -> &Peer {
        &self.peer
    }

    /// The last lines the server wrote to `stderr`, oldest first.
    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr
            .lock()
            .map(|lines| lines.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Every tool the server offers, following its own pagination.
    pub fn tools(&mut self) -> Result<Vec<Tool>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        // A server that keeps handing back a cursor would otherwise loop here
        // forever; the cap is far past any real tool list.
        for _ in 0..50 {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let result = self.request("tools/list", params)?;
            for tool in result["tools"].as_array().cloned().unwrap_or_default() {
                tools.push(read_tool(&tool));
            }
            match result["nextCursor"].as_str() {
                Some(next) if !next.is_empty() => cursor = Some(next.to_string()),
                _ => return Ok(tools),
            }
        }
        Ok(tools)
    }

    /// Call one tool. A tool that fails is `Ok(Called::Done)` with `is_error`
    /// set: only the protocol itself earns an `Err`.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<Called, McpError> {
        let result = self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )?;

        if result["resultType"].as_str() == Some("input_required") {
            let asked = result["inputRequests"]
                .as_object()
                .map(|requests| requests.keys().cloned().collect())
                .unwrap_or_default();
            return Ok(Called::NeedsInput { asked });
        }

        let mut text = Vec::new();
        let mut other = Vec::new();
        for part in result["content"].as_array().cloned().unwrap_or_default() {
            match part["type"].as_str() {
                Some("text") => text.push(part["text"].as_str().unwrap_or_default().to_string()),
                Some(kind) => other.push(kind.to_string()),
                None => other.push("unreadable".to_string()),
            }
        }

        Ok(Called::Done(ToolResult {
            is_error: result["isError"].as_bool().unwrap_or(false),
            text,
            other,
            structured: match &result["structuredContent"] {
                Value::Null => None,
                value => Some(value.clone()),
            },
        }))
    }

    /// Close the input stream, wait for the server to exit, then kill it.
    ///
    /// Closing `stdin` is the only portable way to ask a server to stop, so it
    /// is tried first and given a moment to work.
    pub fn shutdown(mut self) {
        self.stdin = None;
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Probe the server and settle which era it is in.
    ///
    /// The stdio binding is explicit about the three outcomes, and the fallback
    /// must not be keyed to one error code: legacy servers answer an unknown
    /// request with whatever they like, or with nothing at all.
    fn identify(&mut self) -> Result<Peer, McpError> {
        match self.exchange(
            "server/discover",
            json!({}),
            Some(MODERN_VERSION.to_string()),
            self.options.startup_timeout,
        ) {
            Ok(result) => {
                let offered: Vec<String> = result["supportedVersions"]
                    .as_array()
                    .map(|versions| {
                        versions
                            .iter()
                            .filter_map(|version| version.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();

                // A server that does not list the version we just used is still
                // modern; keep talking, but say which version is in use.
                let version = if offered.iter().any(|v| v == MODERN_VERSION) {
                    MODERN_VERSION.to_string()
                } else {
                    offered.first().cloned().unwrap_or_default()
                };

                Ok(Peer {
                    era: Era::Modern,
                    protocol_version: version,
                    // The key holds a slash, so the pointer has to escape it.
                    name: read_string(&result, "/_meta/io.modelcontextprotocol~1serverInfo/name"),
                    version: read_string(
                        &result,
                        "/_meta/io.modelcontextprotocol~1serverInfo/version",
                    ),
                    instructions: result["instructions"].as_str().map(str::to_string),
                    capabilities: result["capabilities"].clone(),
                })
            }
            Err(Refusal::Version { offered }) => {
                let Some(version) = offered.iter().find(|v| v.as_str() == MODERN_VERSION) else {
                    return Err(McpError::NoSharedVersion {
                        requested: MODERN_VERSION.to_string(),
                        offered,
                    });
                };
                // This build speaks exactly one modern revision, so the only
                // way to reach here is a server that offered it.
                Ok(Peer {
                    era: Era::Modern,
                    protocol_version: version.clone(),
                    name: read_string(&Value::Null, "/name"),
                    version: String::new(),
                    instructions: None,
                    capabilities: Value::Null,
                })
            }
            // A JSON-RPC error other than a version one, or no answer at all,
            // is what the binding calls a legacy server: it did not recognise
            // the probe, or its era is too old to have heard of it.
            Err(Refusal::Error { .. }) | Err(Refusal::Lost(McpError::Timeout { .. })) => {
                self.open_legacy()
            }
            // Anything else is a broken transport rather than an older one -
            // junk on stdout, a reply to a request nobody made, a server that
            // has gone away. Falling back would talk more to a peer that has
            // already proved it is not speaking the protocol, and the failure
            // that came out would name the wrong thing.
            Err(Refusal::Lost(error)) => Err(error),
        }
    }

    /// The `initialize` handshake, for servers that predate per-request
    /// metadata. No `_meta` is sent afterwards: a server that supports both eras
    /// treats a request carrying it as modern, which would leave one session
    /// being served two ways.
    fn open_legacy(&mut self) -> Result<Peer, McpError> {
        let result = match self.exchange(
            "initialize",
            json!({
                "protocolVersion": LEGACY_VERSION,
                "capabilities": {},
                "clientInfo": { "name": CLIENT_NAME, "version": CLIENT_VERSION },
            }),
            None,
            self.options.startup_timeout,
        ) {
            Ok(result) => result,
            Err(Refusal::Error { code, message }) => {
                return Err(McpError::Refused {
                    method: "initialize".to_string(),
                    code,
                    message,
                })
            }
            Err(Refusal::Lost(error)) => return Err(error),
            Err(Refusal::Version { .. }) => {
                return Err(McpError::Refused {
                    method: "initialize".to_string(),
                    code: UNSUPPORTED_PROTOCOL_VERSION,
                    message: "the server does not speak the legacy handshake".to_string(),
                })
            }
        };

        self.notify("notifications/initialized", json!({}), None)?;

        Ok(Peer {
            era: Era::Legacy,
            protocol_version: result["protocolVersion"]
                .as_str()
                .unwrap_or(LEGACY_VERSION)
                .to_string(),
            name: read_string(&result, "/serverInfo/name"),
            version: read_string(&result, "/serverInfo/version"),
            instructions: result["instructions"].as_str().map(str::to_string),
            capabilities: result["capabilities"].clone(),
        })
    }

    /// Send a request and read the result.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let version = match self.peer.era {
            Era::Modern => Some(self.peer.protocol_version.clone()),
            Era::Legacy => None,
        };
        match self.exchange(method, params, version, self.options.call_timeout) {
            Ok(result) => Ok(result),
            Err(Refusal::Error { code, message }) => Err(McpError::Refused {
                method: method.to_string(),
                code,
                message,
            }),
            Err(Refusal::Version { offered }) => Err(McpError::NoSharedVersion {
                requested: self.peer.protocol_version.clone(),
                offered,
            }),
            Err(Refusal::Lost(error)) => Err(error),
        }
    }

    /// One request over the wire, with the answer still raw: the caller decides
    /// whether a version error, another error and no answer mean different
    /// things, which for the probe they do.
    fn exchange(
        &mut self,
        method: &str,
        params: Value,
        version: Option<String>,
        timeout: Duration,
    ) -> Result<Value, Refusal> {
        let id = self.next_id;
        self.next_id += 1;

        let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        let mut params = params;
        if let Some(version) = version {
            params["_meta"] = meta(&version);
        }
        message["params"] = params;

        self.write(&message).map_err(Refusal::Lost)?;

        // One deadline for the whole request, so a server that dribbles
        // notifications out cannot extend its own timeout.
        let deadline = Instant::now() + timeout;

        loop {
            match self.next_message(method, timeout, deadline)? {
                Incoming::Notification => continue,
                Incoming::Reply {
                    id: got,
                    result,
                    error,
                } => {
                    if got != id {
                        return Err(Refusal::Lost(McpError::OutOfStep { wanted: id, got }));
                    }
                    if let Some(error) = error {
                        let code = error["code"].as_i64().unwrap_or_default();
                        let message = error["message"].as_str().unwrap_or_default().to_string();
                        if code == UNSUPPORTED_PROTOCOL_VERSION {
                            let offered = error["data"]["supported"]
                                .as_array()
                                .map(|versions| {
                                    versions
                                        .iter()
                                        .filter_map(|v| v.as_str().map(str::to_string))
                                        .collect()
                                })
                                .unwrap_or_default();
                            return Err(Refusal::Version { offered });
                        }
                        return Err(Refusal::Error { code, message });
                    }
                    return Ok(result.unwrap_or(Value::Null));
                }
            }
        }
    }

    /// A notification: no `id`, so nothing is waiting for it.
    fn notify(
        &mut self,
        method: &str,
        params: Value,
        version: Option<String>,
    ) -> Result<(), McpError> {
        let mut message = json!({ "jsonrpc": "2.0", "method": method });
        let mut params = params;
        if let Some(version) = version {
            params["_meta"] = meta(&version);
        }
        message["params"] = params;
        self.write(&message)
    }

    fn write(&mut self, message: &Value) -> Result<(), McpError> {
        let stdin = self.stdin.as_mut().ok_or(McpError::Closed)?;
        let mut line = serde_json::to_string(message)?;
        line.push('\n');
        stdin.write_all(line.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }

    /// One message, waited for until `deadline`.
    fn next_message(
        &mut self,
        what: &str,
        timeout: Duration,
        deadline: Instant,
    ) -> Result<Incoming, Refusal> {
        let late = || McpError::Timeout {
            what: what.to_string(),
            seconds: timeout.as_secs_f64(),
        };

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Refusal::Lost(late()));
        }

        match self.lines.recv_timeout(remaining) {
            Ok(FromServer::Line(line)) => read_incoming(&line).map_err(Refusal::Lost),
            Ok(FromServer::Failed(complaint)) => Err(Refusal::Lost(McpError::Read(complaint))),
            Err(RecvTimeoutError::Timeout) => Err(Refusal::Lost(late())),
            Err(RecvTimeoutError::Disconnected) => Err(Refusal::Lost(McpError::Closed)),
        }
    }
}

impl Drop for Server {
    /// A safety net, not the way out: `shutdown` asks first. Dropping a server
    /// whose tool call is in flight has to end the process, or the app leaks one
    /// per mistake.
    fn drop(&mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Why a request did not produce a result, before the caller decides what it
/// means. The probe cares about the difference; a call does not.
#[derive(Debug)]
enum Refusal {
    Error { code: i64, message: String },
    Version { offered: Vec<String> },
    Lost(McpError),
}

#[derive(Debug)]
enum Incoming {
    /// A notification carries no id, so nothing is waiting for it. What it said
    /// is dropped on purpose: a server's progress notes are not results, and
    /// treating them as results is how a client pairs the wrong answer.
    Notification,
    Reply {
        id: u64,
        result: Option<Value>,
        error: Option<Value>,
    },
}

fn meta(version: &str) -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": version,
        "io.modelcontextprotocol/clientInfo": {
            "name": CLIENT_NAME,
            "version": CLIENT_VERSION,
        },
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

fn read_incoming(line: &str) -> Result<Incoming, McpError> {
    let message: Value = serde_json::from_str(line).map_err(|_| McpError::Protocol {
        line: shorten(line),
    })?;

    match message.get("id") {
        None | Some(Value::Null) => Ok(Incoming::Notification),
        Some(id) => {
            let id = id.as_u64().ok_or_else(|| McpError::Protocol {
                line: shorten(line),
            })?;
            Ok(Incoming::Reply {
                id,
                result: message.get("result").cloned(),
                error: message.get("error").cloned(),
            })
        }
    }
}

fn read_tool(tool: &Value) -> Tool {
    Tool {
        name: tool["name"].as_str().unwrap_or_default().to_string(),
        title: tool["title"].as_str().map(str::to_string),
        description: tool["description"].as_str().map(str::to_string),
        input_schema: match &tool["inputSchema"] {
            Value::Null => json!({ "type": "object" }),
            schema => schema.clone(),
        },
    }
}

/// Read one JSON Pointer out of a value, as a string.
fn read_string(value: &Value, pointer: &str) -> String {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Enough of a line to recognise it in an error, without repeating a megabyte.
fn shorten(line: &str) -> String {
    const KEEP: usize = 200;
    if line.chars().count() <= KEEP {
        return line.to_string();
    }
    let head: String = line.chars().take(KEEP).collect();
    format!("{head}…")
}

fn read_messages(mut stdout: impl Read, tx: Sender<FromServer>) {
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; READ_BUF];

    loop {
        match stdout.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => {
                for byte in &buf[..read] {
                    if *byte == b'\n' {
                        let line = String::from_utf8_lossy(&pending)
                            .trim_end_matches('\r')
                            .to_string();
                        pending.clear();
                        if line.trim().is_empty() {
                            continue;
                        }
                        if tx.send(FromServer::Line(line)).is_err() {
                            return;
                        }
                    } else if pending.len() >= MAX_MESSAGE {
                        let _ = tx.send(FromServer::Failed(format!(
                            "one message grew past {MAX_MESSAGE} bytes without a newline"
                        )));
                        return;
                    } else {
                        pending.push(*byte);
                    }
                }
            }
            Err(error) => {
                let _ = tx.send(FromServer::Failed(error.to_string()));
                return;
            }
        }
    }

    // A last line the server wrote without a trailing newline is still a
    // message: the transport says messages are newline-delimited, and being
    // strict about a missing final newline would lose a real answer.
    let tail = String::from_utf8_lossy(&pending).trim().to_string();
    if !tail.is_empty() {
        let _ = tx.send(FromServer::Line(tail));
    }
}

fn read_stderr(mut stderr: impl Read, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];

    loop {
        match stderr.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => {
                for byte in &buf[..read] {
                    if *byte == b'\n' {
                        let line = String::from_utf8_lossy(&pending)
                            .trim_end_matches('\r')
                            .to_string();
                        pending.clear();
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Ok(mut tail) = tail.lock() {
                            if tail.len() == STDERR_TAIL {
                                tail.pop_front();
                            }
                            tail.push_back(line);
                        }
                    } else {
                        pending.push(*byte);
                    }
                }
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_without_an_id_is_a_notification() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/message","params":{}}"#;
        assert!(matches!(read_incoming(line), Ok(Incoming::Notification)));
    }

    #[test]
    fn a_reply_carries_its_id_and_result() {
        let line = r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}"#;
        match read_incoming(line) {
            Ok(Incoming::Reply { id, result, error }) => {
                assert_eq!(id, 7);
                assert_eq!(result.expect("result")["ok"], Value::Bool(true));
                assert!(error.is_none());
            }
            other => panic!("expected a reply, got {:?}", other.is_ok()),
        }
    }

    #[test]
    fn a_line_that_is_not_json_is_a_protocol_error_naming_it() {
        let error = read_incoming("Starting server...").expect_err("not a message");
        assert!(
            matches!(&error, McpError::Protocol { line } if line == "Starting server..."),
            "{error}"
        );
    }

    #[test]
    fn a_long_line_is_shortened_in_the_error() {
        let junk = "x".repeat(5_000);
        let error = read_incoming(&junk).expect_err("not a message");
        match error {
            McpError::Protocol { line } => {
                assert!(line.chars().count() < 300, "kept {} chars", line.len());
                assert!(line.ends_with('…'));
            }
            other => panic!("expected a protocol error, got {other}"),
        }
    }

    #[test]
    fn a_tool_without_a_schema_still_gets_one() {
        let tool = read_tool(&json!({ "name": "ping" }));
        assert_eq!(tool.name, "ping");
        assert_eq!(tool.input_schema["type"], "object");
        assert_eq!(tool.label(), "ping");
    }

    #[test]
    fn a_server_that_named_itself_nothing_is_labelled_by_its_id() {
        let peer = Peer {
            era: Era::Modern,
            protocol_version: MODERN_VERSION.to_string(),
            name: String::new(),
            version: String::new(),
            instructions: None,
            capabilities: Value::Null,
        };
        assert_eq!(peer.label("weather"), "weather");
    }

    #[test]
    fn reading_lines_splits_on_newlines_and_keeps_a_last_one_without_them() {
        let (tx, rx) = mpsc::channel();
        read_messages(
            std::io::Cursor::new(b"one\r\ntwo\n\nlast without newline".to_vec()),
            tx,
        );

        let lines: Vec<String> = rx
            .iter()
            .map(|message| match message {
                FromServer::Line(line) => line,
                FromServer::Failed(complaint) => panic!("unexpected: {complaint}"),
            })
            .collect();
        assert_eq!(lines, ["one", "two", "last without newline"]);
    }

    #[test]
    fn a_message_that_never_ends_is_refused() {
        let (tx, rx) = mpsc::channel();
        let mut flood = vec![b'x'; MAX_MESSAGE + 1];
        flood.push(b'\n');
        read_messages(std::io::Cursor::new(flood), tx);

        match rx.recv().expect("a message") {
            FromServer::Failed(complaint) => assert!(
                complaint.contains("without a newline"),
                "unexpected complaint: {complaint}"
            ),
            FromServer::Line(line) => panic!("stored {} bytes", line.len()),
        }
    }
}
