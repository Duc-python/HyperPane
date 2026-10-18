//! The machines this one has paired with.
//!
//! An identity names a device; this is the list of names it has met, and the
//! reason a reconnect needs no code. It is tiny and it is a file on purpose: a
//! person can read who is trusted, and can take one away without a UI.
//!
//! What "trust" means here is narrow and worth stating: a pinned id is one this
//! machine agreed a code with, and a later handshake is believed because it is
//! signed by that key. It says nothing about the software on the other end, which
//! is the same thing the MCP guide says about provenance - a key tells you who,
//! never what.
//!
//! The one judgement this file makes is a **name clash**: an id nobody has seen
//! arriving with a name that is already pinned to a different id. That is either a
//! machine that was reinstalled or somebody borrowing a name, and a person is the
//! only one who can tell which. It is reported rather than refused, because
//! refusing would make a reinstall impossible; and a name is a label, not an
//! identity, so the old entry is left alone.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::DeviceId;

/// The version of this file this build writes.
pub const PEERS_VERSION: u32 = 1;

/// The starting point: nobody is trusted, which is why nothing reconnects.
pub const EMPTY_PEERS: &str = r#"{
  "version": 1,
  "peers": {}
}
"#;

#[derive(Debug, Error)]
pub enum PeersError {
    #[error("the peers file is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("the peers file is version {found}; this build understands version {expected}")]
    Version { found: u32, expected: u32 },
}

/// One machine this one has paired with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub id: DeviceId,
    /// What the machine called itself when it was paired. A claim, kept as one.
    pub name: String,
    pub paired_at_ms: u128,
    #[serde(default)]
    pub last_seen_ms: u128,
}

impl Peer {
    /// The first eight characters of the id, for showing beside the name.
    pub fn short(&self) -> &str {
        self.id.short()
    }
}

/// What remembering a pairing did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remembered {
    /// This machine had not been paired with before.
    Added,
    /// It had, and its name and last-seen are what changed.
    Seen,
    /// It had not been paired with, and its name is already pinned to other ids.
    /// The pairing still happened: the caller decides what to say about it.
    Clashed(Vec<DeviceId>),
}

/// Every machine this one has paired with, by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Peers {
    pub version: u32,
    #[serde(default)]
    pub peers: BTreeMap<String, Peer>,
}

impl Default for Peers {
    fn default() -> Self {
        Self {
            version: PEERS_VERSION,
            peers: BTreeMap::new(),
        }
    }
}

