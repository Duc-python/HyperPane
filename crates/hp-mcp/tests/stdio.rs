//! The client against a scripted peer.
//!
//! Every case here is one the specification names: a modern server, a legacy one
//! that refuses the probe, a modern one that speaks a version this build does
//! not, a server that writes junk to its own standard output, one that never
//! answers, one that answers the wrong request, and one that will not exit. The
//! server is this crate's own `fake-mcp-server`, so the tests need nothing
//! installed and no network.

use std::time::{Duration, Instant};

use hp_mcp::{Called, Era, McpError, Options, Server, ServerSpec, MODERN_VERSION};
use serde_json::json;

/// Short enough that a test which is supposed to time out does not hold the
/// suite up, long enough that a loaded machine still answers.
const BRIEF: Duration = Duration::from_millis(1500);

/// Both phases get the same short budget, so a test that is meant to time out
/// finishes quickly without changing what is being tested.
fn brief() -> Options {
    Options {
        call_timeout: BRIEF,
        startup_timeout: BRIEF,
    }
}

fn spec(mode: &str) -> ServerSpec {
    ServerSpec {
        id: format!("fake-{mode}"),
        command: env!("CARGO_BIN_EXE_fake-mcp-server").to_string(),
        args: if mode == "modern" {
            Vec::new()
        } else {
            vec![mode.to_string()]
        },
        env: Default::default(),
    }
}

fn start(mode: &str) -> Server {
    Server::start_with(spec(mode), brief())
        .unwrap_or_else(|error| panic!("starting the {mode} server: {error}"))
}

#[test]
fn a_modern_server_is_probed_and_its_tools_are_callable() {
    let mut server = start("modern");

    assert_eq!(server.peer().era, Era::Modern);
    assert_eq!(server.peer().protocol_version, MODERN_VERSION);
    assert_eq!(server.peer().label("fake"), "fake-mcp-server 0.1.0");
    assert_eq!(
        server.peer().instructions.as_deref(),
        Some("A scripted server, for tests.")
    );

    let tools = server.tools().expect("a tool list");
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, ["echo", "fails", "asks", "image"]);
    // A title is what a person should see, and the schema came through.
    assert_eq!(tools[0].label(), "Says what it was given");
    assert_eq!(tools[0].input_schema["type"], "object");

    let called = server
        .call("echo", json!({ "say": "hello" }))
        .expect("a result");
    match called {
        Called::Done(result) => {
            assert!(!result.is_error);
            assert_eq!(result.text, ["hello"]);
            assert!(result.other.is_empty());
        }
        other => panic!("expected a result, got {other:?}"),
    }
}

#[test]
fn a_legacy_server_is_reached_through_the_initialize_handshake() {
    let mut server = start("legacy");

    // The probe was refused, so this is the legacy path, and the version is the
    // one the server picked rather than the one that was offered.
    assert_eq!(server.peer().era, Era::Legacy);
    assert_eq!(server.peer().protocol_version, "2025-11-25");
    assert_eq!(server.peer().label("fake"), "fake-mcp-server 0.1.0");

    // Reaching the tool list at all means the handshake happened: this server
    // refuses every request until it has been initialised.
    let tools = server.tools().expect("a tool list");
    assert_eq!(tools.len(), 4);
}

#[test]
fn a_modern_server_with_no_shared_version_is_refused_with_both_lists() {
    let error = Server::start_with(spec("unsupported"), brief()).expect_err("no shared version");

    match error {
        McpError::NoSharedVersion { requested, offered } => {
            assert_eq!(requested, MODERN_VERSION);
            assert_eq!(offered, ["2025-11-25"]);
        }
        other => panic!("expected a version refusal, got {other}"),
    }
}

#[test]
fn junk_on_stdout_is_refused_with_the_line_the_server_wrote() {
    let error = Server::start_with(spec("noisy"), brief()).expect_err("not a message");

    match error {
        McpError::Protocol { line } => assert_eq!(line, "Starting server..."),
        other => panic!("expected a protocol error, got {other}"),
    }
}

#[test]
fn a_server_that_never_answers_times_out_instead_of_hanging() {
    let started = Instant::now();
    let error = Server::start_with(spec("silent"), brief()).expect_err("no answer");
    let waited = started.elapsed();

    // A silent server is legacy by the binding's rules, so it is asked twice:
    // once with the probe and once with the handshake.
    assert!(
        matches!(error, McpError::Timeout { .. }),
        "expected a timeout, got {error}"
    );
    assert!(
        waited < BRIEF * 4,
        "waited {waited:?}, which is past both timeouts"
    );
}

