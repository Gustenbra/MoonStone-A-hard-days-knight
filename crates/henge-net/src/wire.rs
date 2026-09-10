//! The link: length-prefixed JSON frames over TCP, polled rather than blocked on.
//!
//! There is no runtime and no thread here. A [`Link`] is a non-blocking socket
//! with a read buffer and a write buffer, and the game loop calls
//! [`Link::poll`] once a tick: whatever whole frames have arrived come back, and
//! whatever is still queued goes out. That keeps the network on the same thread
//! as the simulation, which is what makes a lockstep tick easy to reason about
//! and a desync impossible to blame on a race.
//!
//! A frame is four bytes of big-endian length and then that many bytes of JSON.
//! A length above [`MAX_FRAME`] is refused rather than allocated, because the
//! other end of a socket is not to be trusted with how much memory to reserve.
//!
//! **Nagle is off.** A lockstep tick is one small message whose whole purpose is
//! to arrive now, and waiting for a second one to coalesce with it would add a
//! tick of delay to every tick of the game.

use crate::proto::Msg;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};

/// The longest frame that will be read. A roster is a few hundred bytes and a
/// turn is a few dozen, so this is four orders of magnitude of headroom and
/// still far too small to be used as a way to exhaust memory.
pub const MAX_FRAME: usize = 64 * 1024;

/// How many bytes of length prefix.
const HEADER: usize = 4;

/// What can go wrong on a link.
#[derive(Debug)]
pub enum WireError {
    /// The socket did.
    Io(std::io::Error),
    /// A frame claimed to be longer than [`MAX_FRAME`].
    TooLong(usize),
    /// A frame arrived whole and was not a message.
    Garbled(serde_json::Error),
    /// The other end closed.
    Closed,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Io(e) => write!(f, "{e}"),
            WireError::TooLong(n) => write!(f, "a {n}-byte frame, which is too long to be one"),
            WireError::Garbled(e) => write!(f, "a frame that is not a message: {e}"),
            WireError::Closed => write!(f, "the other end closed"),
        }
    }
}

impl std::error::Error for WireError {}

impl From<std::io::Error> for WireError {
    fn from(e: std::io::Error) -> WireError {
        WireError::Io(e)
    }
}

/// One connection, either way round.
pub struct Link {
    sock: TcpStream,
    peer: SocketAddr,
    /// Bytes read but not yet a whole frame.
    inbox: Vec<u8>,
    /// Frames queued but not yet written, head first.
    outbox: Vec<u8>,
    /// How far into `outbox` the socket has taken.
    sent: usize,
    closed: bool,
}

