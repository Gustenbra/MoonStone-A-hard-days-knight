# henge

A reimplementation of *Moonstone: A Hard Days Knight* (Amiga 1991, DOS 1992) in Rust.

This is an **engine**. It ships no artwork, no sound and no game data. To play it you
need your own copy of the original, which the engine reads and converts locally.

> **Status: early, and private while the licence is undecided.**
> Combat and overworld travel work, and the road is walked by the original's own
> creatures, animated from its own scripts. Their behaviour is still a plain
> opponent's; `docs/BUILD_ORDER.md` says what is and is not there.

## What works

- 57 combat arenas across four families, with real terrain, scenery and depth sorting
- An overworld you travel across, with a day cycle and ambushes, laid out by the
  original's own two map tables: which of the four kinds of ground each 8x8 block
  is, and how hard that block is to cross. Nothing on the map is impassable; the
  forest and the marsh are half speed and the mountain spine a quarter, and a
  step the ground refuses still costs you the day
- Where a fight happens is decided by the terrain you are standing on, and which
  of that family's eight arenas you get is the family's own turn counter, the way
  the original rotates through them
- **Places to go**: Highwood, Waterdeep, a healer in the woods and the stone
  circle, each on the landmark the map already draws. Walk onto one and it opens
  with its own screen and menu
- **Gold and goods**: coin comes off whoever you put down, and the merchants in
  both towns are open. Flasks and draughts are bought, carried and drunk; a
  healer in the woods still charges only days, and one inside the walls wants
  coin as well. What you carry can leave you too, to a cutpurse on the road
- **A title screen, and a knight to choose.** The game opens on its own wordmark,
  which turned out to be the last three frames of the bold font's bank, over an
  option list that is the original's: one to four players, gore on or off,
  practice combat or the moon quest. Leave it alone and it starts showing you the
  intro plates. The four knights are Sir Banner, Sir Dwain, Sir Balain and Sir
  Gunther, blue, gold, emerald and red because the executable's own colour table
  says so, and each begins in his own corner of the map
- **A status panel with something on it.** Strength, constitution and endurance,
  life points, daggers, gold, experience, health, the sword in your hand and the
  armour on your back, at the coordinates the original's own routine places them
  and drawn with its own icons. One plate per fighter along the bottom of an
  arena, and the whole sheet on a key
- **Up to four fighters in one arena**, the original's player count, in any mix of
  people at the keyboard and opponents, each in their own colour
- Movement, committed attacks, positional hit resolution, damage, death
- **The bestiary.** Troll, trogg with axe, hammer or spear, ratmen, mudmen, beast,
  Balok, demon and dragon, each running the original's own animation scripts on
  its own sprite banks, with the hit points, blows and ranges the original's own
  set-up routines give them. Ambushes on the road field them by the ground you
  are standing on. The demon's screen border and the dragon's set piece are not
  built and are not faked
- A deterministic simulation with a state fingerprint, proven by test to agree tick for
  tick across independent runs and across a save/restore
- Sound: swings, blows, deaths and footfalls
- 178 tests, all of it verifiable headlessly with no display or sound card

## Running it

