# henge

A reimplementation of *Moonstone: A Hard Days Knight* (Amiga 1991, DOS 1992) in Rust.

This is an **engine**. It ships no artwork, no sound and no game data. To play it you
need your own copy of the original, which the engine reads and converts locally.

> **Status: early, and private while the licence is undecided.**
> Combat and overworld travel work. Creatures do not animate yet, for reasons in
> `docs/REVERSING.md`.

## What works

- 57 combat arenas across four families, with real terrain, scenery and depth sorting
- An overworld you travel across, with a day cycle and ambushes
- Where a fight happens is decided by the terrain you are standing on
- **Places to go**: Highwood, Waterdeep, a healer in the woods and the stone
  circle, each on the landmark the map already draws. Walk onto one and it opens
  with its own screen and menu. The healer mends you, and the price is days
- **Up to four fighters in one arena**, the original's player count, in any mix of
  people at the keyboard and opponents, each in their own colour
- Movement, committed attacks, positional hit resolution, damage, death
- A deterministic simulation with a state fingerprint, proven by test to agree tick for
  tick across independent runs and across a save/restore
- Sound: swings, blows, deaths and footfalls
- 58 tests, all of it verifiable headlessly with no display or sound card

## Running it

Two commands, once:

```sh
cargo run --release -p henge-formats --bin henge-bake -- "path/to/Moonstone" packs/reference
cargo run --release
```

Walking onto a town, the healer or the stone circle opens it. In a place, up and
down move the highlight, space takes the option, and Tab is always a way back out.

Player one uses the arrows and space. Player two uses `WASD` and `F`. `1` and `2` set
how many people are playing; the remaining seats are filled by opponents. Tab switches
between the overworld and the arena, `[` and `]` change arena, `R` restarts the bout,
escape quits.

The bake step converts your copy of the game into indexed PNGs, WAVs and JSON. **After
it runs, the engine reads only PNG, WAV and JSON.** It has no knowledge that the original
formats exist; those live entirely in `henge-formats`, which is not linked into a release
build.

## Headless

Nothing about this project requires a display, which is what makes it testable in CI.

```sh
henge --screenshot out.png [ticks] [arena] [--walk] [--fight]
henge --trace [ticks] [arena]
```

Both accept a scripted run, which is how a screen you have to walk to and a menu
you have to drive get reached with no keyboard and no display:

```sh
henge --trace 400 0 --goto 158,102 --input "....d.d.s"   # walk there, then choose
henge --screenshot out.png 6 0 --at healer --hurt 30 --input "..s"
```

`--goto x,y` steers across the map a step a tick, and swings back at whatever
ambushes it on the way. `--at <place>` starts inside a place, `--hurt <hp>` starts
the run already wounded so a healer has something to do, and `--input` feeds one
key press per tick: `u`/`d` move the highlight, `s` takes the option, `hjkl` walk,
`.` waits. Presses arrive through the same edge-detected path the keyboard uses,
so a script exercises the game rather than a stub.

`--trace` runs the simulation and prints every state change, which is how combat,
travel and what a town does to a run are verified as behaviour rather than by
looking at screenshots:

```
    0  MAP     day 1   step    1  at  151, 120  on forest
   11  COMBAT  forest   player Idle   hp 100   foe Idle   hp 100
   55  COMBAT  forest   player Hurt   hp  75   foe Attack hp 100
  261  COMBAT  forest   player Dead   hp   0   foe Attack hp 100
  381  MAP     day 1   step   12  at  162, 120  on forest
```

```
    0  PLACE   day 1   hp  30  The Healer   > Tend my wounds
    2  PLACE   day 4   hp 100  The Healer   > Tend my wounds  Rest well. You are whole again.
```

## How it is put together

| Crate | Purpose |
|---|---|
| `henge-core` | The simulation: combat, animation, arenas, overworld. One dependency, serde. No rendering, no I/O, no platform. |
| `henge-audio` | What to play, and where it comes out, split apart so only the second half is platform specific. |
| `henge-assets` | Logical asset ids resolved through a stack of packs. |
| `henge-formats` | Reads the original's files. Research and baking only; never linked into a release. |
| `henge-desktop` | Window, input, palette framebuffer. Produces the executable. |