impl Link {
    /// Dial a host. This one blocks, because there is nothing to do until it
    /// answers and a menu that says "connecting" is a menu that has already
    /// called this.
    pub fn connect(addr: impl ToSocketAddrs) -> Result<Link, WireError> {
        let mut last = None;
        for a in addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&a, std::time::Duration::from_secs(8)) {
                Ok(s) => return Link::wrap(s),
                Err(e) => last = Some(e),
            }
        }
        Err(WireError::Io(last.unwrap_or_else(|| {
            std::io::Error::new(ErrorKind::InvalidInput, "no address to connect to")
        })))
    }

    /// Take over an accepted socket.
    pub fn wrap(sock: TcpStream) -> Result<Link, WireError> {
        sock.set_nonblocking(true)?;
        // See the module note: a tick's message is not waiting for company.
        let _ = sock.set_nodelay(true);
        let peer = sock.peer_addr()?;
        Ok(Link {
            sock,
            peer,
            inbox: Vec::new(),
            outbox: Vec::new(),
            sent: 0,
            closed: false,
        })
    }

    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// Whether the other end has gone, or we have given up on it.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Queue a message. Nothing is written until [`Link::poll`] or
    /// [`Link::flush`], so a handful of messages built in one tick go out
    /// together.
    pub fn send(&mut self, msg: &Msg) -> Result<(), WireError> {
        let body = serde_json::to_vec(msg).map_err(WireError::Garbled)?;
        if body.len() > MAX_FRAME {
            return Err(WireError::TooLong(body.len()));
        }
        self.outbox
            .extend_from_slice(&(body.len() as u32).to_be_bytes());
        self.outbox.extend_from_slice(&body);
        Ok(())
    }

    /// Push what is queued. A socket that will not take all of it keeps the rest
    /// for the next call rather than blocking the game.
    pub fn flush(&mut self) -> Result<(), WireError> {
        while self.sent < self.outbox.len() {
            match self.sock.write(&self.outbox[self.sent..]) {
                Ok(0) => {
                    self.closed = true;
                    return Err(WireError::Closed);
                }
                Ok(n) => self.sent += n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.closed = true;
                    return Err(WireError::Io(e));
                }
            }
        }
        if self.sent == self.outbox.len() {
            self.outbox.clear();
            self.sent = 0;
        } else if self.sent > MAX_FRAME {
            // Keep the unsent tail at the front rather than letting the buffer
            // grow without bound behind a peer that is reading slowly.
            self.outbox.drain(..self.sent);
            self.sent = 0;
        }
        Ok(())
    }

    /// Read whatever has arrived and return every whole message in it, then push
    /// whatever is queued to go out.
    ///
    /// An error here is the end of the link: the caller drops it and tells the
    /// person. Messages read before the error are still returned, because the
    /// last thing a peer says before it goes is often [`Msg::Bye`].
    pub fn poll(&mut self) -> (Vec<Msg>, Option<WireError>) {
        let mut out = Vec::new();
        let mut err = None;
        let mut chunk = [0u8; 8192];
        loop {
            match self.sock.read(&mut chunk) {
                Ok(0) => {
                    self.closed = true;
                    err = Some(WireError::Closed);
                    break;
                }
                Ok(n) => {
                    if self.inbox.len() + n > MAX_FRAME * 4 {
                        self.closed = true;
                        err = Some(WireError::TooLong(self.inbox.len() + n));
                        break;
                    }
                    self.inbox.extend_from_slice(&chunk[..n]);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.closed = true;
                    err = Some(WireError::Io(e));
                    break;
                }
            }
        }
        // Whole frames out of the buffer, however they were split on the way.
        let mut at = 0;
        while err.is_none() || !self.inbox.is_empty() {
            if self.inbox.len() < at + HEADER {
                break;
            }
            let len = u32::from_be_bytes([
                self.inbox[at],
                self.inbox[at + 1],
                self.inbox[at + 2],
                self.inbox[at + 3],
            ]) as usize;
            if len > MAX_FRAME {
                self.closed = true;
                err = Some(WireError::TooLong(len));
                break;
            }
            if self.inbox.len() < at + HEADER + len {
                break;
            }
            let body = &self.inbox[at + HEADER..at + HEADER + len];
            match serde_json::from_slice::<Msg>(body) {
                Ok(m) => out.push(m),
                Err(e) => {
                    self.closed = true;
                    err = Some(WireError::Garbled(e));
                    at += HEADER + len;
                    break;
                }
            }
            at += HEADER + len;
        }
        if at > 0 {
            self.inbox.drain(..at);
        }
        if err.is_none() {
            if let Err(e) = self.flush() {
                err = Some(e);
            }
        }
        (out, err)
    }
}

/// The host's door: a non-blocking listener.
pub struct Listener {
    sock: TcpListener,
    port: u16,
}

impl Listener {
    /// Open the door. Binding the unspecified address is deliberate: a host is
    /// reached from the local network and, with [`crate::portmap`], from outside
    /// it, and binding the loopback only would make a lobby nobody can join.
    pub fn open(port: u16) -> Result<Listener, WireError> {
        let sock = TcpListener::bind(("0.0.0.0", port))?;
        sock.set_nonblocking(true)?;
        let port = sock.local_addr()?.port();
        Ok(Listener { sock, port })
    }

