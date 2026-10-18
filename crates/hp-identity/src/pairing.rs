//! Pairing two machines with a code.
//!
//! A person carries the code; everything else here is bytes. The exchange is
//! four messages and its whole job is to make two facts true at once:
//!
//! - Each side learns the other's **identity**, which is its public key.
//! - Each side learns that the other **knows the code**, so a machine in the
//!   middle that did not see the code cannot be either end.
//!
//! Both proofs are over one transcript - both ids, both names, both nonces, in a
//! fixed order - so a message cannot be reordered, a name cannot be swapped, and
//! a recording of one pairing cannot be replayed into another.
//!
//! The stretch is paid **once per machine**: the key is derived in `answer` and
//! kept for `accept`, because deriving it again would cost the same seconds for no
//! extra safety.
//!
//! This is not a PAKE: whoever records the exchange can still guess codes offline,
//! which is why `PairingCode` is long and stretched. What it is, honestly, is a
//! handshake only as strong as the code plus the key exchange, with both written
//! down where a reader can check the claim.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{nonce, DeviceId, Identity, PairingCode, PairingKey};

/// Separate this proof from any other use of the same key. A proof that fits two
/// purposes fits neither.
const CONTEXT: &[u8] = b"hyperpane/pair/1";

/// The host's opening message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub id: DeviceId,
    /// What the host calls itself. A claim, carried here so the guest can pin it
    /// under the same name the person will see.
    pub name: String,
    pub nonce: Vec<u8>,
}

/// The guest's reply, with proof that it knows the code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub id: DeviceId,
    pub name: String,
    pub nonce: Vec<u8>,
    /// The guest's proof over the transcript below.
    pub proof: Vec<u8>,
}

/// The host's proof back, so the guest is not the only one who was checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Confirmed {
    pub proof: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("the other machine does not know this code")]
    WrongCode,
    #[error("both ends of this pairing are the same machine")]
    SameMachine,
}

impl Offer {
    /// What the guest learns from the opening message: who is asking.
    pub fn host(&self) -> (&DeviceId, &str) {
        (&self.id, &self.name)
    }
}

impl Answer {
    /// What the host learns once the proof checked out: who replied.
    pub fn guest(&self) -> (&DeviceId, &str) {
        (&self.id, &self.name)
    }
}

/// Open a pairing: this machine, its name, and a nonce of its own.
pub fn offer(identity: &Identity, name: &str) -> Offer {
    Offer {
        id: identity.id(),
        name: name.trim().to_string(),
        nonce: nonce(32),
    }
}

/// Reply to an offer, proving the code.
///
/// The key comes back with the reply: this side derived it now, and `accept`
/// needs the same one rather than a second stretch of the same code.
pub fn answer(
    identity: &Identity,
    name: &str,
    code: &PairingCode,
    offer: &Offer,
) -> (Answer, PairingKey) {
    let mut reply = Answer {
        id: identity.id(),
        name: name.trim().to_string(),
        nonce: nonce(32),
        proof: Vec::new(),
    };

    let key = code.key(&salt(offer, &reply));
    reply.proof = key.proof(&transcript(offer, &reply)).to_vec();

    (reply, key)
}

/// Check the guest's proof and, if it holds, prove the code back.
pub fn confirm(
    code: &PairingCode,
    offer: &Offer,
    answer: &Answer,
) -> Result<Confirmed, PairingError> {
    if offer.id == answer.id {
        return Err(PairingError::SameMachine);
    }

    let key = code.key(&salt(offer, answer));
    let transcript = transcript(offer, answer);
    if !key.matches(&transcript, &answer.proof) {
        return Err(PairingError::WrongCode);
    }

    Ok(Confirmed {
        proof: key.proof(&transcript).to_vec(),
    })
}

/// Check the host's proof with the key this side already derived.
///
/// After this, both sides know the other knows the code, and both have the
/// other's id to pin.
pub fn accept(
    key: &PairingKey,
    offer: &Offer,
    answer: &Answer,
    confirmed: &Confirmed,
) -> Result<(), PairingError> {
    if offer.id == answer.id {
        return Err(PairingError::SameMachine);
    }

    if !key.matches(&transcript(offer, answer), &confirmed.proof) {
        return Err(PairingError::WrongCode);
    }

    Ok(())
}