impl Peers {
    pub fn parse(text: &str) -> Result<Self, PeersError> {
        let peers: Self = serde_json::from_str(text)?;
        if peers.version != PEERS_VERSION {
            return Err(PeersError::Version {
                found: peers.version,
                expected: PEERS_VERSION,
            });
        }
        Ok(peers)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| EMPTY_PEERS.to_string())
    }

    /// Whether a handshake from this id is to be believed without a code.
    pub fn is_trusted(&self, id: &DeviceId) -> bool {
        self.peers.contains_key(id.as_str())
    }

    pub fn get(&self, id: &DeviceId) -> Option<&Peer> {
        self.peers.get(id.as_str())
    }

    /// Everyone, in id order.
    pub fn list(&self) -> Vec<&Peer> {
        self.peers.values().collect()
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Record that this machine agreed a code with that one.
    ///
    /// `now_ms` is the caller's clock: this file does not read the time, so a
    /// test can say what it means and a file cannot disagree with itself.
    pub fn remember(&mut self, id: &DeviceId, name: &str, now_ms: u128) -> Remembered {
        let name = name.trim();
        let clashes: Vec<DeviceId> = self
            .peers
            .values()
            .filter(|peer| peer.name == name && &peer.id != id)
            .map(|peer| peer.id.clone())
            .collect();

        match self.peers.get_mut(id.as_str()) {
            Some(peer) => {
                // A machine may rename itself; the id is what is trusted, so the
                // label follows it rather than the other way round.
                peer.name = name.to_string();
                peer.last_seen_ms = now_ms;
                Remembered::Seen
            }
            None => {
                self.peers.insert(
                    id.as_str().to_string(),
                    Peer {
                        id: id.clone(),
                        name: name.to_string(),
                        paired_at_ms: now_ms,
                        last_seen_ms: now_ms,
                    },
                );
                if clashes.is_empty() {
                    Remembered::Added
                } else {
                    Remembered::Clashed(clashes)
                }
            }
        }
    }

    /// Take a machine out. Until it is paired again, its handshakes are refused.
    pub fn forget(&mut self, id: &DeviceId) -> bool {
        self.peers.remove(id.as_str()).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Identity, PairingCode};

    fn id_of(identity: &Identity) -> DeviceId {
        identity.id()
    }

    #[test]
    fn nothing_is_trusted_until_it_is_paired() {
        let peers = Peers::default();
        let identity = Identity::generate();

        assert!(!peers.is_trusted(&id_of(&identity)));
        assert!(peers.is_empty());
    }

    #[test]
    fn a_paired_machine_is_trusted_and_keeps_its_name() {
        let mut peers = Peers::default();
        let identity = Identity::generate();
        let id = id_of(&identity);

        assert_eq!(peers.remember(&id, " desk ", 1_000), Remembered::Added);
        assert!(peers.is_trusted(&id));
        assert_eq!(peers.get(&id).expect("it is there").name, "desk");
        // The first sighting and the last are the same moment.
        assert_eq!(peers.get(&id).expect("it is there").paired_at_ms, 1_000);
        assert_eq!(peers.get(&id).expect("it is there").last_seen_ms, 1_000);
    }

    #[test]
    fn pairing_again_updates_the_label_and_the_last_sighting() {
        let mut peers = Peers::default();
        let identity = Identity::generate();
        let id = id_of(&identity);

        peers.remember(&id, "desk", 1_000);
        assert_eq!(peers.remember(&id, "the desk", 2_000), Remembered::Seen);

        let peer = peers.get(&id).expect("it is there");
        assert_eq!(peer.name, "the desk");
        assert_eq!(peer.paired_at_ms, 1_000, "when it was paired does not move");
        assert_eq!(peer.last_seen_ms, 2_000);
        assert_eq!(peers.len(), 1);
    }

    #[test]
    fn a_new_machine_borrowing_a_known_name_is_reported_rather_than_refused() {
        let mut peers = Peers::default();
        let first = Identity::generate();
        let second = Identity::generate();

        peers.remember(&id_of(&first), "laptop", 1_000);
        let remembered = peers.remember(&id_of(&second), "laptop", 2_000);

        // A reinstalled machine must be pairable, so the pairing happens; the
        // clash is what a person is told about, because only they know which of
        // the two is the laptop they meant.
        assert_eq!(remembered, Remembered::Clashed(vec![id_of(&first)]));
        assert_eq!(peers.len(), 2);
        assert!(peers.is_trusted(&id_of(&second)));
        assert!(peers.is_trusted(&id_of(&first)));
    }

    #[test]
    fn forgetting_takes_the_trust_with_it() {
        let mut peers = Peers::default();
        let identity = Identity::generate();
        let id = id_of(&identity);

        peers.remember(&id, "desk", 1_000);
        assert!(peers.forget(&id));
        assert!(!peers.is_trusted(&id));
        // Forgetting twice is not an error, it is a no-op.
        assert!(!peers.forget(&id));
    }

    #[test]
    fn the_file_round_trips_and_refuses_a_version_it_does_not_know() {
        let mut peers = Peers::default();
        let identity = Identity::generate();
        peers.remember(&id_of(&identity), "desk", 1_000);

        let text = peers.to_json();
        assert_eq!(Peers::parse(&text).expect("it parses"), peers);

        let error = Peers::parse(r#"{"version": 9, "peers": {}}"#).expect_err("a later version");
        assert!(matches!(error, PeersError::Version { found: 9, .. }));
    }

    #[test]
    fn the_shipped_empty_file_is_valid_and_trusts_nobody() {
        let peers = Peers::parse(EMPTY_PEERS).expect("it parses");
        assert!(peers.is_empty());
        assert!(!peers.is_trusted(&Identity::generate().id()));
    }

    #[test]
    fn a_pinned_id_is_what_a_later_handshake_is_checked_against() {
        // The point of the file: no code is needed twice. A machine proves who
        // it is by signing a nonce, and the pinned id is what checks it.
        let mut peers = Peers::default();
        let host = Identity::generate();
        let guest = Identity::generate();
        peers.remember(&id_of(&guest), "desk", 1_000);

        let challenge = crate::nonce(32);
        let signature = guest.sign(&challenge);

        let pinned = peers.get(&id_of(&guest)).expect("it is pinned").id.clone();
        assert!(pinned.verify(&challenge, &signature));
        assert!(!host.id().verify(&challenge, &signature));

        // And an id nobody pinned has nothing to check against.
        let stranger = Identity::generate();
        assert!(!peers.is_trusted(&stranger.id()));
        let _ = PairingCode::generate();
    }
}
