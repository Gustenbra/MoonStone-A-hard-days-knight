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
- Knight against knight: movement, committed attacks, positional hit resolution, damage,
  death
- 29 tests, all of it verifiable headlessly with no display

## Running it

Two commands, once:

```sh
cargo run --release -p henge-formats --bin henge-bake -- "path/to/Moonstone" packs/reference
cargo run --release
```

Arrows travel and fight, space attacks, tab switches between the overworld and the
arena, `[` and `]` change arena, `R` restarts the bout, escape quits.

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

`--trace` runs the simulation and prints every state change, which is how combat and
travel are verified as behaviour rather than by looking at screenshots:

```
    0  MAP     day 1   step    1  at  151, 120  on forest
   11  COMBAT  forest   player Idle   hp 100   foe Idle   hp 100
   55  COMBAT  forest   player Hurt   hp  75   foe Attack hp 100
  261  COMBAT  forest   player Dead   hp   0   foe Attack hp 100
  381  MAP     day 1   step   12  at  162, 120  on forest
```

## How it is put together

| Crate | Purpose |
|---|---|
| `henge-core` | The simulation: combat, animation, arenas, overworld. One dependency, serde. No rendering, no I/O, no platform. |
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

## Documentation

- [`docs/FORMATS.md`](docs/FORMATS.md) — the original's file formats, fully documented
- [`docs/REVERSING.md`](docs/REVERSING.md) — unpacking the executable, and what is still unknown
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — what is next, including the multiplayer architecture
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — including the rules that keep the simulation networkable

## Licence

**Not yet chosen.** Until it is, all rights are reserved and contributions cannot be
accepted, because there would be nothing to license them under.

No code was taken from any other reimplementation. Every decoder here was written from
scratch against the raw data.