    /// Which port it actually got, which is the interesting number when the
    /// caller asked for zero.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Whoever has knocked since last time.
    pub fn accept(&self) -> Vec<Link> {
        let mut out = Vec::new();
        loop {
            match self.sock.accept() {
                Ok((s, _)) => match Link::wrap(s) {
                    Ok(l) => out.push(l),
                    // A socket that cannot be set up is not a reason to stop
                    // accepting the next one.
                    Err(_) => continue,
                },
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Lobby, SeatInput};

    /// Spin both ends until a message arrives or the patience runs out. A test
    /// must not block on a socket, and a non-blocking socket needs a turn of the
    /// crank: this is the game loop, compressed.
    fn pump(a: &mut Link, b: &mut Link) -> Vec<Msg> {
        for _ in 0..2000 {
            let (_, ea) = a.poll();
            assert!(ea.is_none(), "{:?}", ea);
            let (msgs, eb) = b.poll();
            assert!(eb.is_none(), "{:?}", eb);
            if !msgs.is_empty() {
                return msgs;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("nothing arrived");
    }

    fn pair() -> (Link, Link) {
        let door = Listener::open(0).expect("a port");
        let port = door.port();
        let mut guest = Link::connect(("127.0.0.1", port)).expect("to connect");
        for _ in 0..2000 {
            let mut got = door.accept();
            if let Some(host) = got.pop() {
                return (host, guest);
            }
            let (_, e) = guest.poll();
            assert!(e.is_none(), "{:?}", e);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("nobody was accepted");
    }

    #[test]
    fn a_message_goes_one_way_and_comes_back_the_other() {
        let (mut host, mut guest) = pair();
        let hello = Msg::Hello {
            protocol: crate::proto::PROTOCOL,
            name: "carl".into(),
        };
        guest.send(&hello).unwrap();
        assert_eq!(pump(&mut guest, &mut host), vec![hello]);
        let welcome = Msg::Welcome {
            seat: 1,
            lobby: Lobby::new("a game", true),
        };
        host.send(&welcome).unwrap();
        assert_eq!(pump(&mut host, &mut guest), vec![welcome]);
    }

    /// Several frames queued in one tick come out as several messages, in order,
    /// however the kernel chose to split them.
    #[test]
    fn frames_arrive_whole_and_in_order_however_they_were_split() {
        let (mut host, mut guest) = pair();
        let sent: Vec<Msg> = (0..64)
            .map(|t| Msg::Input {
                tick: t,
                input: SeatInput {
                    pad: (t % 32) as u8,
                    ..SeatInput::default()
                },
            })
            .collect();
        for m in &sent {
            guest.send(m).unwrap();
        }
        let mut got = Vec::new();
        for _ in 0..2000 {
            let (_, e) = guest.poll();
            assert!(e.is_none());
            let (msgs, e) = host.poll();
            assert!(e.is_none());
            got.extend(msgs);
            if got.len() >= sent.len() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(got, sent);
    }

    #[test]
    fn a_closed_socket_is_noticed_rather_than_waited_on() {
        let (mut host, guest) = pair();
        drop(guest);
        for _ in 0..2000 {
            let (_, e) = host.poll();
            if e.is_some() {
                assert!(host.closed());
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the close was never noticed");
    }

    /// A peer claiming a huge frame is refused without reserving anything for
    /// it, which is the one place a socket gets to choose an allocation.
    #[test]
    fn an_absurd_length_is_refused() {
        let (mut host, guest) = pair();
        let mut raw = guest;
        // Reach past `send` on purpose: this is a frame no honest peer writes.
        raw.outbox
            .extend_from_slice(&(MAX_FRAME as u32 + 1).to_be_bytes());
        raw.flush().unwrap();
        for _ in 0..2000 {
            let (_, e) = host.poll();
            if let Some(WireError::TooLong(n)) = e {
                assert_eq!(n, MAX_FRAME + 1);
                return;
            }
            let _ = raw.poll();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the length was accepted");
    }
}
