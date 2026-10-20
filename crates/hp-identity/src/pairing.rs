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
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;
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

/// How long either end waits for the other before calling the exchange stuck.
///
/// Generous on purpose. The guest stretches a sixty-bit code with 600,000 rounds
/// of PBKDF2 by design - that is seconds of work - so a short wait would turn a
/// working pairing into a failure on a busy machine. It is still bounded, so a
/// real mistake fails instead of hanging.
pub const PATIENCE: Duration = Duration::from_secs(30);

/// Who a pairing turned out to be with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    pub id: DeviceId,
    pub name: String,
}

#[derive(Debug, Error)]
pub enum WireError {
    #[error("networking: {0}")]
    Network(#[from] std::io::Error),
    #[error("the other end sent something this build cannot read: {0}")]
    Unreadable(#[from] serde_json::Error),
    #[error("the other end left before the pairing finished")]
    HungUp,
    #[error(transparent)]
    Pairing(#[from] PairingError),
}

/// One message, one line.
///
/// JSON lines rather than a length prefix because the exchange is four messages
/// between two people's machines: a reader can watch it with `nc` and check the
/// claim in the module doc without a decoder.
fn send<T: Serialize>(stream: &mut impl Write, message: &T) -> Result<(), WireError> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn receive<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> Result<T, WireError> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(WireError::HungUp);
    }
    Ok(serde_json::from_str(line.trim_end())?)
}

/// Listen, and pair with the first guest that arrives.
///
/// Returns who it was. The caller decides what to do with that - the app pins it
/// - so a library user is never made to write a file it did not ask for.
pub fn host_one(
    listener: &TcpListener,
    identity: &Identity,
    name: &str,
    code: &PairingCode,
) -> Result<Paired, WireError> {
    let (stream, _) = listener.accept()?;
    host_over(stream, identity, name, code)
}

/// Pair over a connection that is already open.
///
/// Separate from [`host_one`] because whoever waits for the guest usually needs
/// to be able to stop waiting - a panel that can be closed, a deadline - and a
/// blocking `accept` inside this function would take that choice away.
pub fn host_over(
    stream: TcpStream,
    identity: &Identity,
    name: &str,
    code: &PairingCode,
) -> Result<Paired, WireError> {
    // A listener that polls for a guest is non-blocking, and Windows hands the
    // accepted socket the same mode. This handshake waits on purpose, with its
    // own deadline below, so it says so before writing anything.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(PATIENCE))?;
    stream.set_write_timeout(Some(PATIENCE))?;

    let mut writing = stream.try_clone()?;
    let mut reading = BufReader::new(stream);

    let opening = offer(identity, name);
    send(&mut writing, &opening)?;

    let reply: Answer = receive(&mut reading)?;
    // Nothing is written back when the proof fails: a refused pairing leaves no
    // trace but the reason, and the guest learns nothing it did not already have.
    let confirmed = confirm(code, &opening, &reply)?;
    send(&mut writing, &confirmed)?;

    let (id, name) = reply.guest();
    Ok(Paired {
        id: id.clone(),
        name: name.to_string(),
    })
}

/// Pair with a host that is listening at `address` (`host:port`).
pub fn join(
    address: &str,
    identity: &Identity,
    name: &str,
    code: &PairingCode,
) -> Result<Paired, WireError> {
    let stream = connect(address)?;
    stream.set_read_timeout(Some(PATIENCE))?;
    stream.set_write_timeout(Some(PATIENCE))?;

    let mut writing = stream.try_clone()?;
    let mut reading = BufReader::new(stream);

    let opening: Offer = receive(&mut reading)?;
    let (reply, key) = answer(identity, name, code, &opening);
    send(&mut writing, &reply)?;

    let confirmed: Confirmed = receive(&mut reading)?;
    accept(&key, &opening, &reply, &confirmed)?;

    let (id, name) = opening.host();
    Ok(Paired {
        id: id.clone(),
        name: name.to_string(),
    })
}

/// Connect with a timeout, so a wrong address is a message rather than a minute
/// of a panel that says "pairing" and will not stop saying it.
fn connect(address: &str) -> Result<TcpStream, WireError> {
    let mut last = None;
    for candidate in address.to_socket_addrs()? {
        match TcpStream::connect_timeout(&candidate, Duration::from_secs(5)) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = Some(error),
        }
    }

    Err(match last {
        Some(error) => WireError::Network(error),
        None => WireError::Network(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "an address is host:port",
        )),
    })
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