/// Everything both sides saw, in one order, with a separator after each part so
/// that moving a boundary between two parts changes the bytes.
fn transcript(offer: &Offer, answer: &Answer) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(256);
    bytes.extend_from_slice(CONTEXT);

    for part in [
        offer.id.as_str().as_bytes(),
        offer.name.as_bytes(),
        offer.nonce.as_slice(),
        answer.id.as_str().as_bytes(),
        answer.name.as_bytes(),
        answer.nonce.as_slice(),
    ] {
        bytes.extend_from_slice(part);
        // A zero byte cannot appear in an id or a name, and a nonce is random
        // bytes, so the separator makes a boundary shift detectable.
        bytes.push(0);
    }

    bytes
}

/// Both nonces, in the order they were said.
fn salt(offer: &Offer, answer: &Answer) -> Vec<u8> {
    let mut salt = Vec::with_capacity(offer.nonce.len() + answer.nonce.len());
    salt.extend_from_slice(&offer.nonce);
    salt.extend_from_slice(&answer.nonce);
    salt
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// One successful pairing, made once and broken differently by each test.
    ///
    /// Sharing it is not only tidiness: every derivation pays the real stretch,
    /// and five tests each paying it twice is a suite that stops being run.
    fn paired() -> &'static (Offer, Answer, Confirmed, PairingCode, PairingKey) {
        static ONE: OnceLock<(Offer, Answer, Confirmed, PairingCode, PairingKey)> = OnceLock::new();

        ONE.get_or_init(|| {
            let host = Identity::generate();
            let guest = Identity::generate();
            let code = PairingCode::generate();

            let offer = super::offer(&host, "desk");
            let (answer, key) = super::answer(&guest, "laptop", &code, &offer);
            let confirmed = super::confirm(&code, &offer, &answer).expect("the code matches");

            (offer, answer, confirmed, code, key)
        })
    }

    #[test]
    fn two_machines_with_the_same_code_learn_who_each_other_are() {
        let (offer, answer, confirmed, _, key) = paired();

        accept(key, offer, answer, confirmed).expect("the host also knows the code");

        assert_eq!(offer.host().1, "desk");
        assert_eq!(answer.guest().1, "laptop");
        assert_ne!(offer.id, answer.id);
        // The ids are keys: what each side pins is what it can check later.
        assert_eq!(offer.id.as_str().len(), 64);
        assert_eq!(answer.id.as_str().len(), 64);
    }

    #[test]
    fn a_wrong_code_does_not_get_as_far_as_a_confirmation() {
        let host = Identity::generate();
        let guest = Identity::generate();
        let code = PairingCode::generate();
        let wrong = PairingCode::generate();

        let offer = super::offer(&host, "desk");
        let (answer, _) = super::answer(&guest, "laptop", &wrong, &offer);

        assert_eq!(
            super::confirm(&code, &offer, &answer),
            Err(PairingError::WrongCode)
        );
    }

    #[test]
    fn a_swapped_name_breaks_the_proof() {
        let (offer, answer, _, code, _) = paired();

        // The transcript covers both names, so a machine in the middle cannot
        // relabel either end and still pass.
        let relabelled = Answer {
            name: "server".to_string(),
            ..answer.clone()
        };

        assert_eq!(
            super::confirm(code, offer, &relabelled),
            Err(PairingError::WrongCode)
        );
    }

    #[test]
    fn a_reply_from_another_pairing_does_not_fit_this_one() {
        let host = Identity::generate();
        let guest = Identity::generate();
        let code = PairingCode::generate();

        let first = super::offer(&host, "desk");
        let (reply, _) = super::answer(&guest, "laptop", &code, &first);

        // The same guest, the same code, a different offer: the nonce is what
        // makes the old proof useless here.
        let second = super::offer(&host, "desk");
        assert_eq!(
            super::confirm(&code, &second, &reply),
            Err(PairingError::WrongCode)
        );
    }

    #[test]
    fn a_guest_does_not_accept_a_host_that_cannot_prove_the_code() {
        let (offer, answer, _, _, key) = paired();

        let forged = Confirmed {
            proof: vec![0u8; 32],
        };
        assert_eq!(
            accept(key, offer, answer, &forged),
            Err(PairingError::WrongCode)
        );
    }

    #[test]
    fn a_machine_cannot_pair_with_itself() {
        let machine = Identity::generate();
        let code = PairingCode::generate();

        let offer = super::offer(&machine, "desk");
        let (answer, _) = super::answer(&machine, "desk", &code, &offer);

        assert_eq!(
            super::confirm(&code, &offer, &answer),
            Err(PairingError::SameMachine)
        );
    }
}
