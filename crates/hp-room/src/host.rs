//! The host end of a room: accept connections, decide, fan out.
//!
//! The rules live in `Room`; this is the part that owns sockets and threads. The
//! split matters because it decides where the hard thinking is tested: everything
//! about who may do what is in `Room` and needs no network, and this file is
//! only plumbing - accept, read a message, apply it, write the events back.
//!
//! Effects are handed to the caller through a channel rather than carried out
//! here. Writing into a pane is the app's job: it owns the PTY, the guard and the
//! audit log, and none of that belongs in a crate about who may see what.

use std::collections::BTreeMap;
use std::io::Write;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use thiserror::Error;

use crate::net::{Connection, Hello, NetError};
use crate::{encode, Delivery, Effect, Message, ParticipantId, Room, RoomSnapshot, HOST};

/// How long a peer has to introduce itself, and to take an event it is sent.
const HANDSHAKE: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub enum HostError {
    #[error("could not listen on {address}: {source}")]
    Listen {
        address: String,
        #[source]
        source: std::io::Error,
    },
    #[error("that is not an address: {0}")]
    Address(String),
    #[error(transparent)]
    Net(#[from] NetError),
}

/// The host's view: the rules, the peers, and the place effects go.
pub struct Host {
    address: SocketAddr,
    room: Arc<Mutex<Room>>,
    peers: Arc<Mutex<BTreeMap<ParticipantId, TcpStream>>>,
    effects: Sender<Effect>,
    stop: Arc<AtomicBool>,
}

impl Host {
    /// Listen and be ready. `address` is `127.0.0.1:0` for "any free port".
    ///
    /// The effects receiver comes back with the host: the caller drains it and
    /// does the writing, which keeps the socket threads away from the PTY.
    pub fn start(address: &str, name: &str) -> Result<(Self, Receiver<Effect>), HostError> {
        let address: SocketAddr = address
            .to_socket_addrs()
            .map_err(|_| HostError::Address(address.to_string()))?
            .next()
            .ok_or_else(|| HostError::Address(address.to_string()))?;

        let listener = TcpListener::bind(address).map_err(|source| HostError::Listen {
            address: address.to_string(),
            source,
        })?;
        let local = listener.local_addr().map_err(|source| HostError::Listen {
            address: address.to_string(),
            source,
        })?;

        let room = Arc::new(Mutex::new(Room::open("room", name)));
        let peers: Arc<Mutex<BTreeMap<ParticipantId, TcpStream>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (effects, receiver) = mpsc::channel();

        {
            let room = Arc::clone(&room);
            let peers = Arc::clone(&peers);
            let stop = Arc::clone(&stop);
            let effects = effects.clone();
            thread::Builder::new()
                .name("hp-room-accept".to_string())
                .spawn(move || accept(listener, room, peers, stop, effects))
                .map_err(|error| HostError::Listen {
                    address: address.to_string(),
                    source: error,
                })?;
        }

        Ok((
            Self {
                address: local,
                room,
                peers,
                effects,
                stop,
            },
            receiver,
        ))
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    /// The room as the host may see it.
    pub fn snapshot(&self) -> Option<RoomSnapshot> {
        self.room.lock().ok().map(|room| room.snapshot_for(HOST))
    }

    /// Do something as the host: share, grant, focus, or type.
    pub fn apply(&self, message: Message) {
        let (deliveries, effects) = match self.room.lock() {
            Ok(mut room) => {
                let applied = room.apply(HOST, message);
                (applied.deliveries, applied.effects)
            }
            Err(_) => return,
        };

        for effect in effects {
            let _ = self.effects.send(effect);
        }
        deliver(&self.peers, deliveries);
    }

    /// Output from one of the host's panes. Everyone who may see it is told.
    pub fn mirror(&self, pane: &str, data: &str) {
        let deliveries = match self.room.lock() {
            Ok(room) => room.mirror(pane, data),
            Err(_) => return,
        };
        deliver(&self.peers, deliveries);
    }

    /// Stop accepting and end every peer.
    ///
    /// Shutting each socket down is what unblocks the threads reading from it:
    /// dropping the map would only close one handle on a socket another handle
    /// is still reading, which is the difference between "closed" and "quiet".
    /// The accept thread is woken by a connection of our own.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);

        if let Ok(peers) = self.peers.lock() {
            for stream in peers.values() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }

        let _ = TcpStream::connect(self.address);
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept(
    listener: TcpListener,
    room: Arc<Mutex<Room>>,
    peers: Arc<Mutex<BTreeMap<ParticipantId, TcpStream>>>,
    stop: Arc<AtomicBool>,
    effects: Sender<Effect>,
) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else { continue };

        let room = Arc::clone(&room);
        let peers = Arc::clone(&peers);
        let effects = effects.clone();
        let _ = thread::Builder::new()
            .name("hp-room-peer".to_string())
            .spawn(move || serve(stream, room, peers, effects));
    }
}

/// One peer, from its hello to its last message.
fn serve(
    stream: TcpStream,
    room: Arc<Mutex<Room>>,
    peers: Arc<Mutex<BTreeMap<ParticipantId, TcpStream>>>,
    effects: Sender<Effect>,
) {
    // The post office for this peer: a second handle on the same socket, used by
    // whoever has an event for it.
    if stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err() {
        return;
    }
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let Ok(mut connection) = Connection::new(stream) else {
        return;
    };
    let _ = connection.set_read_timeout(Some(HANDSHAKE));

    let Ok(hello) = connection.handshake::<Hello>() else {
        return;
    };
    let _ = connection.set_read_timeout(None);

    let (id, applied) = match room.lock() {
        Ok(mut room) => room.join(hello.name),
        Err(_) => return,
    };
    if let Ok(mut peers) = peers.lock() {
        peers.insert(id.clone(), writer);
    }
    for effect in applied.effects {
        let _ = effects.send(effect);
    }
    deliver(&peers, applied.deliveries);

    // Read until the peer goes away, applying each message as it arrives.
    loop {
        match connection.next_message() {
            Ok(Some(message)) => {
                let (deliveries, writes) = match room.lock() {
                    Ok(mut room) => {
                        let applied = room.apply(&id, message);
                        (applied.deliveries, applied.effects)
                    }
                    Err(_) => break,
                };
                for effect in writes {
                    let _ = effects.send(effect);
                }
                deliver(&peers, deliveries);
            }
            // A clean end, a closed socket, or a line that was not a message all
            // mean this peer is done.
            Ok(None) | Err(_) => break,
        }
    }

    let applied = match room.lock() {
        Ok(mut room) => room.leave(&id),
        Err(_) => return,
    };
    if let Ok(mut peers) = peers.lock() {
        peers.remove(&id);
    }
    deliver(&peers, applied.deliveries);
}

/// Write each event to the peer it is for.
///
/// A peer that cannot take an event in time is dropped: one viewer that stopped
/// reading must not hold the room up.
fn deliver(peers: &Mutex<BTreeMap<ParticipantId, TcpStream>>, deliveries: Vec<Delivery>) {
    let Ok(mut peers) = peers.lock() else { return };

    for delivery in deliveries {
        let Some(stream) = peers.get_mut(&delivery.to) else {
            continue;
        };
        let Ok(mut line) = encode(&delivery.event) else {
            continue;
        };
        line.push('\n');

        let sent = stream
            .write_all(line.as_bytes())
            .and_then(|()| stream.flush())
            .is_ok();

        if !sent {
            peers.remove(&delivery.to);
        }
    }
}
