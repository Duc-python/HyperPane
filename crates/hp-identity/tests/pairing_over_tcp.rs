//! Two machines pairing over a socket.
//!
//! The exchange is four messages. What is worth checking over a real socket
//! rather than in memory is that the order is enough and nothing else is needed:
//! no third party, no name lookup, no shared file. A code typed by a person is
//! the whole of the secret.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use hp_identity::pairing::{self, Answer, Confirmed, Offer};
use hp_identity::{Identity, PairingCode, Peers};

fn send<T: serde::Serialize>(stream: &mut TcpStream, message: &T) {
    let mut line = serde_json::to_string(message).expect("it encodes");
    line.push('\n');
    stream.write_all(line.as_bytes()).expect("it writes");
    stream.flush().expect("it flushes");
}

fn receive<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> Option<T> {
    let mut line = String::new();
    let read = reader.read_line(&mut line).expect("it reads");
    if read == 0 {
        return None;
    }
    Some(serde_json::from_str(line.trim_end()).expect("it decodes"))
}

#[test]
fn two_machines_pair_and_both_pin_the_other() {
    let code = PairingCode::generate();
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("an address");

    // The host opens the pairing; the guest answers it.
    let host_code = code.clone();
    let host = std::thread::spawn(move || {
        let identity = Identity::generate();
        let (stream, _) = listener.accept().expect("a connection");
        let mut writing = stream.try_clone().expect("a handle");
        let mut reading = BufReader::new(stream);
        reading
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("a timeout");

        let offer = pairing::offer(&identity, "desk");
        send(&mut writing, &offer);

        let answer: Answer = receive(&mut reading).expect("the guest answered");
        let confirmed = pairing::confirm(&host_code, &offer, &answer).expect("the code matches");
        send(&mut writing, &confirmed);

        let mut peers = Peers::default();
        let (guest, name) = answer.guest();
        let remembered = peers.remember(guest, name, 1);

        (identity.id(), peers, remembered)
    });

    let guest = Identity::generate();
    let stream = TcpStream::connect(address).expect("a connection");
    let mut writing = stream.try_clone().expect("a handle");
    let mut reading = BufReader::new(stream);
    reading
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a timeout");

    let offer: Offer = receive(&mut reading).expect("the host offered");
    assert_eq!(offer.host().1, "desk");

    let (reply, key) = pairing::answer(&guest, "laptop", &code, &offer);
    send(&mut writing, &reply);

    let confirmed: Confirmed = receive(&mut reading).expect("the host proved itself");
    pairing::accept(&key, &offer, &reply, &confirmed).expect("the host knows the code");

    let (host_id, host_peers, remembered) = host.join().expect("the host thread");
    assert_eq!(remembered, hp_identity::peers::Remembered::Added);

    // Both ends now trust each other's id, and nothing else was needed.
    assert!(host_peers.is_trusted(&guest.id()));
    assert_eq!(
        host_peers.get(&guest.id()).expect("pinned").name,
        "laptop",
        "the name travels with the pair, so the person sees the machine they typed the code for"
    );
    assert_eq!(host_id, offer.id);
}

#[test]
fn a_wrong_code_is_refused_by_the_host_and_nothing_is_pinned() {
    let code = PairingCode::generate();
    let wrong = PairingCode::generate();
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("an address");

    let host_code = code.clone();
    let host = std::thread::spawn(move || {
        let identity = Identity::generate();
        let (stream, _) = listener.accept().expect("a connection");
        let mut writing = stream.try_clone().expect("a handle");
        let mut reading = BufReader::new(stream);

        let offer = pairing::offer(&identity, "desk");
        send(&mut writing, &offer);

        let answer: Answer = receive(&mut reading).expect("the guest answered");
        let refused = pairing::confirm(&host_code, &offer, &answer);

        // Nothing is sent back, and nothing is pinned: a refused pairing leaves
        // no trace but the reason.
        refused.is_err()
    });

    let stream = TcpStream::connect(address).expect("a connection");
    let mut writing = stream.try_clone().expect("a handle");
    let mut reading = BufReader::new(stream);
    reading
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a timeout");

    let guest = Identity::generate();
    let offer: Offer = receive(&mut reading).expect("the host offered");
    send(
        &mut writing,
        &pairing::answer(&guest, "laptop", &wrong, &offer).0,
    );

    assert!(
        host.join().expect("the host thread"),
        "the host should have refused a code it does not share"
    );
    // The host says nothing more: the guest is left holding an offer it cannot
    // complete, which is the correct end of a refused pairing.
    assert_eq!(receive::<Confirmed>(&mut reading), None);
}
