# The list server

`henge-list` is the small program that lets people find a game without being read
an address. It is **ours**, like all of `henge-net`: the original has no network
code at all.

It does two things and holds nothing:

- **The list.** Hosts announce their lobby and refresh every fifteen seconds;
  the game's *Open games* page asks for the list. An entry is a name, an address,
  a head count, whether a password is wanted, and which build opened it. An entry
  whose host has not refreshed for forty five seconds is dropped (`--stale`
  changes that), and the server answers every refresh, which is also what keeps
  the connection warm through a carrier's NAT. A host that loses the connection
  puts its listing back a few seconds later on its own.
- **The relay.** A host whose router will not open a port keeps one outbound
  connection to the server. When somebody joins, the server asks that host for a
  second connection, glues the two sockets together and copies bytes between them
  until one of them goes. It never parses what it carries.

It holds no game state, it cannot join a game, it never sees a password, and
losing it costs the browse and the relay and nothing else. A game found by typing
an address plays exactly the same.

## The probe, which is the part worth having

An announcement arrives on a TCP connection, so the server already knows the
host's real public address: it is the source address of that connection, and no
router has to be believed about it. The server then tries to connect **back** to
the announced port.

That is the only honest answer to "can my friends reach me". Not what UPnP
claimed. Not what the router's status page says. Whether something outside the
house actually got in. A host that comes back unreachable is offered the relay,
and one that comes back reachable is told there is nothing to carry.

This only measures the right thing if the server is on the **far side of the
host's router**. A list server on the same home network as its players answers a
different question and answers it wrongly.

## Where to run it

It needs one thing: **a real public IP and one inbound TCP port**. That rules
some places out.

| Where | List | Probe | Relay |
|---|---|---|---|
| A VPS with a public IP | yes | yes | yes |
| A home line with a forwarded port | yes | yes | yes |
| Starlink, or any carrier-grade NAT | **no** | no | no |
| Shared web hosting (PHP only) | not as it stands | usually blocked | no |

**Starlink gives no inbound address at all.** A list server behind it cannot be
reached by anybody. It also means every *player* on Starlink is unreachable, so
Starlink players will always be carried by the relay, which is another reason the
relay has to live somewhere with a real address.

**Shared web hosting** normally will not run a long-lived process listening on a
port of its own. The list could be done over plain HTTP by a small script there;
the relay could not, and the probe usually could not either, because outbound
connections to high ports are often blocked.

A small VPS is the answer that makes all three work, and it is a few euros a
month. Anything with 256 MB of memory is plenty: the program holds a few hundred
entries in memory and writes nothing to disk.

## Running it

```text
henge-list                     # port 19911, relay on
henge-list --port 25000        # somewhere else
henge-list --no-relay          # hold the list only, and carry nobody's game
henge-list --quiet             # no line per event
```

Forward that one TCP port to the machine and nothing else. There is no database,
no configuration file and nothing on disk: everything it knows is in memory, and
the hosts rebuild it themselves within one refresh of a restart.

### As a service on Ubuntu

```ini
# /etc/systemd/system/henge-list.service
[Unit]
Description=Moonstone game list
After=network-online.target

[Service]
ExecStart=/usr/local/bin/henge-list --quiet
Restart=always
RestartSec=5
# It needs no files, no home and no privileges of any kind.
DynamicUser=yes
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
RestrictAddressFamilies=AF_INET AF_INET6
MemoryMax=256M

[Install]
WantedBy=multi-user.target
```

```sh
cargo build --release -p henge-net --bin henge-list
sudo install -m755 target/release/henge-list /usr/local/bin/
sudo systemctl enable --now henge-list
sudo ufw allow 19911/tcp
```

## When everybody is behind a carrier's NAT

Starlink, and mobile broadband, and a good deal of rural fibre, put every
customer behind carrier-grade NAT. Nobody behind one can accept an incoming
connection, so if **both** players are on such a line there is no address either
of them could publish that the other could dial. Exchanging connection details
through a web page does not help: a URL can carry an address, it cannot carry
packets, and shared hosting runs a script per request so it cannot hold a socket
either.

In that case the relay is not a fallback, it is the path every game takes, and
two things follow:

- **The server has to be near the players, not free.** Both ends' traffic goes to
  it and back, so its distance is added to every keypress. See the region note
  below.
