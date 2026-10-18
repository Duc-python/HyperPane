//! A stream that carries room messages, one JSON object per line.
//!
//! The rules do not care what carries them, and this is the simplest stream that
//! proves the framing: newline-delimited JSON, which is also what the MCP
//! transport uses, so a person reading a capture has one shape to learn.
//!
//! This is **not** a security boundary. Identity here is whatever the stream is
//! attached to, and nothing stops a process on the same machine from connecting.
//! Binding an identity to a connection is TLS or QUIC's job, and that is the
//! next step rather than something this module pretends to do.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

use crate::{decode, encode, Event, Message, WireError, MAX_MESSAGE};

#[derive(Debug, Error)]
pub enum NetError {
    #[error("the connection failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("the message could not be encoded: {0}")]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("a message longer than {MAX_MESSAGE} bytes arrived, and the line was cut")]
    TooLong,
    #[error("the other end closed the connection")]
    Closed,
}

/// The first line a participant sends, before the room has given it an id.
///
/// It is not a `Message`: the room decides who you are, so this is the only
/// thing a stranger may say.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub name: String,
}

/// One connection, both directions.
#[derive(Debug)]
pub struct Connection {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Result<Self, NetError> {
        // A keystroke is a few bytes, and waiting to coalesce it is the
        // keystroke arriving late.
        stream.set_nodelay(true)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self { stream, reader })
    }

    /// Send one message. It is flushed here: a caller that has to remember to
    /// flush is a caller that will forget.
    pub fn send<T: Serialize>(&mut self, message: &T) -> Result<(), NetError> {
        let mut line = encode(message)?;
        line.push('\n');
        self.stream.write_all(line.as_bytes())?;
        self.stream.flush()?;
        Ok(())
    }

    /// The next message from a participant, or `None` when it closed cleanly.
    pub fn next_message(&mut self) -> Result<Option<Message>, NetError> {
        match self.next_line()? {
            None => Ok(None),
            Some(line) => Ok(Some(decode(&line)?)),
        }
    }

    /// The next event for a participant. The two directions carry different
    /// types on purpose: a client sends messages and is sent events, so neither
    /// side can quietly act as the other.
    pub fn next_event(&mut self) -> Result<Option<Event>, NetError> {
        match self.next_line()? {
            None => Ok(None),
            Some(line) => Ok(Some(decode(&line)?)),
        }
    }

    /// The one message a peer sends before it has an id.
    pub fn handshake<T: DeserializeOwned>(&mut self) -> Result<T, NetError> {
        match self.next_line()? {
            None => Err(NetError::Closed),
            Some(line) => Ok(decode(&line)?),
        }
    }

    /// How long a read may block. `None` blocks until something arrives, which
    /// is what a participant waiting for events wants.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), NetError> {
        self.stream.set_read_timeout(timeout)?;
        Ok(())
    }

    /// One line, refused if it is longer than the limit.
    ///
    /// The read is bounded, so an endless line cannot grow this process. A line
    /// that arrives without its newline was cut, and the caller is told that
    /// rather than being handed half a message.
    fn next_line(&mut self) -> Result<Option<String>, NetError> {
        let mut line = String::new();
        let read = self
            .reader
            .by_ref()
            .take(MAX_MESSAGE as u64 + 1)
            .read_line(&mut line)?;

        if read == 0 {
            return Ok(None);
        }
        if !line.ends_with('\n') {
            return Err(NetError::TooLong);
        }

        Ok(Some(line.trim_end().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Access, Event};
    use std::net::{TcpListener, TcpStream};

    /// A listener on a port the operating system picked, and a client connected
    /// to it.
    fn pair() -> (Connection, Connection) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("an address");
        let client = TcpStream::connect(address).expect("a connection");
        let (accepted, _) = listener.accept().expect("an accepted connection");

        (
            Connection::new(client).expect("a client"),
            Connection::new(accepted).expect("a server"),
        )
    }

    #[test]
    fn a_message_and_an_event_round_trip_over_a_socket() {
        let (mut client, mut server) = pair();

        client
            .send(&Message::Grant {
                pane: "secret".into(),
                who: "p1".into(),
                access: Access::Control,
            })
            .expect("it sends");

        assert_eq!(
            server.next_message().expect("it reads"),
            Some(Message::Grant {
                pane: "secret".into(),
                who: "p1".into(),
                access: Access::Control,
            })
        );

        server
            .send(&Event::Focus {
                who: "p1".into(),
                pane: Some("secret".into()),
            })
            .expect("it sends back");

        assert_eq!(
            client.next_event().expect("it reads"),
            Some(Event::Focus {
                who: "p1".into(),
                pane: Some("secret".into()),
            })
        );
    }

    #[test]
    fn a_closed_connection_is_an_end_and_not_an_error() {
        let (client, mut server) = pair();
        drop(client);

        assert_eq!(server.next_message().expect("a clean end"), None);
    }

    #[test]
    fn a_line_longer_than_the_limit_is_refused() {
        let (mut client, mut server) = pair();

        client.send(&"x".repeat(MAX_MESSAGE)).expect("it sends");

        match server.next_message() {
            Err(NetError::TooLong) => {}
            other => panic!("expected a refused line, got {other:?}"),
        }
    }
}
