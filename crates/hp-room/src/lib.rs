//! A control room: who may see which pane, what they may do there, and who is
//! watching whom.
//!
//! This is the rules, not the wire. A room takes a message from a participant,
//! decides what it means, and answers with two things: effects for the host to
//! carry out on its own machine (writing into a pane), and events to send to
//! participants. Nothing here opens a socket or knows what a transport is, which
//! is what makes every rule testable without one.
//!
//! It is host-authoritative on purpose. A viewer is told what it may see and
//! what happened; it is never asked and never trusted. Every decision below is
//! made on the host, from state the host holds.
//!
//! Two properties are worth stating because they are easy to get wrong and they
//! are the ones tested hardest:
//!
//! - **Access filters the snapshot, not only the actions.** A participant who
//!   may see one pane is not told that the others exist, not even as an empty
//!   slot. An action refused is a small problem; a pane id leaked is a map of
//!   someone's screen.
//! - **The host is a participant**, with an id like anyone else, so the app has
//!   one code path for "send this event somewhere" and the host's own UI proves
//!   the same path the remote ones use.

use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod client;
pub mod host;
pub mod net;

pub use client::{Client, ClientError};
pub use host::{Host, HostError};

/// The version of this protocol. A peer that speaks another one is refused
/// rather than guessed at.
pub const PROTOCOL: u32 = 1;

/// The longest message this crate will encode. A viewer that sends more than
/// this is not speaking the protocol, and a host must not grow to hold it.
pub const MAX_MESSAGE: usize = 512 * 1024;

/// What a participant may do in a pane.
///
/// Ordered, so a `Control` grant satisfies a `View` check and not the other way
/// round. That ordering is the whole access model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    /// See the pane's output.
    View,
    /// See it and type into it.
    Control,
}

/// The host's id. Fixed, because there is exactly one host per room and its UI
/// needs a stable name for itself.
pub const HOST: &str = "host";

pub type ParticipantId = String;

/// One person in the room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub id: ParticipantId,
    /// What to show beside them. Self-reported, so the UI shows it as a claim.
    pub name: String,
    /// The pane they are looking at, when it is one they may see.
    pub focus: Option<String>,
    /// Whom they are following, if anyone.
    #[serde(default)]
    pub following: Option<ParticipantId>,
    pub host: bool,
}

/// One pane as one participant may see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneView {
    pub pane: String,
    pub access: Access,
}

/// The room as one participant may see it. Built per participant, never shared
/// between them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomSnapshot {
    pub id: String,
    pub participants: Vec<Participant>,
    /// Only the panes this participant may see.
    pub panes: Vec<PaneView>,
}

/// What a participant asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Message {
    /// "I am looking at this pane now." Presence, and what a follower follows.
    Focus { pane: Option<String> },
    /// "Show me whatever this person is looking at." Clears with `None`.
    Follow { who: Option<ParticipantId> },
    /// Type into a pane. The host checks the grant before it reaches the PTY.
    Input { pane: String, data: String },
    /// The host offers a pane to the room.
    Share { pane: String },
    /// The host stops offering it, and every grant on it goes with it.
    Unshare { pane: String },
    /// The host says what one person may do in one pane.
    Grant {
        pane: String,
        who: ParticipantId,
        access: Access,
    },
    /// The host takes that away.
    Revoke { pane: String, who: ParticipantId },
    /// "I am leaving."
    Bye,
}

/// What a participant is told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Event {
    Welcome {
        protocol: u32,
        you: ParticipantId,
        room: RoomSnapshot,
    },
    /// The room changed in a way this participant may see.
    Snapshot { room: RoomSnapshot },
    /// Someone's focus moved. `pane` is present only when the recipient may see
    /// that pane, so presence never leaks a pane id.
    Focus {
        who: ParticipantId,
        pane: Option<String>,
    },
    /// Output from a pane the recipient may see.
    Output { pane: String, data: String },
    /// The request was understood and refused, with the reason a person needs.
    Refused { because: String },
}

/// One event, for one participant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub to: ParticipantId,
    pub event: Event,
}

/// Something the host has to do on its own machine. Only ever produced after
/// the access check passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Write this to the pane's PTY.
    Write { pane: String, data: String },
}