`henge-core` being austere is deliberate. It compiles for a server, a browser and a
headless test unchanged, and it is the precondition for networked play. See
`docs/ROADMAP.md`.

### Assets are addressed by name, never by path

`actor.knight`, `sfx.swish`, `palette.forest`. Ids resolve through a stack of packs,
first match wins:

```
packs/original/     original artwork, safe to distribute   <- searched first
packs/reference/    baked from your copy of the game       <- gitignored, never distributed
```

Replacing a sprite is dropping a PNG into `packs/original` and adding one manifest
entry. No code change, no rebuild, and the game stays playable throughout.

Every pack declares its provenance, so the project can always answer whether a build
contains anything derived:

```sh
cargo run --release -p henge-assets --bin henge-pack-status packs
```

## Text

The game can say things now. The fonts decoded from the start, but which glyph draws
which character is a table inside the original executable that is not recovered, so the
order was read off the artwork: A-Z, then a-z, then 0-9, then punctuation. A few ornaments
at the end are unidentified and left unmapped rather than guessed at; an unmapped glyph is
simply never drawn.

The mapping lives in the pack as content, so a replacement font needs no code change.
Glyphs draw as a silhouette in a chosen colour, since a glyph's own shades are legible
over one arena's palette and invisible over the next.

```sh
henge --screenshot out.png 5 0 --say "Day 1|forest|100 of 100"
```

`--say` draws a line over whatever was rendered, which is how the font and its mapping are
checked without hunting for a frame that happens to show the status bar.

`--peaceful` suppresses ambushes. Without it, checking how the map draws anywhere but the
starting corner is impossible: the traveller is killed en route long before arriving, so
half the map could never be looked at.

## Sound

`henge-audio` keeps two things separate. Deciding **what** should be heard is done by
watching the fight, and is pure logic with no platform behind it, so it is tested without
a sound card. Deciding **where it comes out** sits behind a trait, so a browser build or a
headless server replaces only that half.

Cues are derived rather than scripted: a swing is announced when it starts, so the sound
leads the blow; footfalls land on chosen frames of the walk cycle rather than every frame;
and hits come from the simulation itself, so a blow is never unheard and a miss is never
announced.

Building without a sound stack at all is supported and checked in CI:

```sh
cargo build --workspace --no-default-features
```

**No audio device is a normal state, not a failure.** Containers, CI and plenty of
machines have none. The game says so once and plays silently.

## Combat

Positional, not box-on-box. Every attack frame carries a **hit line**, a polyline swept
in the actor's own space, and a swing connects when that line crosses the target's body
*and* their depths line up. That is the shape the original stores for its creatures.

- An attack **commits**: no turning, no second swing, no walking out of it.
- A swing connects **once**, however many of its frames carry a line, but a swing that
  misses keeps offering its line for the rest of the animation.
- Frames carry the fighter through their own `dx`, so a lunge that travels is a property
  of the swing rather than something bolted on beside it.

All of it is data. Retuning the feel of the game is editing JSON, not editing Rust.

A bout takes **one `Intent` per fighter and cannot tell where they came from**. A
keyboard, an opponent and a network packet are interchangeable, which is the seam
networked play plugs into without touching combat. See `docs/ROADMAP.md`.

## Documentation

- [`docs/FORMATS.md`](docs/FORMATS.md) — the original's file formats, fully documented
- [`docs/REVERSING.md`](docs/REVERSING.md) — unpacking the executable, and what is still unknown
- [`docs/BUILD_ORDER.md`](docs/BUILD_ORDER.md) — every item once, in the order to do it
- [`docs/COMPLETE.md`](docs/COMPLETE.md) — everything left to build, against the original's own 334 function names
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — what is next, including the multiplayer architecture
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — including the rules that keep the simulation networkable

## Licence

**Not yet chosen.** Until it is, all rights are reserved and contributions cannot be
accepted, because there would be nothing to license them under.

No code was taken from any other reimplementation. Every decoder here was written from
scratch against the raw data.
