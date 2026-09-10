//! Opening the host's port, so that nobody has to find their router's web page.
//!
//! **Ours entirely**, like the rest of this crate, and the most thankless part of
//! it: a host behind a home router is not reachable from the internet until
//! something tells the router to send that port inside. Two protocols do that,
//! both spoken by almost every consumer router made this century, and neither
//! needs a library:
//!
//! - **NAT-PMP** (and its successor PCP), one UDP datagram to the gateway on port
//!   5351. Twelve bytes out, sixteen back. Apple's routers and most of the rest.
//! - **UPnP IGD**, which is SSDP to find the gateway's description and then two
//!   SOAP calls over plain HTTP on the local network. Wordier, and what most
//!   Windows-facing routers speak.
//!
//! NAT-PMP is tried first because it is one datagram and answers in milliseconds;
//! UPnP is the fallback. Both are asked for a mapping with a lifetime rather than
//! a permanent one, so a host that crashes does not leave a hole open in
//! somebody's router for ever, and [`Mapping::close`] takes it down on the way
//! out.
//!
//! ### What it does not do
//!
//! It does not punch through a carrier-grade NAT, and it cannot: there is no
//! router in the house to ask. [`Reach`] says which of the three answers came
//! back, and the lobby screen says it in words, because a host who is told "port
//! opened" when it was not is a host whose friends cannot join and who has no
//! idea why.
//!
//! It also never talks to anything outside the local network. The external
//! address comes from the router's own `GetExternalIPAddress`, not from an
//! address-reflecting web service, so hosting a game does not tell a third party
//! that you are hosting a game.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

/// How long a mapping is asked for. Renewed at half this while the lobby is up,
/// so a long game does not lose its port, and short enough that a crash cleans
/// itself up within the hour.
pub const LIFETIME: Duration = Duration::from_secs(3600);

/// How long to wait for a router to answer.
const PATIENCE: Duration = Duration::from_millis(1500);

/// How long to wait for SSDP replies, which arrive in a trickle rather than at
/// once.
const SSDP_PATIENCE: Duration = Duration::from_millis(2000);

/// Which way the port was opened, if it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// NAT-PMP answered.
    NatPmp,
    /// UPnP IGD answered.
    Upnp,
    /// Nothing did. The port may still be reachable (no router, or one already
    /// forwarding), and it may not. The person is told exactly that.
    Unknown,
}

impl Reach {
    pub fn opened(self) -> bool {
        self != Reach::Unknown
    }
}

/// What came of asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    /// The port asked for, which is the port guests connect to.
    pub port: u16,
    /// The address outside, when the router would say.
    pub external: Option<IpAddr>,
    /// This machine's address on the local network, which is what a friend in the
    /// same house connects to.
    pub local: Option<Ipv4Addr>,
    pub how: Reach,
    /// What went wrong, for the line under the lobby name. Empty when nothing
    /// did.
    pub note: String,
    /// How the mapping is taken down again, kept from the attempt that made it.
    undo: Undo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Undo {
    Nothing,
    NatPmp { gateway: Ipv4Addr, port: u16 },
    Upnp { control: String, service: String },
}

impl Mapping {
    /// The address to read out to a friend: outside if the router said what it
    /// is, and the local one otherwise.
    pub fn address(&self) -> String {
        match (self.external, self.local) {
            (Some(ip), _) => format!("{ip}:{}", self.port),
            (None, Some(ip)) => format!("{ip}:{}", self.port),
            (None, None) => format!("this machine, port {}", self.port),
        }
    }

    /// Ask again, which is what keeps a long game's mapping alive. Both
    /// protocols treat a repeat as a renewal.
    pub fn renew(&self) {
        match &self.undo {
            Undo::Nothing => {}
            Undo::NatPmp { gateway, port } => {
                let _ = natpmp_map(*gateway, *port, LIFETIME.as_secs() as u32);
            }
            Undo::Upnp { control, service } => {
                if let Some(local) = self.local {
                    let _ = upnp_add(
                        control,
                        service,
                        self.port,
                        local,
                        LIFETIME.as_secs() as u32,
                    );
                }
            }
        }
    }

    /// Take it down. Called on the way out of a lobby, so the hole does not
    /// outlive the game that wanted it.
    pub fn close(&self) {
        match &self.undo {
            Undo::Nothing => {}
            // A lifetime of zero is how both protocols say "forget it".
            Undo::NatPmp { gateway, port } => {
                let _ = natpmp_map(*gateway, *port, 0);
            }
            Undo::Upnp { control, service } => {
                let _ = upnp_delete(control, service, self.port);
            }
        }
    }
}