/// What came of one message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub effects: Vec<Effect>,
    pub deliveries: Vec<Delivery>,
}

impl Applied {
    /// The events one participant was sent.
    pub fn to(&self, id: &str) -> Vec<&Event> {
        self.deliveries
            .iter()
            .filter(|delivery| delivery.to == id)
            .map(|delivery| &delivery.event)
            .collect()
    }

    fn send(&mut self, to: impl Into<String>, event: Event) {
        self.deliveries.push(Delivery {
            to: to.into(),
            event,
        });
    }
}

#[derive(Debug, Error)]
pub enum RoomError {
    #[error("this room already has a host")]
    AlreadyOpen,
    #[error("no participant called {0}")]
    Unknown(String),
}

/// The host's authoritative state.
#[derive(Debug)]
pub struct Room {
    id: String,
    participants: BTreeMap<ParticipantId, Participant>,
    /// Panes offered to the room. A pane nobody shared is not here at all.
    shared: BTreeSet<String>,
    /// Per pane, what each participant may do. The host owns its panes and is
    /// not in here.
    grants: BTreeMap<String, BTreeMap<ParticipantId, Access>>,
    next_participant: u64,
}

impl Room {
    /// Open a room with the host as its first participant.
    pub fn open(id: impl Into<String>, host_name: impl Into<String>) -> Self {
        let mut participants = BTreeMap::new();
        participants.insert(
            HOST.to_string(),
            Participant {
                id: HOST.to_string(),
                name: host_name.into(),
                focus: None,
                following: None,
                host: true,
            },
        );

        Self {
            id: id.into(),
            participants,
            shared: BTreeSet::new(),
            grants: BTreeMap::new(),
            next_participant: 1,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Let someone in. Their id is issued here, so a participant cannot choose
    /// one that belongs to somebody else.
    pub fn join(&mut self, name: impl Into<String>) -> (ParticipantId, Applied) {
        let id = format!("p{}", self.next_participant);
        self.next_participant += 1;

        self.participants.insert(
            id.clone(),
            Participant {
                id: id.clone(),
                name: name.into(),
                focus: None,
                following: None,
                host: false,
            },
        );

        let mut applied = Applied::default();
        applied.send(
            &id,
            Event::Welcome {
                protocol: PROTOCOL,
                you: id.clone(),
                room: self.snapshot_for(&id),
            },
        );
        // Everyone else sees the room change, not the newcomer's welcome.
        for other in self.participants.keys().filter(|key| *key != &id) {
            applied.send(
                other,
                Event::Snapshot {
                    room: self.snapshot_for(other),
                },
            );
        }

        (id, applied)
    }

    /// Someone left. Their grants stay: a grant is about a person, and the id is
    /// how the room knows which person.
    pub fn leave(&mut self, who: &str) -> Applied {
        let mut applied = Applied::default();
        if self.participants.remove(who).is_none() {
            return applied;
        }

        // Anyone following them has nobody to follow any more.
        for participant in self.participants.values_mut() {
            if participant.following.as_deref() == Some(who) {
                participant.following = None;
            }
        }

        for other in self.participants.keys().cloned().collect::<Vec<_>>() {
            applied.send(
                &other,
                Event::Snapshot {
                    room: self.snapshot_for(&other),
                },
            );
        }
        applied
    }

    /// Handle one message from one participant.
    pub fn apply(&mut self, from: &str, message: Message) -> Applied {
        if !self.participants.contains_key(from) {
            return Applied::default();
        }

        match message {
            Message::Focus { pane } => self.set_focus(from, pane),
            Message::Follow { who } => self.set_following(from, who),
            Message::Input { pane, data } => self.input(from, &pane, data),
            Message::Share { pane } => self.share(from, &pane),
            Message::Unshare { pane } => self.unshare(from, &pane),
            Message::Grant { pane, who, access } => self.grant(from, &pane, &who, access),
            Message::Revoke { pane, who } => self.revoke(from, &pane, &who),
            Message::Bye => self.leave(from),
        }
    }

    /// A pane produced output. Everyone who may see it is told, and the host is
    /// not: the host is where the output came from.
    pub fn mirror(&self, pane: &str, data: &str) -> Vec<Delivery> {
        if !self.shared.contains(pane) {
            return Vec::new();
        }

        self.grants
            .get(pane)
            .map(|granted| {
                granted
                    .keys()
                    .map(|to| Delivery {
                        to: to.clone(),
                        event: Event::Output {
                            pane: pane.to_string(),
                            data: data.to_string(),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The room as one participant may see it.
    pub fn snapshot_for(&self, who: &str) -> RoomSnapshot {
        let panes = if who == HOST {
            self.shared
                .iter()
                .map(|pane| PaneView {
                    pane: pane.clone(),
                    access: Access::Control,
                })
                .collect()
        } else {
            self.shared
                .iter()
                .filter_map(|pane| {
                    self.grants
                        .get(pane)
                        .and_then(|granted| granted.get(who))
                        .map(|access| PaneView {
                            pane: pane.clone(),
                            access: *access,
                        })
                })
                .collect()
        };

        RoomSnapshot {
            id: self.id.clone(),
            participants: self.participants.values().cloned().collect(),
            panes,
        }
    }

    /// What one participant may do in one pane, if anything.
    pub fn access(&self, who: &str, pane: &str) -> Option<Access> {
        if who == HOST {
            return self.shared.contains(pane).then_some(Access::Control);
        }
        if !self.shared.contains(pane) {
            return None;
        }
        self.grants
            .get(pane)
            .and_then(|granted| granted.get(who))
            .copied()
    }

    fn refuse(&self, to: &str, because: impl Into<String>) -> Applied {
        let mut applied = Applied::default();
        applied.send(
            to,
            Event::Refused {
                because: because.into(),
            },
        );
        applied
    }

    fn set_focus(&mut self, from: &str, pane: Option<String>) -> Applied {
        // A focus on a pane nobody may see is refused rather than recorded: it
        // would otherwise be a way to ask the host "does this pane exist?".
        if let Some(pane) = &pane {
            if self.access(from, pane).is_none() {
                return self.refuse(from, format!("no pane {pane} is shared with you"));
            }
        }

        if let Some(participant) = self.participants.get_mut(from) {
            participant.focus = pane.clone();
        }

        let mut applied = Applied::default();
        for other in self.participants.keys().cloned().collect::<Vec<_>>() {
            // The id is only sent to someone who may see the pane.
            let visible = pane
                .as_ref()
                .filter(|pane| self.access(&other, pane).is_some())
                .cloned();
            applied.send(
                &other,
                Event::Focus {
                    who: from.to_string(),
                    pane: visible,
                },
            );
        }
        applied
    }

    fn set_following(&mut self, from: &str, who: Option<ParticipantId>) -> Applied {
        if let Some(target) = &who {
            if target == from {
                return self.refuse(from, "you cannot follow yourself");
            }
            if !self.participants.contains_key(target) {
                return self.refuse(from, format!("nobody called {target} is here"));
            }
        }

        if let Some(participant) = self.participants.get_mut(from) {
            participant.following = who.clone();
        }

        let mut applied = Applied::default();
        for other in self.participants.keys().cloned().collect::<Vec<_>>() {
            applied.send(
                &other,
                Event::Snapshot {
                    room: self.snapshot_for(&other),
                },
            );
        }

        // The follower is told where the target is looking now, if they may see
        // it, so their view can move without waiting for the next move.
        if let Some(target) = &who {
            if let Some(pane) = self
                .participants
                .get(target)
                .and_then(|participant| participant.focus.clone())
            {
                if self.access(from, &pane).is_some() {
                    applied.send(
                        from,
                        Event::Focus {
                            who: target.clone(),
                            pane: Some(pane),
                        },
                    );
                }
            }
        }

        applied
    }

    fn input(&mut self, from: &str, pane: &str, data: String) -> Applied {
        match self.access(from, pane) {
            Some(Access::Control) => {
                let mut applied = Applied::default();
                applied.effects.push(Effect::Write {
                    pane: pane.to_string(),
                    data,
                });
                applied
            }
            Some(Access::View) => self.refuse(from, format!("{pane} is shared with you to view")),
            None => self.refuse(from, format!("no pane {pane} is shared with you")),
        }
    }

    fn share(&mut self, from: &str, pane: &str) -> Applied {
        if from != HOST {
            return self.refuse(from, "only the host shares a pane");
        }
        if !self.shared.insert(pane.to_string()) {
            return Applied::default();
        }

        let mut applied = Applied::default();
        for other in self.participants.keys().cloned().collect::<Vec<_>>() {
            applied.send(
                &other,
                Event::Snapshot {
                    room: self.snapshot_for(&other),
                },
            );
        }
        applied
    }

    fn unshare(&mut self, from: &str, pane: &str) -> Applied {
        if from != HOST {
            return self.refuse(from, "only the host stops sharing a pane");
        }
        if !self.shared.remove(pane) {
            return Applied::default();
        }
        // Grants belong to a share, not to a pane id: if it is shared again,
        // nobody gets back in without being granted again.
        self.grants.remove(pane);

        let mut applied = Applied::default();
        for other in self.participants.keys().cloned().collect::<Vec<_>>() {
            applied.send(
                &other,
                Event::Snapshot {
                    room: self.snapshot_for(&other),
                },
            );
        }
        applied
    }

    fn grant(&mut self, from: &str, pane: &str, who: &str, access: Access) -> Applied {
        if from != HOST {
            return self.refuse(from, "only the host decides what may be seen");
        }
        if !self.shared.contains(pane) {
            return self.refuse(from, format!("pane {pane} is not shared"));
        }
        if !self.participants.contains_key(who) {
            return self.refuse(from, format!("nobody called {who} is here"));
        }
        if who == HOST {
            return self.refuse(from, "the host already has its own panes");
        }

        self.grants
            .entry(pane.to_string())
            .or_default()
            .insert(who.to_string(), access);

        let mut applied = Applied::default();
        for other in self.participants.keys().cloned().collect::<Vec<_>>() {
            applied.send(
                &other,
                Event::Snapshot {
                    room: self.snapshot_for(&other),
                },
            );
        }
        applied
    }

    fn revoke(&mut self, from: &str, pane: &str, who: &str) -> Applied {
        if from != HOST {
            return self.refuse(from, "only the host decides what may be seen");
        }

        let removed = self
            .grants
            .get_mut(pane)
            .and_then(|granted| granted.remove(who))
            .is_some();
        if !removed {
            return Applied::default();
        }

        // Someone whose focus was a pane they can no longer see is looking at
        // nothing, and has to be told so.
        let lost_focus = self.participants.get_mut(who).and_then(|participant| {
            match participant.focus.as_deref() {
                Some(focus) if focus == pane => participant.focus.take(),
                _ => None,
            }
        });

        let mut applied = Applied::default();
        for other in self.participants.keys().cloned().collect::<Vec<_>>() {
            applied.send(
                &other,
                Event::Snapshot {
                    room: self.snapshot_for(&other),
                },
            );
        }
        if lost_focus.is_some() {
            for other in self.participants.keys().cloned().collect::<Vec<_>>() {
                applied.send(
                    &other,
                    Event::Focus {
                        who: who.to_string(),
                        pane: None,
                    },
                );
            }
        }
        applied
    }
}

/// Put a message on the wire: one line of JSON.
pub fn encode<T: Serialize>(message: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(message)
}

/// Read one message, refusing anything over the size a host will hold.
pub fn decode<T: DeserializeOwned>(line: &str) -> Result<T, WireError> {
    if line.len() > MAX_MESSAGE {
        return Err(WireError::TooLong {
            bytes: line.len(),
            limit: MAX_MESSAGE,
        });
    }
    serde_json::from_str(line).map_err(WireError::from)
}

#[derive(Debug, Error)]
pub enum WireError {
    #[error("a message of {bytes} bytes is more than the {limit} a host will read")]
    TooLong { bytes: usize, limit: usize },
    #[error("that is not a message this build knows: {0}")]
    Parse(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room with the host, one viewer of `secret`, one controller of it, and
    /// one participant who may see nothing.
    fn room() -> (Room, ParticipantId, ParticipantId, ParticipantId) {
        let mut room = Room::open("r1", "Duc");
        room.apply(
            HOST,
            Message::Share {
                pane: "secret".into(),
            },
        );

        let (viewer, _) = room.join("Viewer");
        let (controller, _) = room.join("Controller");
        let (outsider, _) = room.join("Outsider");

        room.apply(
            HOST,
            Message::Grant {
                pane: "secret".into(),
                who: viewer.clone(),
                access: Access::View,
            },
        );
        room.apply(
            HOST,
            Message::Grant {
                pane: "secret".into(),
                who: controller.clone(),
                access: Access::Control,
            },
        );

        (room, viewer, controller, outsider)
    }

    #[test]
    fn the_host_is_a_participant_with_an_id_like_anyone_else() {
        let room = Room::open("r1", "Duc");
        let snapshot = room.snapshot_for(HOST);

        assert_eq!(snapshot.participants.len(), 1);
        assert_eq!(snapshot.participants[0].id, HOST);
        assert!(snapshot.participants[0].host);
        assert!(snapshot.panes.is_empty());
    }

    #[test]
    fn joining_gets_a_welcome_and_tells_everyone_else() {
        let mut room = Room::open("r1", "Duc");
        let (id, applied) = room.join("Mai");

        assert!(id.starts_with('p'));
        match applied.to(&id).as_slice() {
            [Event::Welcome {
                protocol,
                you,
                room: snapshot,
            }] => {
                assert_eq!(*protocol, PROTOCOL);
                assert_eq!(you, &id);
                assert_eq!(snapshot.participants.len(), 2);
            }
            other => panic!("expected a welcome, got {other:?}"),
        }
        assert!(
            !applied
                .to(&id)
                .iter()
                .any(|event| matches!(event, Event::Snapshot { .. })),
            "the newcomer is welcomed, not sent a snapshot as well"
        );
        assert!(matches!(
            applied.to(HOST).as_slice(),
            [Event::Snapshot { .. }]
        ));
    }

    #[test]
    fn access_filters_the_snapshot_rather_than_only_the_actions() {
        let (room, viewer, controller, outsider) = room();

        // A pane shared with nobody is not in anyone else's snapshot, not even
        // as an empty slot: the id alone would be a map of the screen.
        assert_eq!(room.snapshot_for(&outsider).panes.len(), 0);
        assert_eq!(room.snapshot_for(&viewer).panes.len(), 1);
        assert_eq!(room.snapshot_for(&viewer).panes[0].access, Access::View);
        assert_eq!(
            room.snapshot_for(&controller).panes[0].access,
            Access::Control
        );

        // The host needs no grant to see its own panes.
        assert_eq!(room.snapshot_for(HOST).panes.len(), 1);
        assert_eq!(room.snapshot_for(HOST).panes[0].access, Access::Control);
    }

    #[test]
    fn only_the_host_can_share_or_decide_who_sees_what() {
        let (mut room, viewer, _, _) = room();

        for message in [
            Message::Share {
                pane: "other".into(),
            },
            Message::Unshare {
                pane: "secret".into(),
            },
            Message::Grant {
                pane: "secret".into(),
                who: viewer.clone(),
                access: Access::Control,
            },
            Message::Revoke {
                pane: "secret".into(),
                who: viewer.clone(),
            },
        ] {
            let applied = room.apply(&viewer, message);

            assert!(
                matches!(applied.to(&viewer).as_slice(), [Event::Refused { .. }]),
                "a viewer was allowed to change the share"
            );
            assert!(applied.effects.is_empty());
        }

        // And nothing moved: the viewer still has exactly what it had.
        assert_eq!(room.access(&viewer, "secret"), Some(Access::View));
        assert!(!room.snapshot_for(&viewer).panes.is_empty());
        assert!(room
            .snapshot_for(&viewer)
            .panes
            .iter()
            .all(|pane| pane.pane == "secret"));
    }

    #[test]
    fn typing_needs_control_and_viewing_does_not_get_it() {
        let (mut room, viewer, controller, outsider) = room();

        let refused = room.apply(
            &viewer,
            Message::Input {
                pane: "secret".into(),
                data: "rm -rf".into(),
            },
        );
        assert!(refused.effects.is_empty());
        assert!(
            matches!(refused.to(&viewer).as_slice(), [Event::Refused { because }] if because.contains("to view")),
            "{:?}",
            refused.to(&viewer)
        );

        let allowed = room.apply(
            &controller,
            Message::Input {
                pane: "secret".into(),
                data: "ls -la\n".into(),
            },
        );
        assert_eq!(
            allowed.effects,
            [Effect::Write {
                pane: "secret".into(),
                data: "ls -la\n".into()
            }]
        );
        assert!(allowed
            .deliveries
            .iter()
            .all(|delivery| !matches!(delivery.event, Event::Refused { .. })));

        // Somebody with no grant at all is refused too, and told nothing about
        // the pane beyond "no pane is shared with you".
        let nothing = room.apply(
            &outsider,
            Message::Input {
                pane: "secret".into(),
                data: "x".into(),
            },
        );
        assert!(nothing.effects.is_empty());
        assert!(matches!(
            nothing.to(&outsider).as_slice(),
            [Event::Refused { because }] if because.contains("no pane secret is shared")
        ));
    }

    #[test]
    fn the_host_types_into_its_own_panes_without_a_grant() {
        let (mut room, _, _, _) = room();

        let applied = room.apply(
            HOST,
            Message::Input {
                pane: "secret".into(),
                data: "echo hi\n".into(),
            },
        );
        assert_eq!(applied.effects.len(), 1);

        // But not into a pane it has not shared: that pane is not in the room.
        let unshared = room.apply(
            HOST,
            Message::Input {
                pane: "private".into(),
                data: "x".into(),
            },
        );
        assert!(unshared.effects.is_empty());
    }

    #[test]
    fn output_reaches_whoever_may_see_the_pane_and_not_the_host() {
        let (room, viewer, controller, outsider) = room();

        let deliveries = room.mirror("secret", "hello\n");
        let recipients: Vec<&str> = deliveries
            .iter()
            .map(|delivery| delivery.to.as_str())
            .collect();

        assert!(recipients.contains(&viewer.as_str()));
        assert!(recipients.contains(&controller.as_str()));
        assert!(!recipients.contains(&outsider.as_str()));
        assert!(
            !recipients.contains(&HOST),
            "the host is where it came from"
        );
        assert!(deliveries.iter().all(|delivery| matches!(
            &delivery.event,
            Event::Output { pane, data } if pane == "secret" && data == "hello\n"
        )));
    }

    #[test]
    fn output_from_a_pane_nobody_shares_goes_nowhere() {
        let (room, _, _, _) = room();
        assert!(room.mirror("private", "secret output").is_empty());
    }

    #[test]
    fn a_follower_is_told_where_the_other_person_is_looking() {
        let (mut room, viewer, controller, _) = room();

        // The controller looks at the pane both of them may see.
        room.apply(
            &controller,
            Message::Focus {
                pane: Some("secret".into()),
            },
        );

        let applied = room.apply(
            &viewer,
            Message::Follow {
                who: Some(controller.clone()),
            },
        );
        assert!(applied.to(&viewer).iter().any(|event| matches!(
            event,
            Event::Focus { who, pane } if who == &controller && pane.as_deref() == Some("secret")
        )));
        assert_eq!(
            room.snapshot_for(&viewer)
                .participants
                .iter()
                .find(|participant| participant.id == viewer)
                .and_then(|participant| participant.following.clone()),
            Some(controller)
        );
    }

    #[test]
    fn following_someone_who_is_not_here_is_refused() {
        let (mut room, viewer, _, _) = room();

        let applied = room.apply(
            &viewer,
            Message::Follow {
                who: Some("nobody".into()),
            },
        );
        assert!(matches!(
            applied.to(&viewer).as_slice(),
            [Event::Refused { because }] if because.contains("nobody called nobody")
        ));

        let self_follow = room.apply(
            &viewer,
            Message::Follow {
                who: Some(viewer.clone()),
            },
        );
        assert!(matches!(
            self_follow.to(&viewer).as_slice(),
            [Event::Refused { because }] if because.contains("follow yourself")
        ));
    }

    #[test]
    fn presence_never_names_a_pane_the_recipient_may_not_see() {
        let (mut room, viewer, controller, outsider) = room();

        // A second pane only the controller may see.
        room.apply(
            HOST,
            Message::Share {
                pane: "ledger".into(),
            },
        );
        room.apply(
            HOST,
            Message::Grant {
                pane: "ledger".into(),
                who: controller.clone(),
                access: Access::View,
            },
        );

        let applied = room.apply(
            &controller,
            Message::Focus {
                pane: Some("ledger".into()),
            },
        );

        // The viewer may see the controller, but not that pane.
        let shown = applied
            .to(&viewer)
            .into_iter()
            .find(|event| matches!(event, Event::Focus { .. }))
            .cloned();
        assert_eq!(
            shown,
            Some(Event::Focus {
                who: controller.clone(),
                pane: None
            })
        );

        // The controller is told the pane, because it is its own focus.
        assert!(applied.to(&controller).iter().any(|event| matches!(
            event,
            Event::Focus { pane, .. } if pane.as_deref() == Some("ledger")
        )));

        // The outsider is told the presence and nothing else.
        assert_eq!(
            applied.to(&outsider),
            [&Event::Focus {
                who: controller,
                pane: None
            }]
        );
    }

    #[test]
    fn focusing_a_pane_you_cannot_see_is_refused() {
        let (mut room, _, _, outsider) = room();

        let applied = room.apply(
            &outsider,
            Message::Focus {
                pane: Some("secret".into()),
            },
        );
        assert!(matches!(
            applied.to(&outsider).as_slice(),
            [Event::Refused { because }] if because.contains("no pane secret is shared with you")
        ));
    }

    #[test]
    fn unsharing_takes_every_grant_with_it() {
        let (mut room, viewer, _, _) = room();
        assert_eq!(room.access(&viewer, "secret"), Some(Access::View));

        room.apply(
            HOST,
            Message::Unshare {
                pane: "secret".into(),
            },
        );
        assert_eq!(room.access(&viewer, "secret"), None);

        // Sharing the same id again does not restore the old grant: access
        // belongs to a share, not to a string.
        room.apply(
            HOST,
            Message::Share {
                pane: "secret".into(),
            },
        );
        assert_eq!(room.access(&viewer, "secret"), None);
    }

    #[test]
    fn revoking_a_grant_clears_a_focus_that_depended_on_it() {
        let (mut room, viewer, _, _) = room();
        room.apply(
            &viewer,
            Message::Focus {
                pane: Some("secret".into()),
            },
        );

        let applied = room.apply(
            HOST,
            Message::Revoke {
                pane: "secret".into(),
                who: viewer.clone(),
            },
        );

        assert!(room
            .snapshot_for(&viewer)
            .participants
            .iter()
            .find(|participant| participant.id == viewer)
            .is_some_and(|participant| participant.focus.is_none()));
        // Everyone is told the focus went, including the person who had it.
        assert!(applied.deliveries.iter().any(|delivery| matches!(
            &delivery.event,
            Event::Focus { who, pane: None } if who == &viewer
        )));
    }

    #[test]
    fn leaving_clears_anyone_following_them_and_keeps_their_grants() {
        let (mut room, viewer, controller, _) = room();
        room.apply(
            &viewer,
            Message::Follow {
                who: Some(controller.clone()),
            },
        );

        room.leave(&controller);

        let snapshot = room.snapshot_for(&viewer);
        assert!(snapshot
            .participants
            .iter()
            .find(|participant| participant.id == viewer)
            .is_some_and(|participant| participant.following.is_none()));
        // A grant is about a person, so it survives the connection.
        assert_eq!(room.access(&controller, "secret"), Some(Access::Control));
    }

    #[test]
    fn a_message_from_someone_who_left_does_nothing() {
        let (mut room, viewer, _, _) = room();
        room.leave(&viewer);

        let applied = room.apply(
            &viewer,
            Message::Input {
                pane: "secret".into(),
                data: "x".into(),
            },
        );
        assert_eq!(applied, Applied::default());
    }

    #[test]
    fn a_message_round_trips_through_the_wire() {
        let message = Message::Grant {
            pane: "secret".into(),
            who: "p1".into(),
            access: Access::Control,
        };
        let line = encode(&message).expect("it encodes");
        assert_eq!(decode::<Message>(&line).expect("it decodes"), message);

        let event = Event::Output {
            pane: "secret".into(),
            data: "hello".into(),
        };
        let line = encode(&event).expect("it encodes");
        assert_eq!(decode::<Event>(&line).expect("it decodes"), event);
    }

    #[test]
    fn a_message_over_the_limit_is_refused_before_it_is_parsed() {
        let line = "x".repeat(MAX_MESSAGE + 1);
        let error = decode::<Message>(&line).expect_err("too long");
        assert!(matches!(error, WireError::TooLong { .. }), "{error}");
    }
}
