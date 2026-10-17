//! Ask an MCP server what it is and what it offers.
//!
//! `mcp-probe <command> [args...] [--call <tool> [json]]`
//!
//! The point is to be able to see a server's own answer before anything in the
//! app depends on it: which era it speaks, which version, what it calls itself,
//! and which tools it offers. `--call` goes one step further and calls one, so
//! the whole path can be checked against a real server rather than only against
//! this crate's own fake one.

use std::process::ExitCode;

use hp_mcp::{Called, Era, Server, ServerSpec};
use serde_json::Value;

fn main() -> ExitCode {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let call_at = all.iter().position(|arg| arg == "--call");
    let (command_args, call) = match call_at {
        Some(at) => (
            &all[..at],
            Some((
                all.get(at + 1).cloned().unwrap_or_default(),
                all.get(at + 2).cloned(),
            )),
        ),
        None => (&all[..], None),
    };

    let Some(command) = command_args.first().cloned() else {
        eprintln!("usage: mcp-probe <command> [args...] [--call <tool> [json]]");
        return ExitCode::from(2);
    };
    let args: Vec<String> = command_args.iter().skip(1).cloned().collect();

    let spec = ServerSpec {
        id: "probe".to_string(),
        command,
        args,
        env: Default::default(),
    };
    let label = format!("{} {}", spec.command, spec.args.join(" "));

    let mut server = match Server::start(spec) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("{label}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let peer = server.peer();
    let era = match peer.era {
        Era::Modern => "modern",
        Era::Legacy => "legacy",
    };
    println!("{label}");
    println!("  era:      {era} ({})", peer.protocol_version);
    println!("  calls itself: {}", peer.label("(nothing)"));
    if let Some(instructions) = &peer.instructions {
        println!("  instructions: {instructions}");
    }
    println!("  capabilities: {}", peer.capabilities);

    match server.tools() {
        Ok(tools) => {
            println!("  tools: {}", tools.len());
            for tool in tools {
                let description = tool.description.unwrap_or_default();
                let first = description.lines().next().unwrap_or_default();
                println!("    {}  {first}", tool.name);
            }
        }
        Err(error) => {
            println!("  tools: {error}");
            let tail = server.stderr_tail();
            if !tail.is_empty() {
                println!("  the server said, on stderr:");
                for line in tail {
                    println!("    {line}");
                }
            }
            server.shutdown();
            return ExitCode::FAILURE;
        }
    }

    if let Some((tool, arguments)) = call {
        let arguments: Value = match arguments {
            Some(json) => match serde_json::from_str(&json) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("--call arguments are not JSON: {error}");
                    server.shutdown();
                    return ExitCode::from(2);
                }
            },
            None => Value::Object(Default::default()),
        };

        println!("  call {tool} {arguments}");
        match server.call(&tool, arguments) {
            Ok(Called::Done(result)) => {
                println!("    error: {}", result.is_error);
                for part in &result.text {
                    println!("    text: {part}");
                }
                for part in &result.other {
                    println!("    part this build does not render: {part}");
                }
                if let Some(structured) = &result.structured {
                    println!("    structured: {structured}");
                }
            }
            Ok(Called::NeedsInput { asked }) => {
                println!("    asked for input first: {}", asked.join(", "));
            }
            Err(error) => {
                println!("    {error}");
                server.shutdown();
                return ExitCode::FAILURE;
            }
        }
    }

    server.shutdown();
    ExitCode::SUCCESS
}