/// Ask the router to open a port, NAT-PMP first and UPnP after it.
///
/// Never fails: a router that will not answer is an answer, and the caller shows
/// it rather than refusing to host.
pub fn open_port(port: u16) -> Mapping {
    let local = local_ip();
    let mut notes: Vec<String> = Vec::new();

    // NAT-PMP. One datagram, and the gateway is guessed from this machine's own
    // address, which is right for every home network and cheap to be wrong about.
    for gateway in gateways(local) {
        match natpmp_map(gateway, port, LIFETIME.as_secs() as u32) {
            Ok(external) => {
                return Mapping {
                    port,
                    external: external.map(IpAddr::V4),
                    local,
                    how: Reach::NatPmp,
                    note: String::new(),
                    undo: Undo::NatPmp { gateway, port },
                }
            }
            Err(e) => notes.push(format!("{gateway}: {e}")),
        }
    }

    // UPnP IGD.
    match upnp(port, local) {
        Ok((external, control, service)) => {
            return Mapping {
                port,
                external,
                local,
                how: Reach::Upnp,
                note: String::new(),
                undo: Undo::Upnp { control, service },
            }
        }
        Err(e) => notes.push(e),
    }

    Mapping {
        port,
        external: None,
        local,
        how: Reach::Unknown,
        note: notes.join("; "),
        undo: Undo::Nothing,
    }
}

/// This machine's address on the local network.
///
/// Found by asking the routing table which address it would use to reach the
/// outside, which a connected UDP socket answers without sending a single
/// packet. The address it is pointed at is never contacted and does not have to
/// exist.
pub fn local_ip() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    sock.connect(("192.0.2.1", 9)).ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Where the gateway probably is. The first host of this machine's own /24 is
/// right on effectively every home network; the two other private prefixes'
/// usual answers are tried after it so that a guess costs one more datagram
/// rather than a failure.
fn gateways(local: Option<Ipv4Addr>) -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    if let Some(ip) = local {
        let o = ip.octets();
        out.push(Ipv4Addr::new(o[0], o[1], o[2], 1));
        // Some routers sit on .254 instead.
        out.push(Ipv4Addr::new(o[0], o[1], o[2], 254));
    }
    out.dedup();
    out
}

// ------------------------------------------------------------------- NAT-PMP

/// One NAT-PMP mapping request, RFC 6886 section 3.3.
///
/// ```text
/// out  00           version
///      02           opcode: map a TCP port
///      00 00        reserved
///      pp pp        internal port
///      pp pp        external port wanted, and the same one is wanted
///      ll ll ll ll  lifetime in seconds
/// in   00           version
///      82           opcode with the reply bit
///      rr rr        result: zero is yes
///      tt tt tt tt  seconds since the router came up
///      pp pp        internal port
///      pp pp        the external port actually given
///      ll ll ll ll  the lifetime actually given
/// ```
///
/// The external *address* is a second request, opcode zero, which is where
/// `external` comes from. A router that maps the port but will not say its
/// address is still a router that mapped the port.
fn natpmp_map(gateway: Ipv4Addr, port: u16, lifetime: u32) -> Result<Option<Ipv4Addr>, String> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(PATIENCE))
        .map_err(|e| e.to_string())?;
    let to = SocketAddr::from((gateway, 5351));

    let mut req = Vec::with_capacity(12);
    req.push(0u8); // version
    req.push(2u8); // map TCP
    req.extend_from_slice(&0u16.to_be_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&lifetime.to_be_bytes());
    sock.send_to(&req, to).map_err(|e| e.to_string())?;

    let mut buf = [0u8; 32];
    let (n, from) = sock
        .recv_from(&mut buf)
        .map_err(|_| "no answer".to_string())?;
    if from.ip() != IpAddr::V4(gateway) {
        return Err("an answer from somewhere else".into());
    }
    if n < 16 {
        return Err(format!("a {n}-byte answer"));
    }
    if buf[0] != 0 || buf[1] != 0x82 {
        return Err("an answer that is not a mapping".into());
    }
    let result = u16::from_be_bytes([buf[2], buf[3]]);
    if result != 0 {
        return Err(natpmp_reason(result));
    }
    // Taking a mapping down answers the same way and has no address to ask for.
    if lifetime == 0 {
        return Ok(None);
    }
    Ok(natpmp_external(&sock, to))
}