You need [Rust](https://rustup.rs) and your own copy of the original game's data
files. Put the folder holding `KN1.OB`, `MAP.CMP` and the rest next to this one, so
that both sit side by side, then:

```
play.bat                 Windows
./play.sh                Linux and macOS
```

That reads the original files once, builds, and starts the game. If your copy of the
original lives somewhere else, say where:

```
play.bat --data "C:\path\to\Moonstone"
./play.sh --data /path/to/Moonstone
```

Pass `--rebake` after changing anything about how the data is read. Anything else you
pass goes straight to the game, so `play.bat --start select` opens on character select.

The long way, if you would rather drive it yourself:

```sh
cargo run --release -p henge-formats --bin henge-bake -- "path/to/Moonstone" packs/reference
cargo run --release
```

It opens on the title screen. Up and down move the highlight, left and right change
the setting on the row you are on, and space takes it. The moon quest goes to the
select screen, where left and right pick a knight and space takes him.

Walking onto a town, the healer or the stone circle opens it. In a place, up and
down move the highlight, space takes the option, and Tab is always a way back out.

Player one uses the arrows and space. Player two uses `WASD` and `F`. `1` and `2` set
how many people are playing; the remaining seats are filled by opponents. `C` shows
the character sheet. Tab switches between the overworld and the arena, `[` and `]`
change arena, `,` and `.` change which creature fills the opponents' seats, `R`
restarts the bout, escape quits.

The knight's and the creatures' animations are the original's own scripts, read out of
the unpacked `MAIN.EXE`. The baker looks for `research/main.final.bin` and
`research/symbols.json`, which `tools/symbolmap.py` writes (`docs/REVERSING.md`), and
bakes a knight with no animation and no creatures at all if they are missing, saying so.

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

`--foe <actor>` fills the opponents' seats with one creature, which is how each of the
bestiary is captured and traced on its own:

```sh
henge --screenshot troll.png 40 0 --start arena --foe troll --walk
henge --trace 600 0 --start arena --knight 0 --foe balok
```

Both accept a scripted run, which is how a screen you have to walk to and a menu
you have to drive get reached with no keyboard and no display:

```sh
henge --trace 400 0 --goto 158,102 --input "....d.d.s"   # walk there, then choose
henge --screenshot out.png 6 0 --at healer --hurt 30 --input "..s"
henge --trace 12 0 --start title --input "ldddss"        # two players, quest, choose
henge --screenshot out.png 120 0 --start arena --knight 2 --fight --sheet
```

`--goto x,y` steers across the map a step a tick, and swings back at whatever
ambushes it on the way. `--at <place>` starts inside a place, `--hurt <hp>` starts
the run already wounded so a healer has something to do, `--gold <n>` starts it
with coin so a stall can be reached without first winning the fights that pay for
it, and `--input` feeds one key press per tick: `u`/`d` move the highlight, `s`
takes the option, `hjkl` walk or move the highlight sideways, `.` waits. Presses
arrive through the same edge-detected path the keyboard uses, so a script
exercises the game rather than a stub.

`--start <title|select|map|arena>` says which screen to open on. It defaults to the
map, so every recipe written before the shell existed still does what it did; the
window opens on the title. `--knight <0..3>` begins a run as one of the four
without going through the select screen, and `--sheet` holds the character sheet
open over whatever is drawn.

`--trace` runs the simulation and prints every state change, which is how combat,
travel and what a town does to a run are verified as behaviour rather than by
looking at screenshots:

```
    0  MAP     day 1   at 146,115 hp 100  gold    0 -   won 0  fought 0        on forest
   21  MAP     day 1   at 157,115 hp 100  gold    0 -   won 0  fought 0        on swamp
   30  COMBAT  sw1   swamp    knight Attack  20 @ 56,117 | mudmen Walk    30 @109,111
   50  COMBAT  sw1   swamp    knight Attack  20 @ 69,117 | mudmen Attack  25 @ 99,111
```

The map line carries the position because the ground is a table now: `on swamp` is
`MapType` under the traveller's feet, not a guess at the colour of the picture. The
combat line carries the arena's own name because which of a family's eight you get
is a rotation, and watching it go round is how that is checked; and each fighter's
actor, because what waits in a swamp is a mudman and that is worth seeing too.

```
    0  PLACE   day 1  hp  30  gold  100  -         The Healer  > Tend my wounds
    2  PLACE   day 4  hp 100  gold  100  -         The Healer  > Tend my wounds  Rest well...
```

```
    0  PLACE   day 1  hp  40  gold  100  -         Merchant  > Flask of healing [25]
    1  PLACE   day 1  hp  40  gold   75  potion    Merchant  > Flask of healing [25]  A fair trade.
    6  PLACE   day 1  hp  80  gold   50  potion    Merchant  > Drink a flask          You drain it.
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

## Gold, goods and a pack

A run carries a purse and a pack, both of them ordinary simulation state that
serializes with everything else. Coin comes off the fallen: what a fighter is
worth is a `bounty` on its actor definition, so a troll can be worth more than a
rat without a line of Rust changing. A purse is picked up after a fight, which
means only a winner still on their feet collects one.

Items are data like everything else. Each one names a price and a virtue, and a
menu line takes its price from the goods rather than repeating it in the label,
so the two can never drift apart:

```json
"potion": { "name": "Flask of healing", "price": 25, "consumed": true,
            "virtue": { "does": "heal", "health": 40 } }
```

**Things leave the pack as well as entering it.** The original names a routine
`TAKEFROMKNIGHT`, so losing is a real operation rather than an afterthought on a
list that only ever grows: a flask drunk is a flask gone, and a cutpurse on the
road takes a share of the purse or, failing that, something out of the pack. Who
gets robbed is decided by a seeded roll carried in the run, so two machines
walking the same road are robbed on the same step.

A stall is its own place, marked `hidden` so walking can never stumble into it,
reached through the town's own menu and leaving back into it. That keeps a
town's front door short and gives the shop a box wide enough for goods and their
prices.

A bout takes **one `Intent` per fighter and cannot tell where they came from**. A
keyboard, an opponent and a network packet are interchangeable, which is the seam
networked play plugs into without touching combat. See `docs/ROADMAP.md`.

## Documentation

- [`docs/FORMATS.md`](docs/FORMATS.md): the original's file formats, fully documented
- [`docs/REVERSING.md`](docs/REVERSING.md): unpacking the executable, and what is still unknown
- [`docs/BUILD_ORDER.md`](docs/BUILD_ORDER.md): every item once, in the order to do it
- [`docs/COMPLETE.md`](docs/COMPLETE.md): everything left to build, against the original's own function names
- [`docs/ROADMAP.md`](docs/ROADMAP.md): what is next, including the multiplayer architecture
- [`CONTRIBUTING.md`](CONTRIBUTING.md): including the rules that keep the simulation networkable

## Licence

**Not yet chosen.** Until it is, all rights are reserved and contributions cannot be
accepted, because there would be nothing to license them under.

No code was taken from any other reimplementation. Every decoder here was written from
scratch against the raw data.
