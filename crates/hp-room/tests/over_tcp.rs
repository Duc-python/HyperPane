//! Two participants over real sockets.
//!
//! The rules are tested in the crate without a network. These check the part
//! that only a socket can: a hello, a welcome, snapshots arriving as the room
//! changes, typing refused and then allowed, output reaching the right person,
//! and a room that closes.

use std::time::Duration;

use hp_room::{Access, Client, Effect, Event, Host, Message, PaneView, HOST};

const BRIEF: Duration = Duration::from_secs(5);

fn host() -> (Host, std::sync::mpsc::Receiver<Effect>) {
    Host::start("127.0.0.1:0", "Duc").expect("a room")
}

fn client(host: &Host, name: &str) -> Client {
    Client::join(&format!("127.0.0.1:{}", host.port()), name, BRIEF).expect("a client")
}

/// The next event that matches, skipping the snapshots the room sends on the way.
fn next_matching(client: &mut Client, want: impl Fn(&Event) -> bool) -> Event {
    for _ in 0..12 {
        match client.next_event().expect("an event") {
            Some(event) => {
                if want(&event) {
                    return event;
                }
            }
            None => panic!("the room closed while waiting for an event"),
        }
    }
    panic!("no matching event arrived");
}

#[test]
fn a_client_joins_and_both_ends_see_the_room() {
    let (host, _effects) = host();
    let client = client(&host, "Mai");

    assert!(host.port() > 0);

    let welcome = client.welcome();
    assert_eq!(welcome.participants.len(), 2);
    assert!(welcome
        .participants
        .iter()
        .any(|participant| participant.name == "Mai" && !participant.host));

    let host_side = host.snapshot().expect("the host's view");
    assert_eq!(host_side.participants.len(), 2);
    assert!(host_side
        .participants
        .iter()
        .any(|participant| participant.id == HOST && participant.host));
}

#[test]
fn a_pane_reaches_a_client_only_after_the_grant() {
    let (host, _effects) = host();
    let mut client = client(&host, "Mai");

    // Shared with nobody: the client is told the room changed, and the change
    // contains no panes - not even the one that was just shared.
    host.apply(Message::Share { pane: "p1".into() });
    match next_matching(&mut client, |event| matches!(event, Event::Snapshot { .. })) {
        Event::Snapshot { room } => {
            assert!(room.panes.is_empty(), "the grant had not been given yet")
        }
        _ => unreachable!(),
    }

    host.apply(Message::Grant {
        pane: "p1".into(),
        who: client.me().to_string(),
        access: Access::View,
    });
    match next_matching(&mut client, |event| matches!(event, Event::Snapshot { .. })) {
        Event::Snapshot { room } => assert_eq!(
            room.panes,
            [PaneView {
                pane: "p1".into(),
                access: Access::View
            }]
        ),
        _ => unreachable!(),
    }
}

#[test]
fn typing_is_refused_to_a_viewer_and_forwarded_to_a_controller() {
    let (host, effects) = host();
    let mut client = client(&host, "Mai");
    let me = client.me().to_string();

    host.apply(Message::Share { pane: "p1".into() });
    host.apply(Message::Grant {
        pane: "p1".into(),
        who: me.clone(),
        access: Access::View,
    });

    client
        .send(Message::Input {
            pane: "p1".into(),
            data: "rm -rf /\n".into(),
        })
        .expect("it sends");

    match next_matching(&mut client, |event| matches!(event, Event::Refused { .. })) {
        Event::Refused { because } => assert!(because.contains("to view"), "{because}"),
        _ => unreachable!(),
    }
    assert!(
        effects.try_recv().is_err(),
        "a viewer's keystrokes reached the pane"
    );

    host.apply(Message::Grant {
        pane: "p1".into(),
        who: me,
        access: Access::Control,
    });

    client
        .send(Message::Input {
            pane: "p1".into(),
            data: "ls\n".into(),
        })
        .expect("it sends");

    match effects.recv_timeout(BRIEF) {
        Ok(Effect::Write { pane, data }) => {
            assert_eq!(pane, "p1");
            assert_eq!(data, "ls\n");
        }
        other => panic!("expected a write, got {other:?}"),
    }
}

#[test]
fn output_reaches_whoever_may_see_the_pane() {
    let (host, _effects) = host();
    let mut client = client(&host, "Mai");

    host.apply(Message::Share { pane: "p1".into() });
    host.apply(Message::Grant {
        pane: "p1".into(),
        who: client.me().to_string(),
        access: Access::View,
    });

    host.mirror("p1", "hello\n");

    match next_matching(&mut client, |event| matches!(event, Event::Output { .. })) {
        Event::Output { pane, data } => {
            assert_eq!(pane, "p1");
            assert_eq!(data, "hello\n");
        }
        _ => unreachable!(),
    }
}

#[test]
fn a_client_without_a_grant_is_told_nothing_about_the_pane() {
    let (host, _effects) = host();
    let viewer = client(&host, "Mai");
    let mut outsider = client(&host, "Nam");

    host.apply(Message::Share { pane: "p1".into() });
    host.apply(Message::Grant {
        pane: "p1".into(),
        who: viewer.me().to_string(),
        access: Access::View,
    });

    host.mirror("p1", "secret\n");

    // The outsider asks about the pane. If any output had leaked, it would
    // arrive before this refusal, and the assertion below would see it.
    outsider
        .send(Message::Focus {
            pane: Some("p1".into()),
        })
        .expect("it sends");

    match next_matching(&mut outsider, |event| {
        matches!(event, Event::Refused { .. } | Event::Output { .. })
    }) {
        Event::Refused { because } => {
            assert!(because.contains("no pane p1 is shared"), "{because}")
        }
        other => panic!("a pane it may not see reached the outsider: {other:?}"),
    }
}

#[test]
fn a_client_is_told_when_someone_focuses_a_pane_it_may_see() {
    let (host, _effects) = host();
    let mut client = client(&host, "Mai");

    host.apply(Message::Share { pane: "p1".into() });
    host.apply(Message::Grant {
        pane: "p1".into(),
        who: client.me().to_string(),
        access: Access::View,
    });

    // The host is the one looking at it.
    host.apply(Message::Focus {
        pane: Some("p1".into()),
    });

    match next_matching(&mut client, |event| matches!(event, Event::Focus { .. })) {
        Event::Focus { who, pane } => {
            assert_eq!(who, HOST);
            assert_eq!(pane.as_deref(), Some("p1"));
        }
        _ => unreachable!(),
    }
}

#[test]
fn stopping_the_host_ends_the_clients() {
    let (host, _effects) = host();
    let mut client = client(&host, "Mai");

    host.stop();

    let end = client.next_event();
    assert!(matches!(end, Ok(None) | Err(_)), "got {end:?}");
}