#[test]
fn a_reply_to_another_request_is_not_taken_for_ours() {
    let mut server = start("out-of-step");

    match server.tools().expect_err("the wrong answer") {
        McpError::OutOfStep { wanted, got } => assert_eq!(got, wanted + 100),
        other => panic!("expected an out-of-step error, got {other}"),
    }
}

#[test]
fn a_server_that_exits_is_reported_as_closed() {
    let mut server = start("exits");

    match server.tools().expect_err("no server left") {
        McpError::Closed => {}
        other => panic!("expected a closed server, got {other}"),
    }
}

#[test]
fn a_server_that_only_talks_about_itself_still_runs_out_of_time() {
    let mut server = start("dribble");
    let started = Instant::now();

    match server.tools().expect_err("no answer") {
        McpError::Timeout { what, .. } => assert_eq!(what, "tools/list"),
        other => panic!("expected a timeout, got {other}"),
    }

    // The notifications arrive throughout, and the budget is for the request
    // rather than for each message: waiting here would be waiting forever.
    let waited = started.elapsed();
    assert!(waited < BRIEF * 3, "waited {waited:?}");
}

#[test]
fn a_tool_that_fails_is_a_result_and_not_a_protocol_error() {
    let mut server = start("modern");

    match server.call("fails", json!({})).expect("a result") {
        Called::Done(result) => {
            assert!(result.is_error, "the tool said it failed");
            assert_eq!(result.text, ["the file is not there"]);
        }
        other => panic!("expected a result, got {other:?}"),
    }
}

#[test]
fn an_unknown_tool_is_a_refusal_and_the_server_stays_usable() {
    let mut server = start("refuses");

    match server.call("nope", json!({})).expect_err("a refusal") {
        McpError::Refused { method, code, .. } => {
            assert_eq!(method, "tools/call");
            assert_eq!(code, -32602);
        }
        other => panic!("expected a refusal, got {other}"),
    }

    // What the server said on stderr is read by its own thread, so it can be a
    // moment behind the reply on stdout. Waiting for it is the test being
    // honest about that rather than a race that fails once in a while.
    let mut tail = String::new();
    for _ in 0..20 {
        tail = server.stderr_tail().join("\n");
        if tail.contains("refusing to call nope") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        tail.contains("refusing to call nope"),
        "stderr tail was {tail:?}"
    );

    // A refusal is about one call, not about the server.
    assert!(server.tools().is_ok(), "the server is still there");
}

#[test]
fn a_tool_that_wants_input_says_which_fields_it_wants() {
    let mut server = start("modern");

    match server.call("asks", json!({})).expect("a result") {
        Called::NeedsInput { asked } => assert_eq!(asked, ["github_login"]),
        other => panic!("expected a request for input, got {other:?}"),
    }
}

#[test]
fn parts_this_build_does_not_render_are_named_rather_than_dropped() {
    let mut server = start("modern");

    match server.call("image", json!({})).expect("a result") {
        Called::Done(result) => {
            assert_eq!(result.text, ["here it is"]);
            assert_eq!(result.other, ["image"]);
            assert_eq!(result.structured, Some(json!({ "width": 2 })));
        }
        other => panic!("expected a result, got {other:?}"),
    }
}

#[test]
fn tools_are_followed_through_the_servers_own_pagination() {
    let mut server = start("paged");

    let tools = server.tools().expect("a tool list");
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, ["echo", "asks", "image"]);
}

#[test]
fn a_message_that_never_ends_is_refused_rather_than_stored() {
    let mut server = start("huge");

    // The probe is answered; the next request is met with a message that never
    // ends. The client has to give up on the server instead of growing.
    match server.tools().expect_err("an oversized message") {
        McpError::Read(complaint) => assert!(
            complaint.contains("without a newline"),
            "complaint was {complaint:?}"
        ),
        other => panic!("expected a read failure, got {other}"),
    }
}

#[test]
fn shutting_down_ends_a_server_that_ignores_a_closed_input() {
    let server = start("slow-exit");
    let started = Instant::now();

    // Closing the input stream is the portable way to ask, and this server
    // ignores it, so the client has to end the process itself.
    server.shutdown();
    let waited = started.elapsed();

    assert!(
        waited < Duration::from_secs(5),
        "shutdown took {waited:?}, which would hang the app on exit"
    );
}
