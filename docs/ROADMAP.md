# Roadmap

Ordered by what the project is missing most, not by what is easiest.

## Now

- [ ] **N fighters instead of two.** The arena hardcodes `player` and `foe`. Replace with a
      `Vec<Fighter>` plus an input source per fighter (local device, AI, or remote).
      This is the single highest-leverage change in the codebase: it delivers the
      original's four-player mode *and* it is the exact seam networking plugs into later.
- [ ] **Four knight colours.** `KN1`-`KN5` are separate banks, and the original recoloured
      knights per player with a palette remap (`COLOURENKNIGHT` in the symbol table).
- [ ] **Audio.** All 49 samples are already baked to WAV and sitting unused. Write it behind
      a trait from the start: native on desktop, Web Audio in a browser build.
- [ ] **Text.** `BOLD.F` and `SMALL.FON` decode, but the character lookup table lives in
      `MAIN.EXE` and is not recovered, so glyph order is unknown. Either recover it or map
      it by eye.

## Next

- [ ] Title screen, character select, results
- [ ] Health and inventory persisting between fights, so dying costs something
- [ ] Save and load
- [ ] Web build (wasm), including running the bake client-side so no assets are ever served
- [ ] Settle the arena family pairing question (see FORMATS.md)

## Reverse engineering still open

- [ ] **Multi-part sprite composition.** Creature banks are body parts, not whole poses, so
      no creature can animate until this is decoded. This gates the entire bestiary.
      Data located around `0x0e000` in the unpacked image; field semantics unknown.
- [ ] Symbol name to address mapping in the debug info, which would land `TASKSEQ`,
      `TASKPLACE` and `CALCHIT` exactly and make the above fall out
- [ ] Overworld node graph, if we ever want the original's map rather than ours

---

# Multiplayer

The goal: live play for many people, potentially hundreds in one world, while the
original's four-player mode works today.

## The tension, stated honestly

These two requirements pull in opposite directions.

**Precise 2-4 player fighting** wants deterministic lockstep or rollback: every client
simulates the entire world identically, inputs are exchanged, and mispredictions are
corrected by re-simulating. It is how fighting games feel responsive over a network.
It requires perfect determinism and it does not scale, because every client pays for
every other player.

**Hundreds of players in one world** wants an authoritative server with area-of-interest
culling: each client knows only its neighbourhood, the server owns the truth, and clients
predict locally and reconcile. It scales, and it cannot give you frame-perfect fighting.

You cannot get both from one design. Pretending otherwise is how this kind of project dies.

## The resolution, which this game hands us for free

**Moonstone already separates the overworld from combat**, and those two halves have
exactly the opposite requirements.

| | Overworld | Arena |
|---|---|---|
| Players | hundreds | 2 to 4 |
| Tick rate | low, ~10 Hz | high, 60 Hz |
| Precision | none needed | frame-exact |
| Model | authoritative server, area-of-interest, interpolation | instanced, deterministic, lockstep or rollback |

So: one persistent world where many people travel, and instanced arenas spun up when
players meet. That is a shard-and-instance architecture, it is how this genre actually
works, and the seam falls exactly where the game already has one.

## What has to be true now, or it becomes impossible later

Most of this is already true, which is why the door is still open. Keeping it that way
costs nothing today and cannot be retrofitted cheaply.

1. **The simulation is deterministic.** Integers only, no floating point anywhere in
   `henge-core`. No iteration over `HashMap` (use `BTreeMap`). No wall-clock time.
   *Currently true. Must stay true.*
2. **Fixed tick, never delta-time.** The simulation advances in whole ticks or not at all.
   *Currently true.*
3. **Input is a value, separated from state.** `Intent { dx, dy, attack }` is the entire
   input surface. A local keyboard, an AI, and a network packet are interchangeable.
   *Currently true. This is the network seam, and it already exists.*
4. **All simulation state is serializable.** Needed for joining, resync and rollback
   snapshots. *Not yet: `Fighter` and `Overworld` need `Serialize`/`Deserialize`.*
5. **Randomness is explicit and seeded, never ambient.** `Overworld` already carries its
   own seeded generator. Combat has no randomness at all, and any that gets added must
   come from a seeded stream, not from the system.
   *Currently true.*
6. **The simulation performs no I/O and does no rendering.** `henge-core` depends only on
   serde. It compiles for a server, a browser and a headless test with no changes.
   *Currently true.*
7. **Actors are a collection, not named fields.** *Not yet. This is the "Now" item above.*

## Order of work

1. `Vec<Fighter>` with pluggable input sources. Four-player local falls out immediately.
2. Derive `Serialize`/`Deserialize` across all simulation state; add a snapshot test that
   proves a serialized-then-restored world simulates identically.
3. A determinism test: run N ticks from a seed on two independent worlds, assert the state
   hashes match. This is the guard rail that keeps 1 to 7 above honest, and it belongs in
   CI before any network code exists.
4. Local network play for one arena, deterministic lockstep, 2 to 4 players.
5. Rollback on top of lockstep, once latency becomes the thing that hurts.
6. The persistent overworld server, which is a separate program that shares `henge-core`
   and never needs to touch the arena code.

Steps 1 to 3 are worth doing regardless of whether networking ever happens: they make the
game testable, reproducible, and debuggable. Nothing here is speculative work.