- **The input delay has to be measured.** The host pings each guest once a second
  while the lobby is open and picks the delay from the worst round trip when the
  game begins (`Host::suggested_delay`). The measured figure is drawn beside each
  name in the lobby, so a bad line shows up there rather than a minute into a
  fight.

The relay costs nothing else: the game is unchanged, the fingerprint check still
runs, and the server still never parses a byte of what it carries.

## On Google Cloud

Yes, and it is a reasonable place for it. Two things to know first.

**It has to be Compute Engine, a plain virtual machine.** Cloud Run and App
Engine will not do: both are request-scoped HTTP, neither will hold a socket open
between requests, and the relay's whole job is holding two sockets open for the
length of a game. Cloud Run also gives you no inbound TCP port of your own.

**The region matters more than the price, because of the relay.** The list does
not care where the server is: it is one question when somebody opens the menu.
The relay carries every byte of a game, so a relayed game between two people in
Norway with the server in Iowa pays the trip to Iowa and back on top of the trip
between them. Roughly thirty milliseconds becomes roughly a hundred and thirty,
and because this is lockstep, that difference **is** the input delay: see
`henge_net::lockstep::delay_for_rtt`.

So:

- **Only the list, or friends who can all host directly:** the always-free
  `e2-micro` is fine, and it only exists in `us-west1`, `us-central1` and
  `us-east1`.
- **The relay, for players in Europe:** put it in `europe-north1` (Finland) or
  `europe-west4` (Netherlands). That is a paid `e2-micro` and it is small money,
  and it is the difference between a fight that feels right and one that does
  not.

**If everyone is on Starlink, every game is relayed**, so the server's distance is
added to every keypress twice: once from the host to it, once from it to the
guest. Working it through for players in Norway:

| Server | Peer to peer round trip, relayed | Input delay it asks for |
|---|---|---|
| `europe-north1`, Finland | roughly 120 ms | about 8 ticks, 115 ms |
| `us-east1`, South Carolina | roughly 250 ms | about 18 ticks, 257 ms |
| `us-west1`, Oregon | roughly 330 ms | about 23 ticks, 330 ms |

The map and the towns do not care. A fight does: that is the gap between pressing
and swinging.

**Of the three free regions, `us-east1` is the closest to Europe** by a wide
margin, so if the free tier is the choice, that is the one to take.

### What the near server costs

List prices in USD, for `europe-north1`. Checked in September 2026, and worth
re-checking: cloud pricing moves.

| | Rate | A month, always on |
|---|---|---|
| `e2-micro`, on demand | $0.0092/hour | $6.73 |
| External IPv4, attached to a running VM | $0.004/hour | $2.92 |
| 30 GB standard persistent disk | | about $1.50 |
| **Always on** | | **about $11** |

**For playtesting, do not leave it on.** Compute and the attached address are
billed by the second, so an evening's session costs about five cents:

```text
(0.0092 + 0.004) x 4 hours = $0.053
```

Ten evenings a month plus the disk is **about a dollar**. Shrink the disk to
10 GB and it is less.

Two traps in that:

- **Do not reserve a static IP if the machine is going to be off.** An idle
  reserved address is $0.01/hour, which is $7.30 a month: more than the machine.
  Use an ephemeral address and accept that it changes each time the machine
  starts. That costs one edited line in `henge-list.txt` per session, which is
  the whole reason that file exists.
- A **Spot** `e2-micro` is $0.0026/hour, but Google may take it back at any
  moment, and a relay that vanishes ends the game it is carrying.

For a machine that is simply always up, a flat-rate box elsewhere is cheaper than
Google's Finland region and needs no watching: a Hetzner CX22 in Helsinki is
around $4.59 a month with the address and the disk included, which is closer to
Norway than anything in the free tier and less than half the always-on price
above.

None of this is guesswork you have to accept. The lobby measures the real round
trip once a second and draws it beside each name, so the number above is checkable
before anybody presses Begin, and moving the server later is one line in
`henge-list.txt` rather than a rebuild.

Reported free-tier terms at the time of writing, worth checking against Google's
own page before relying on them: one `e2-micro` in those three US regions, a
30 GB standard persistent disk, and 200 GiB a month of egress to most
destinations. A carried game uses under 7 KB/s each way, so 200 GiB is thousands
of hours; the free tier's constraint is the region, not the bandwidth.

### Making the machine