/// Opcode zero: what the router's outside address is.
fn natpmp_external(sock: &UdpSocket, to: SocketAddr) -> Option<Ipv4Addr> {
    sock.send_to(&[0u8, 0u8], to).ok()?;
    let mut buf = [0u8; 32];
    let (n, from) = sock.recv_from(&mut buf).ok()?;
    if from.ip() != to.ip() || n < 12 || buf[1] != 0x80 {
        return None;
    }
    if u16::from_be_bytes([buf[2], buf[3]]) != 0 {
        return None;
    }
    Some(Ipv4Addr::new(buf[8], buf[9], buf[10], buf[11]))
}

/// RFC 6886 section 3.5's result codes, in words a person can act on.
fn natpmp_reason(code: u16) -> String {
    match code {
        1 => "the router speaks a newer version of NAT-PMP".into(),
        2 => "the router refused: port mapping is turned off on it".into(),
        3 => "the router has no address outside yet".into(),
        4 => "the router is out of room for mappings".into(),
        5 => "the router does not map ports this way".into(),
        other => format!("the router said no, code {other}"),
    }
}

// ---------------------------------------------------------------- UPnP IGD

/// Find a gateway, then ask it for the mapping and its address.
///
/// Returns the external address when it would say, and what is needed to take
/// the mapping down again.
fn upnp(port: u16, local: Option<Ipv4Addr>) -> Result<(Option<IpAddr>, String, String), String> {
    let local = local.ok_or("this machine has no address on the local network")?;
    let found = ssdp_discover()?;
    let mut last = "no gateway described itself".to_string();
    for location in found {
        let xml = match http_get(&location) {
            Ok(x) => x,
            Err(e) => {
                last = format!("{location}: {e}");
                continue;
            }
        };
        let Some((service, control)) = wan_service(&xml, &location) else {
            last = format!("{location}: no connection service in its description");
            continue;
        };
        match upnp_add(&control, &service, port, local, LIFETIME.as_secs() as u32) {
            Ok(()) => {
                let external = upnp_external(&control, &service);
                return Ok((external, control, service));
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// SSDP: shout on the local network and collect the `LOCATION` of everything
/// that says it is an internet gateway.
fn ssdp_discover() -> Result<Vec<String>, String> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(Duration::from_millis(300)))
        .map_err(|e| e.to_string())?;
    // Three targets, because routers answer to different ones: the gateway
    // device itself and the two services that actually do the mapping.
    let targets = [
        "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
        "urn:schemas-upnp-org:service:WANIPConnection:1",
        "urn:schemas-upnp-org:service:WANPPPConnection:1",
    ];
    for st in targets {
        let req = format!(
            "M-SEARCH * HTTP/1.1\r\n\
             HOST: 239.255.255.250:1900\r\n\
             MAN: \"ssdp:discover\"\r\n\
             MX: 2\r\n\
             ST: {st}\r\n\r\n"
        );
        let _ = sock.send_to(req.as_bytes(), ("239.255.255.250", 1900));
    }
    let until = std::time::Instant::now() + SSDP_PATIENCE;
    let mut out: Vec<String> = Vec::new();
    let mut buf = [0u8; 2048];
    while std::time::Instant::now() < until {
        let Ok((n, _)) = sock.recv_from(&mut buf) else {
            continue;
        };
        let text = String::from_utf8_lossy(&buf[..n]);
        if let Some(loc) = header(&text, "location") {
            if !out.contains(&loc) {
                out.push(loc);
            }
        }
    }
    if out.is_empty() {
        return Err("nothing on the network answered as a gateway".into());
    }
    Ok(out)
}

/// One header out of an SSDP or HTTP response, case-insensitively because every
/// router spells them differently.
fn header(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// The one element this needs out of a device description. Not a parser: the
/// first `<tag>` and its closing partner, which is all a SOAP answer or a
/// `<service>` block is being asked for.
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let from = xml.find(&open)? + open.len();
    let to = xml[from..].find(&close)? + from;
    Some(xml[from..to].trim().to_string())
}

/// Which service in a gateway's description does the mapping, and the URL to call
/// it on. `WANIPConnection` for a router on a cable or fibre line,
/// `WANPPPConnection` for one on a dial-up-descended line.
fn wan_service(xml: &str, location: &str) -> Option<(String, String)> {
    for want in [
        "urn:schemas-upnp-org:service:WANIPConnection:1",
        "urn:schemas-upnp-org:service:WANPPPConnection:1",
        "urn:schemas-upnp-org:service:WANIPConnection:2",
    ] {
        // Every `<service>` block holds its type and its control URL together,
        // so the block around the type is the block to read the URL out of.
        let mut at = 0;
        while let Some(found) = xml[at..].find(want) {
            let here = at + found;
            let start = xml[..here].rfind("<service>").unwrap_or(0);
            let end = xml[here..]
                .find("</service>")
                .map(|e| here + e)
                .unwrap_or(xml.len());
            if let Some(control) = tag(&xml[start..end], "controlURL") {
                return Some((want.to_string(), join(location, &control)));
            }
            at = end.max(here + want.len());
        }
    }
    None
}

/// A control URL in a description may be absolute or rooted at the device, so it
/// is joined onto the description's own scheme and host.
fn join(base: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    let root = match base.find("://") {
        Some(i) => match base[i + 3..].find('/') {
            Some(j) => &base[..i + 3 + j],
            None => base,
        },
        None => base,
    };
    if path.starts_with('/') {
        format!("{root}{path}")
    } else {
        format!("{root}/{path}")
    }
}

fn upnp_add(
    control: &str,
    service: &str,
    port: u16,
    local: Ipv4Addr,
    lifetime: u32,
) -> Result<(), String> {
    let body = format!(
        "<NewRemoteHost></NewRemoteHost>\
         <NewExternalPort>{port}</NewExternalPort>\
         <NewProtocol>TCP</NewProtocol>\
         <NewInternalPort>{port}</NewInternalPort>\
         <NewInternalClient>{local}</NewInternalClient>\
         <NewEnabled>1</NewEnabled>\
         <NewPortMappingDescription>Moonstone</NewPortMappingDescription>\
         <NewLeaseDuration>{lifetime}</NewLeaseDuration>"
    );
    let answer = soap(control, service, "AddPortMapping", &body)?;
    if answer.contains("UPnPError") {
        let code = tag(&answer, "errorCode").unwrap_or_default();
        // 725 is a router that only does permanent mappings, which is a yes to
        // the part that matters: ask again without a lease.
        if code == "725" {
            let body = body.replace(
                &format!("<NewLeaseDuration>{lifetime}</NewLeaseDuration>"),
                "<NewLeaseDuration>0</NewLeaseDuration>",
            );
            let again = soap(control, service, "AddPortMapping", &body)?;
            if !again.contains("UPnPError") {
                return Ok(());
            }
        }
        return Err(format!("the router refused the mapping, error {code}"));
    }
    Ok(())
}

fn upnp_delete(control: &str, service: &str, port: u16) -> Result<(), String> {
    let body = format!(
        "<NewRemoteHost></NewRemoteHost>\
         <NewExternalPort>{port}</NewExternalPort>\
         <NewProtocol>TCP</NewProtocol>"
    );
    soap(control, service, "DeletePortMapping", &body).map(|_| ())
}

fn upnp_external(control: &str, service: &str) -> Option<IpAddr> {
    let answer = soap(control, service, "GetExternalIPAddress", "").ok()?;
    let ip = tag(&answer, "NewExternalIPAddress")?;
    ip.parse().ok()
}

/// One SOAP call over plain HTTP. UPnP is not encrypted and is not routable, so
/// there is no TLS to arrange and no dependency to add.
fn soap(control: &str, service: &str, action: &str, body: &str) -> Result<String, String> {
    let envelope = format!(
        "<?xml version=\"1.0\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:{action} xmlns:u=\"{service}\">{body}</u:{action}></s:Body>\
         </s:Envelope>"
    );
    let (host, path) = split_url(control)?;
    let head = format!(
        "POST {path} HTTP/1.1\r\n\
         HOST: {host}\r\n\
         CONTENT-TYPE: text/xml; charset=\"utf-8\"\r\n\
         CONTENT-LENGTH: {}\r\n\
         SOAPACTION: \"{service}#{action}\"\r\n\
         CONNECTION: close\r\n\r\n",
        envelope.len()
    );
    http(&host, &format!("{head}{envelope}"))
}

fn http_get(url: &str) -> Result<String, String> {
    let (host, path) = split_url(url)?;
    let req =
        format!("GET {path} HTTP/1.1\r\nHOST: {host}\r\nCONNECTION: close\r\nACCEPT: */*\r\n\r\n");
    http(&host, &req)
}

/// Host and path out of a `http://host:port/path` URL, without a URL crate for
/// the two shapes a router ever produces.
fn split_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("{url} is not an http url"))?;
    match rest.find('/') {
        Some(i) => Ok((rest[..i].to_string(), rest[i..].to_string())),
        None => Ok((rest.to_string(), "/".to_string())),
    }
}

