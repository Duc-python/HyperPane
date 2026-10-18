//! The visiting end of a room.
//!
//! Deliberately thin: a client asks and is told. It does not decide what it may
//! see, and it does not know what it is missing - the host sends a snapshot
//! built for this participant, and the client keeps it.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use thiserror::Error;

use crate::net::{Connection, Hello, NetError};
use crate::{Event, Message, ParticipantId, RoomSnapshot};

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("that is not an address: {0}")]
    Address(String),
    #[error("could not reach {address}: {source}")]
    Connect {
        address: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Net(#[from] NetError),
    #[error("the host answered with something other than a welcome")]
    NotAWelcome,
}

/// A joined participant.
pub struct Client {
    connection: Connection,
    me: ParticipantId,
    welcome: RoomSnapshot,
}

impl Client {
    /// Join a room: say who you are, and wait for the room to answer.
    pub fn join(address: &str, name: &str, timeout: Duration) -> Result<Self, ClientError> {
        let address: SocketAddr = address
            .to_socket_addrs()
            .map_err(|_| ClientError::Address(address.to_string()))?
            .next()
            .ok_or_else(|| ClientError::Address(address.to_string()))?;

        let stream = TcpStream::connect_timeout(&address, timeout).map_err(|source| {
            ClientError::Connect {
                address: address.to_string(),
                source,
            }
        })?;

        let mut connection = Connection::new(stream)?;
        connection.set_read_timeout(Some(timeout))?;
        connection.send(&Hello {
            name: name.to_string(),
        })?;

        let Some(Event::Welcome { you, room, .. }) = connection.next_event()? else {
            return Err(ClientError::NotAWelcome);
        };

        // From here the connection is quiet until something happens, and a
        // timeout is not an error: it is what an idle room looks like.
        connection.set_read_timeout(None)?;

        Ok(Self {
            connection,
            me: you,
            welcome: room,
        })
    }

    /// The id the host gave this participant.
    pub fn me(&self) -> &str {
        &self.me
    }

    /// The room as the host described it on joining.
    pub fn welcome(&self) -> &RoomSnapshot {
        &self.welcome
    }

    pub fn send(&mut self, message: Message) -> Result<(), ClientError> {
        self.connection.send(&message)?;
        Ok(())
    }

    /// The next thing the host says, or `None` when it closed the room.
    pub fn next_event(&mut self) -> Result<Option<Event>, ClientError> {
        Ok(self.connection.next_event()?)
    }
}
