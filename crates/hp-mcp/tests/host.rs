//! The plugin host against the scripted server.
//!
//! These cover what the host adds on top of the client: nothing starts by
//! reading the file, a command nobody has read does not run, a server starts
//! once and is kept, and a failure is remembered as a reason rather than retried
//! as a wait.

use std::time::{Duration, Instant};

use hp_mcp::{Called, Host, PluginError, Trust, EMPTY_FILE};
use serde_json::json;

/// Both phases get the same short budget, so a case that is meant to fail fast
/// does not hold the suite up.
fn brief() -> hp_mcp::Options {
    hp_mcp::Options {
        call_timeout: Duration::from_millis(1500),
        startup_timeout: Duration::from_millis(1500),
    }
}

/// A file declaring one plugin, running the scripted server in `mode`.
///
/// The path goes through `serde_json` because it is written into JSON, where a
/// Windows path's backslashes have to be escaped.
fn host_for(id: &str, mode: &str) -> Host {
    host_declaring(id, &format!(r#"["{mode}"]"#))
}

fn host_declaring(id: &str, args: &str) -> Host {
    let text = format!(
        r#"{{"version":1,"servers":[{{"id":"{id}","command":{},"args":{args}}}]}}"#,
        serde_json::to_string(env!("CARGO_BIN_EXE_fake-mcp-server")).expect("a path")
    );
    Host::with_options(
        &text,
        |command, args| (command.to_string(), args.to_vec()),
        brief(),
    )
    .expect("the file parses")
}

/// The trust a person would have after reading this declaration and saying yes,
/// without any tools recorded yet.
fn read_and_approved(host: &Host, id: &str) -> Trust {
    let declaration = host.declaration(id).expect("a declaration");
    Trust::default().approve(id, &declaration, &[])
}

#[test]
fn a_declared_plugin_is_visible_before_anything_runs() {
    let host = host_for("scripted", "modern");
    let views = host.views(&Trust::default());

    assert_eq!(views.len(), 1);
    assert_eq!(views[0].id, "scripted");
    // No title in the file, so the id is what a person sees.
    assert_eq!(views[0].title, "scripted");
    assert_eq!(views[0].state, "declared");
    assert!(views[0].tools.is_empty(), "nothing has been asked yet");
    assert!(views[0].problem.is_none());
    assert!(!views[0].approved, "nothing has been read yet");
    assert!(views[0].approval_note.is_some());
}

#[test]
fn a_declaration_nobody_has_read_does_not_start() {
    let mut host = host_for("scripted", "modern");

    match host.open("scripted", &Trust::default()) {
        Err(PluginError::Unapproved { id, why }) => {
            assert_eq!(id, "scripted");
            assert!(why.contains("never run from here"), "{why}");
        }
        other => panic!("expected an unapproved plugin, got {other:?}"),
    }

    // And nothing was started to find that out.
    assert!(host.views(&Trust::default())[0].tools.is_empty());
}

#[test]
fn approving_and_starting_are_one_action_that_hands_back_the_tools() {
    let mut host = host_for("scripted", "modern");

    let (view, tools) = host.start_approved("scripted").expect("it starts");
    assert_eq!(view.state, "running");
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["echo", "fails", "asks", "image"]
    );

    // Recording the approval with those tools closes the loop: the declaration
    // is approved and the tool list is the one that was seen.
    let trust = read_and_approved(&host, "scripted");
    let recorded = trust.approve(
        "scripted",
        &host.declaration("scripted").expect("a declaration"),
        &tools,
    );

    let views = host.views(&recorded);
    assert!(views[0].approved);
    assert!(views[0].approval_note.is_none());
    assert!(
        views[0].tool_change_note.is_none(),
        "the list is the one approved"
    );

    // Asking again reuses the server, and the tools it offers are the same.
    let opened = host.open("scripted", &recorded).expect("still running");
    assert_eq!(opened.state, "running");
}

#[test]
fn opening_starts_the_plugin_and_keeps_what_it_offers() {
    let mut host = host_for("scripted", "modern");
    let trust = read_and_approved(&host, "scripted");

    let opened = host.open("scripted", &trust).expect("it starts");
    assert_eq!(opened.state, "running");
    assert_eq!(opened.peer.as_deref(), Some("fake-mcp-server 0.1.0"));
    assert_eq!(opened.era, Some("modern"));
    assert_eq!(opened.protocol_version.as_deref(), Some("2026-07-28"));
    assert_eq!(
        opened
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["echo", "fails", "asks", "image"]
    );
    assert_eq!(opened.tools[0].input_schema["type"], "object");

    // Asking again reuses the server: the mark the process prints when it
    // answers its opening question is there exactly once.
    let again = host.open("scripted", &trust).expect("still running");
    let ready = again
        .last_words
        .iter()
        .filter(|line| line.as_str() == "ready")
        .count();
    assert_eq!(ready, 1, "the server was started more than once");
}

#[test]
fn a_tool_list_that_moved_since_it_was_approved_is_visible() {
    let mut host = host_for("scripted", "modern");

    // Approved with nothing recorded: the shape of an approval given before the
    // server ever described itself.
    let trust = read_and_approved(&host, "scripted");
    let opened = host.open("scripted", &trust).expect("it starts");

    let note = opened.tool_change_note.expect("a note about the tools");
    assert!(note.contains("new tool(s)"), "{note}");
    assert!(note.contains("echo"), "{note}");
}

#[test]
fn a_changed_argument_is_refused_and_says_what_changed() {
    let before = host_declaring("scripted", r#"["modern"]"#);
    let trust = read_and_approved(&before, "scripted");

    // The same plugin, now with an argument added: the command line is not the
    // one anybody read.
    let mut after = host_declaring("scripted", r#"["modern", "--read-only"]"#);
    match after.open("scripted", &trust) {
        Err(PluginError::Unapproved { why, .. }) => {
            assert!(why.contains("argument was added"), "{why}");
            assert!(why.contains("--read-only"), "{why}");
        }
        other => panic!("expected a refusal naming the change, got {other:?}"),
    }
}

#[test]
fn a_tool_call_goes_through_the_host() {
    let mut host = host_for("scripted", "modern");
    let trust = read_and_approved(&host, "scripted");

    match host
        .call("scripted", "echo", json!({ "say": "hello" }), &trust)
        .expect("a result")
    {
        Called::Done(result) => {
            assert!(!result.is_error);
            assert_eq!(result.text, ["hello"]);
        }
        other => panic!("expected a result, got {other:?}"),
    }
}

#[test]
fn a_refusal_is_about_the_call_and_leaves_the_plugin_usable() {
    let mut host = host_for("scripted", "refuses");
    let trust = read_and_approved(&host, "scripted");

    match host.call("scripted", "nope", json!({}), &trust) {
        Err(PluginError::Server(hp_mcp::McpError::Refused { code, .. })) => {
            assert_eq!(code, -32602)
        }
        other => panic!("expected a refusal, got {other:?}"),
    }

    let view = host.open("scripted", &trust).expect("still running");
    assert_eq!(view.state, "running");
    assert!(view.problem.is_none());
}

#[test]
fn a_plugin_that_cannot_start_is_remembered_rather_than_retried() {
    // This server answers the probe and then goes away before it can list
    // anything, so the failure happens while opening.
    let mut host = host_for("scripted", "exits");
    let trust = read_and_approved(&host, "scripted");

    let first = host.open("scripted", &trust);
    assert!(
        matches!(first, Err(PluginError::Broken { .. })),
        "got {first:?}"
    );

    let view = &host.views(&trust)[0];
    assert_eq!(view.state, "broken");
    assert!(
        view.problem
            .as_deref()
            .is_some_and(|problem| problem.contains("closed its output")),
        "problem was {:?}",
        view.problem
    );

    // Asking again is an answer, not another wait.
    let started = Instant::now();
    let second = host.open("scripted", &trust);
    assert!(
        matches!(second, Err(PluginError::Broken { .. })),
        "got {second:?}"
    );
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the second attempt waited {:?}, so it tried to start again",
        started.elapsed()
    );
}

#[test]
fn a_server_that_dies_mid_call_is_marked_broken_with_what_it_said() {
    let mut host = host_for("scripted", "dies-on-call");
    let trust = read_and_approved(&host, "scripted");

    // Opening works: this server answers the probe and lists its tools.
    assert_eq!(
        host.open("scripted", &trust).expect("it starts").state,
        "running"
    );

    match host.call("scripted", "echo", json!({ "say": "hello" }), &trust) {
        Err(PluginError::Server(hp_mcp::McpError::Closed)) => {}
        other => panic!("expected a closed server, got {other:?}"),
    }

    let view = &host.views(&trust)[0];
    assert_eq!(view.state, "broken");
    assert!(
        view.problem
            .as_deref()
            .is_some_and(|problem| problem.contains("goodbye")),
        "the server's own last words should be in the reason, got {:?}",
        view.problem
    );
}

#[test]
fn a_plugin_switched_off_is_refused_and_still_listed() {
    let text = format!(
        r#"{{"version":1,"servers":[{{"id":"scripted","command":{},"enabled":false}}]}}"#,
        serde_json::to_string(env!("CARGO_BIN_EXE_fake-mcp-server")).expect("a path")
    );
    let mut host = Host::with_options(
        &text,
        |command, args| (command.to_string(), args.to_vec()),
        brief(),
    )
    .expect("the file parses");

    assert!(!host.views(&Trust::default())[0].enabled);
    match host.call("scripted", "echo", json!({}), &Trust::default()) {
        Err(PluginError::Disabled(id)) => assert_eq!(id, "scripted"),
        other => panic!("expected a disabled plugin, got {other:?}"),
    }
}

#[test]
fn shutting_down_ends_a_server_that_ignores_a_closed_input() {
    let mut host = host_for("scripted", "slow-exit");
    let trust = read_and_approved(&host, "scripted");
    assert_eq!(
        host.open("scripted", &trust).expect("it starts").state,
        "running"
    );

    let started = Instant::now();
    host.shutdown();

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "shutdown took {:?}, which would hang the app on exit",
        started.elapsed()
    );
}

#[test]
fn closing_one_plugin_leaves_the_view_of_it_intact() {
    let mut host = host_for("scripted", "modern");
    let trust = read_and_approved(&host, "scripted");
    host.open("scripted", &trust).expect("it starts");

    host.close("scripted");

    // What the file says is still there; what the process knew is not.
    let view = &host.views(&trust)[0];
    assert_eq!(view.state, "declared");
    assert!(view.tools.is_empty());
    assert_eq!(view.command, env!("CARGO_BIN_EXE_fake-mcp-server"));
}

#[test]
fn a_file_with_nothing_in_it_loads_and_hosts_nothing() {
    let host = Host::load(EMPTY_FILE, |command, args| {
        (command.to_string(), args.to_vec())
    })
    .expect("the shipped file parses");

    assert!(host.views(&Trust::default()).is_empty());
}