/// Write a request, read the body. Short timeouts throughout: this is a device on
/// the same network, and a router that has not answered in a second and a half is
/// a router that is not going to.
fn http(host: &str, request: &str) -> Result<String, String> {
    let addr: SocketAddr = host
        .to_socket_addrs_one()
        .ok_or_else(|| format!("{host} is not an address"))?;
    let mut sock =
        TcpStream::connect_timeout(&addr, PATIENCE).map_err(|e| format!("{host}: {e}"))?;
    sock.set_read_timeout(Some(PATIENCE))
        .map_err(|e| e.to_string())?;
    sock.set_write_timeout(Some(PATIENCE))
        .map_err(|e| e.to_string())?;
    sock.write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    // Read to the end, or to a sane cap: a device description is a few
    // kilobytes, and nothing on this path should be a megabyte.
    let mut chunk = [0u8; 4096];
    while raw.len() < 256 * 1024 {
        match sock.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            // A timeout with a body already read is a device that did not close;
            // what it sent is still the answer.
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&raw).to_string();
    match text.find("\r\n\r\n") {
        Some(i) => Ok(text[i + 4..].to_string()),
        None => Ok(text),
    }
}

/// One address out of a `host:port` or bare host, defaulting to 80.
trait OneAddr {
    fn to_socket_addrs_one(&self) -> Option<SocketAddr>;
}

