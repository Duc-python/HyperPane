//! Two machines pairing over a socket.
//!
//! The exchange is four messages. What is worth checking over a real socket
//! rather than in memory is that the order is enough and nothing else is needed:
//! no third party, no name lookup, no shared file. A code typed by a person is
//! the whole of the secret.

use hp_identity::pairing::{self, PairingError, WireError};
use hp_identity::{Identity, PairingCode, Peers};

#[test]
fn two_machines_pair_and_both_pin_the_other() {
    let code = PairingCode::generate();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("an address").to_string();

    // The host opens the pairing; the guest answers it.
    let host_code = code.clone();
    let host = std::thread::spawn(move || {
        let identity = Identity::generate();
        let paired =
            pairing::host_one(&listener, &identity, "desk", &host_code).expect("the guest pairs");
        (identity.id(), paired)
    });

    let guest = Identity::generate();
    let paired = pairing::join(&address, &guest, "laptop", &code).expect("the host pairs");

    let (host_id, host_paired) = host.join().expect("the host thread");

    // Each side learned the other's identity and the name that travels with it.
    assert_eq!(host_paired.id, guest.id());
    assert_eq!(host_paired.name, "laptop");
    assert_eq!(paired.id, host_id);
    assert_eq!(paired.name, "desk");

    // And what the app does with the answer: pin it. The pinned id verifies its
    // own signatures, so a reconnect later needs no code.
    let mut peers = Peers::default();
    assert_eq!(
        peers.remember(&paired.id, &paired.name, 1),
        hp_identity::peers::Remembered::Added
    );
    assert!(peers.is_trusted(&paired.id));
}

#[test]
fn a_listener_that_polls_still_gives_a_socket_that_can_wait() {
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    // The app's host polls its listener instead of blocking inside accept, and
    // Windows hands the accepted socket the listener's mode. A handshake that
    // assumed blocking wrote its opening message into a non-blocking socket,
    // dropped the connection, and left the guest reading a reset (os error
    // 10053) - which looked exactly like a wrong code.
    let code = PairingCode::generate();
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    listener.set_nonblocking(true).expect("a mode");
    let address = listener.local_addr().expect("an address").to_string();

    let host_code = code.clone();
    let host = std::thread::spawn(move || {
        let identity = Identity::generate();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((stream, _)) => {
                    return pairing::host_over(stream, &identity, "desk", &host_code)
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
        panic!("no guest arrived");
    });

    let guest = Identity::generate();
    let paired = pairing::join(&address, &guest, "laptop", &code).expect("the host pairs");
    assert_eq!(paired.name, "desk");

    let host_paired = host
        .join()
        .expect("the host thread")
        .expect("the host paired");
    assert_eq!(host_paired.name, "laptop");
}

#[test]
fn a_wrong_code_is_refused_by_the_host_and_nothing_is_pinned() {
    let code = PairingCode::generate();
    let wrong = PairingCode::generate();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("an address").to_string();

    let host = std::thread::spawn(move || {
        let identity = Identity::generate();
        pairing::host_one(&listener, &identity, "desk", &code)
    });

    let guest = Identity::generate();
    let joined = pairing::join(&address, &guest, "laptop", &wrong);

    // The host says nothing back after a failed proof, and the guest is left
    // holding an offer it cannot complete: that is the correct end of a refused
    // pairing, whichever way the closed socket reaches it.
    assert!(
        matches!(
            host.join().expect("the host thread"),
            Err(WireError::Pairing(PairingError::WrongCode))
        ),
        "the host must refuse a code it does not share"
    );
    assert!(joined.is_err(), "the guest must not think it paired");
}