1. **Compute Engine → VM instances → Create instance.**
2. Machine type **E2 → e2-micro**. Region as above.
3. Boot disk **Ubuntu 22.04 or 24.04 LTS**, 30 GB, and the disk type must be
   **standard persistent disk**: the free tier covers 30 GB-months of `pd-standard`
   and the console's default of `pd-balanced` is charged.
4. Under the network interface, set the external IPv4 to a **reserved static
   address**. An ephemeral one changes when the machine stops, and the address is
   what the game is pointed at. A static address is free while it is attached to
   a running machine and charged when it is reserved and idle, so release it if
   you delete the machine.
5. **Firewall.** Do not use the "Allow HTTP traffic" tick boxes: those are ports
   80 and 443. Make a rule of your own: VPC network → Firewall → Create, ingress,
   source `0.0.0.0/0`, protocol `tcp:19911`, target tag `henge-list`, and put
   that tag on the machine.

### Putting it there

Either copy the built binary up:

```sh
gcloud compute scp target/release/henge-list NAME:~ --zone ZONE
```

or build it on the machine, which needs nothing from this repository but the two
crates the server uses:

```sh
sudo apt update && sudo apt install -y build-essential git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
git clone <this repository> && cd henge
cargo build --release -p henge-net --bin henge-list
```

`e2-micro` has a gigabyte of memory, which is enough, but add a swap file first
if the build is killed:

```sh
sudo fallocate -l 2G /swapfile && sudo chmod 600 /swapfile
sudo mkswap /swapfile && sudo swapon /swapfile
```

Then install it as a service with the unit above, and check it from your own
machine before trusting it:

```sh
nc -vz THE.ADDRESS 19911
```

### Updating one that is already running

The server and the game are versioned together (`henge_net::list::LIST_PROTOCOL`),
so a change to either usually wants the other. On the machine:

```sh
cd henge && git pull
cargo build --release -p henge-net --bin henge-list
sudo install -m755 target/release/henge-list /usr/local/bin/
sudo systemctl restart henge-list
journalctl -u henge-list -f
```

Restarting drops the open lobbies' control connections. Nothing is lost that
matters: a game already being played is unaffected, and every host puts its
listing back within a few seconds of noticing.

### Watching what it costs

Set a **budget alert** on the project, at whatever number would annoy you, before
you leave it running. The relay's limits mean this program cannot run up a large
bill on its own, but a cloud account with nothing watching it is how people find
out about something else they left switched on.

## Pointing the game at it

Three places, in order of preference:

1. `--list <address>` on the command line.
2. One line in `henge-list.txt` beside the game. A `#` starts a comment.
3. `LIST_SERVER` in `crates/henge-desktop/src/main.rs`, which is what a build
   ships with. While it is empty the *Open games* page says so rather than
   pretending to look.

**This build ships with `34.51.244.53`**: an `e2-micro` named `henge-list` in
`europe-north2-a`, Stockholm, which is the nearest Google region to the people
playing.

That address is **ephemeral**, which is what keeps it free while the machine is
off. It stays put for as long as the machine keeps running and changes when the
machine is stopped and started again. If that happens, put the new one in
`henge-list.txt` rather than rebuilding: nothing else in the game knows the
number.

A bare address gets port 19911, so `henge-list.txt` holding `203.0.113.7` is
enough.

## What it will not do

Every limit is a refusal with a reason rather than a silent drop, because the
address of this program will eventually be known to people who were not invited.

- 500 games at once, and 8 from any one address.
- 64 carried games at once. A relay is only given to a game that is on the list
  **and was measured as unreachable**: a host that can be reached and asks to be
  carried anyway is told there is nothing to carry.
- 20 KB/s through any one carried game, and 1 MB/s through all of them together.
  A four-seat game at the retrace rate actually uses under seven.
- A connection that says nothing for ninety seconds is closed.

## Passwords

A host may ask for a word. **The word never leaves the host machine**: the list
carries only the fact that a game wants one, and the check happens on the hello,
between the two people playing. So the list server cannot be made to hand out
entry to a game, because it does not know how, and neither can whoever is running
it.

The game never draws a password either: the row shows stars.

## Testing it

`crates/henge-net/tests/list_server.rs` starts the real program on a port the
system picks and drives it over a socket: announce, browse, withdraw, the version
filter, the password refusals, a relayed game carrying an ordinary game message
both ways, a reachable host being refused a relay, and `--no-relay` meaning what
it says.
