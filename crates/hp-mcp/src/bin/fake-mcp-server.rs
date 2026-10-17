//! A scripted MCP server, for testing the client and for trying it by hand.
//!
//! It is not part of the app. It exists so the client can be tested against a
//! peer that behaves exactly as the awkward cases require: a legacy server that
//! refuses the probe, a server that lists a version the client does not speak, a
//! server that writes junk to its own standard output, one that never answers,
//! and one that answers the wrong request.
//!
//! `fake-mcp-server <mode>`, defaulting to `modern`.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

/// Long enough to pass anything the client is willing to store.
const OVERSIZED: usize = 9 * 1024 * 1024;

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "modern".to_string());
    let stdin = std::io::stdin();

    if mode == "noisy" {
        // A server that prints a banner to the wrong stream. The transport says
        // stdout carries messages and nothing else.
        println!("Starting server...");
        let _ = std::io::stdout().flush();
    }

    let mut initialized = false;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { return };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        let method = message["method"].as_str().unwrap_or_default().to_string();
        let id = message["id"].clone();
        // Kept before the match arms move `id` into their replies.
        let wrong_id = json!(id.as_u64().unwrap_or(0) + 100);

        // A notification has no id and never gets an answer.
        if id.is_null() {
            if method == "notifications/initialized" {
                initialized = true;
            }
            continue;
        }

        if mode == "silent" {
            continue;
        }

        if mode == "dribble" && method != "server/discover" {
            // Notifications, forever, and never an answer. A client that starts
            // its timeout again for every message waits here for good.
            loop {
                let note = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/message",
                    "params": { "level": "info", "data": "still working" },
                });
                let mut out = std::io::stdout();
                if writeln!(out, "{note}").is_err() || out.flush().is_err() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        if mode == "huge" && method != "server/discover" {
            // One message that never ends: no newline at all.
            let _ = std::io::stdout().write_all(&vec![b'x'; OVERSIZED]);
            let _ = std::io::stdout().flush();
            return;
        }

        if mode == "exits" && method != "server/discover" {
            return;
        }

        if mode == "dies-on-call" && method == "tools/call" {
            // Something to be remembered by, then gone.
            eprintln!("goodbye");
            return;
        }

        let mut reply = match method.as_str() {
            "server/discover" => match mode.as_str() {
                "legacy" => error(id, -32601, "Method not found: server/discover"),
                "unsupported" => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32022,
                        "message": "Unsupported protocol version",
                        "data": { "supported": ["2025-11-25"], "requested": "2026-07-28" },
                    }
                }),
                _ => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "resultType": "complete",
                        "supportedVersions": ["2026-07-28"],
                        "capabilities": { "tools": { "listChanged": false } },
                        "_meta": {
                            "io.modelcontextprotocol/serverInfo": {
                                "name": "fake-mcp-server",
                                "version": "0.1.0"
                            }
                        },
                        "instructions": "A scripted server, for tests."
                    }
                }),
            },

            "initialize" => {
                if mode == "legacy" || mode == "modern" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": "2025-11-25",
                            "capabilities": { "tools": {} },
                            "serverInfo": { "name": "fake-mcp-server", "version": "0.1.0" },
                        }
                    })
                } else {
                    error(id, -32601, "Method not found: initialize")
                }
            }

            "tools/list" => {
                if mode == "legacy" && !initialized {
                    // A strict legacy server refuses work before the handshake.
                    error(id, -32002, "Not initialized")
                } else if mode == "paged" {
                    match message["params"]["cursor"].as_str() {
                        Some("page-2") => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "resultType": "complete",
                                "tools": [tool("asks", "Asks for input"), tool("image", "Sends a picture")],
                            }
                        }),
                        _ => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "resultType": "complete",
                                "tools": [tool("echo", "Says what it was given")],
                                "nextCursor": "page-2",
                            }
                        }),
                    }
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "resultType": "complete",
                            "tools": [
                                tool("echo", "Says what it was given"),
                                tool("fails", "Reports a failure of its own"),
                                tool("asks", "Asks for input"),
                                tool("image", "Sends a picture"),
                            ],
                        }
                    })
                }
            }

            "tools/call" => {
                let name = message["params"]["name"].as_str().unwrap_or_default();
                match name {
                    "echo" => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "resultType": "complete",
                            "content": [{
                                "type": "text",
                                "text": message["params"]["arguments"]["say"]
                                    .as_str()
                                    .unwrap_or("(nothing)"),
                            }],
                            "isError": false,
                        }
                    }),
                    "fails" => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "resultType": "complete",
                            "content": [{ "type": "text", "text": "the file is not there" }],
                            "isError": true,
                        }
                    }),
                    "asks" => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "resultType": "input_required",
                            "inputRequests": {
                                "github_login": { "method": "elicitation/create", "params": {} }
                            },
                            "requestState": "opaque",
                        }
                    }),
                    "image" => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "resultType": "complete",
                            "content": [
                                { "type": "text", "text": "here it is" },
                                { "type": "image", "data": "AAAA", "mimeType": "image/png" },
                            ],
                            "structuredContent": { "width": 2 },
                        }
                    }),
                    other => {
                        if mode == "refuses" {
                            eprintln!("fake-mcp-server: refusing to call {other}");
                        }
                        error(id, -32602, &format!("Unknown tool: {other}"))
                    }
                }
            }

            other => error(id, -32601, &format!("Method not found: {other}")),
        };

        if mode == "out-of-step" && method != "server/discover" {
            // The answer to a request nobody made: a client that pairs replies
            // by position rather than by id would take this for its own. The
            // probe is answered properly, so the server starts and then goes
            // wrong, which is the case worth catching.
            reply["id"] = wrong_id;
        }

        // A mark that this process answered its opening question, so a caller
        // can tell one start from two.
        if method == "server/discover" || method == "initialize" {
            eprintln!("ready");
        }

        let mut out = std::io::stdout();
        if serde_json::to_writer(&mut out, &reply).is_err() {
            return;
        }
        if writeln!(out).is_err() || out.flush().is_err() {
            return;
        }
    }

    // `slow-exit` never returns: the point is a server that ignores a closed
    // input stream, so the client has to be the one that ends it.
    if mode == "slow-exit" {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }
}

fn tool(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "title": description,
        "description": description,
        "inputSchema": { "type": "object", "properties": {}, "additionalProperties": true },
    })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}