impl OneAddr for str {
    fn to_socket_addrs_one(&self) -> Option<SocketAddr> {
        use std::net::ToSocketAddrs;
        let with_port = if self.contains(':') {
            self.to_string()
        } else {
            format!("{self}:80")
        };
        with_port.to_socket_addrs().ok()?.next()
    }
}

// ------------------------------------------------------------------- asking

/// Asking in the background.
///
/// SSDP waits two seconds for routers to trickle in, and a game loop cannot
/// stop for that. [`Opener::start`] does the asking on a thread of its own and
/// [`Opener::ready`] hands the answer over when there is one, so the lobby
/// screen is up and drawable from the first frame and fills the address in when
/// the router answers.
///
/// This is the only thread in the crate, and it touches nothing but the socket.
pub struct Opener {
    rx: std::sync::mpsc::Receiver<Mapping>,
    got: Option<Mapping>,
}

impl Opener {
    pub fn start(port: u16) -> Opener {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(open_port(port));
        });
        Opener { rx, got: None }
    }

    /// The answer, once the router has given one. Keeps it, so this can be
    /// called every frame.
    pub fn ready(&mut self) -> Option<&Mapping> {
        if self.got.is_none() {
            if let Ok(m) = self.rx.try_recv() {
                self.got = Some(m);
            }
        }
        self.got.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_url_is_joined_onto_the_description_it_came_from() {
        let base = "http://192.168.1.1:5000/rootDesc.xml";
        assert_eq!(
            join(base, "/ctl/IPConn"),
            "http://192.168.1.1:5000/ctl/IPConn"
        );
        assert_eq!(
            join(base, "ctl/IPConn"),
            "http://192.168.1.1:5000/ctl/IPConn"
        );
        assert_eq!(
            join(base, "http://10.0.0.1/x"),
            "http://10.0.0.1/x",
            "an absolute one is left alone"
        );
    }

    #[test]
    fn a_url_splits_into_a_host_and_a_path() {
        assert_eq!(
            split_url("http://192.168.0.1:49152/ctl").unwrap(),
            ("192.168.0.1:49152".to_string(), "/ctl".to_string())
        );
        assert_eq!(
            split_url("http://192.168.0.1").unwrap(),
            ("192.168.0.1".to_string(), "/".to_string())
        );
        assert!(split_url("ftp://nope").is_err());
    }

    #[test]
    fn a_header_is_found_however_it_is_spelled() {
        let reply = "HTTP/1.1 200 OK\r\nLOCATION: http://a/b.xml\r\nST: x\r\n\r\n";
        assert_eq!(header(reply, "location").unwrap(), "http://a/b.xml");
        assert_eq!(header(reply, "LoCaTiOn").unwrap(), "http://a/b.xml");
        assert_eq!(header(reply, "server"), None);
    }

    /// The description of a real router: two services, and the mapping is done by
    /// the second of them. Picking the block around the service type rather than
    /// the first `controlURL` in the file is what gets this right.
    #[test]
    fn the_connection_service_is_found_in_a_two_service_description() {
        let xml = "<root><device><serviceList>\
            <service>\
              <serviceType>urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1</serviceType>\
              <controlURL>/ctl/CommonIfCfg</controlURL>\
            </service>\
            <service>\
              <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
              <controlURL>/ctl/IPConn</controlURL>\
            </service>\
            </serviceList></device></root>";
        let (service, control) = wan_service(xml, "http://192.168.1.1:5000/desc.xml").unwrap();
        assert_eq!(service, "urn:schemas-upnp-org:service:WANIPConnection:1");
        assert_eq!(control, "http://192.168.1.1:5000/ctl/IPConn");
    }

    #[test]
    fn a_ppp_router_is_found_too_and_a_silent_one_is_not() {
        let xml = "<service>\
            <serviceType>urn:schemas-upnp-org:service:WANPPPConnection:1</serviceType>\
            <controlURL>/upnp/control/WANPPPConn1</controlURL></service>";
        let (service, control) = wan_service(xml, "http://10.0.0.138:80/igd.xml").unwrap();
        assert!(service.ends_with("WANPPPConnection:1"));
        assert_eq!(control, "http://10.0.0.138:80/upnp/control/WANPPPConn1");
        assert_eq!(wan_service("<root></root>", "http://a/b"), None);
    }

    #[test]
    fn a_soap_answer_is_read_for_the_one_field_that_matters() {
        let answer = "<?xml version=\"1.0\"?><s:Envelope><s:Body>\
            <u:GetExternalIPAddressResponse>\
            <NewExternalIPAddress>203.0.113.7</NewExternalIPAddress>\
            </u:GetExternalIPAddressResponse></s:Body></s:Envelope>";
        assert_eq!(tag(answer, "NewExternalIPAddress").unwrap(), "203.0.113.7");
        let err = "<s:Fault><UPnPError><errorCode>725</errorCode></UPnPError></s:Fault>";
        assert_eq!(tag(err, "errorCode").unwrap(), "725");
        assert_eq!(tag(err, "NewExternalIPAddress"), None);
    }

    /// Every result code a router can give has words of its own, because "it did
    /// not work" is not something a person can act on.
    #[test]
    fn every_natpmp_refusal_says_what_to_do_about_it() {
        for code in 1..=5u16 {
            let said = natpmp_reason(code);
            assert!(said.contains("router"), "{code}: {said}");
        }
        assert!(natpmp_reason(99).contains("99"));
    }

    #[test]
    fn the_gateway_is_guessed_from_this_machines_own_address() {
        let g = gateways(Some(Ipv4Addr::new(192, 168, 7, 42)));
        assert_eq!(g[0], Ipv4Addr::new(192, 168, 7, 1));
        assert_eq!(g[1], Ipv4Addr::new(192, 168, 7, 254));
        assert!(gateways(None).is_empty(), "nothing to guess from");
    }

    /// A host with no router and no answer still gets a mapping value with
    /// something to show the person, which is what keeps the lobby screen from
    /// having to know any of this.
    #[test]
    fn an_unanswered_mapping_still_says_something_useful() {
        let m = Mapping {
            port: DEFAULT_PORT_FOR_TEST,
            external: None,
            local: Some(Ipv4Addr::new(192, 168, 1, 20)),
            how: Reach::Unknown,
            note: "nothing answered".into(),
            undo: Undo::Nothing,
        };
        assert!(!m.how.opened());
        assert_eq!(m.address(), "192.168.1.20:19910");
        // And taking down a mapping that was never made does nothing at all.
        m.close();
        m.renew();
    }

    #[test]
    fn an_opened_mapping_reads_out_the_address_outside() {
        let m = Mapping {
            port: DEFAULT_PORT_FOR_TEST,
            external: Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))),
            local: Some(Ipv4Addr::new(192, 168, 1, 20)),
            how: Reach::NatPmp,
            note: String::new(),
            undo: Undo::Nothing,
        };
        assert!(m.how.opened());
        assert_eq!(m.address(), "203.0.113.7:19910");
    }

    const DEFAULT_PORT_FOR_TEST: u16 = crate::proto::DEFAULT_PORT;
}
