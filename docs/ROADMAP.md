# Roadmap

> For the exhaustive version, built around the 334 symbols left in the original
> executable, see [COMPLETE.md](COMPLETE.md). This file is the working shortlist.

Ordered by what the project is missing most, not by what is easiest.

## Now

- [x] **N fighters instead of two.** Done. Combat resolution moved out of the renderer
      and into `henge-core` as `Bout`, which holds a `Vec<Fighter>` and takes one
      `Intent` each. Four-player local works; a network peer slots into the same seam.
- [x] **Four knight colours.** Done, by palette substitution rather than by using the
      separate banks. Arena palettes are fixed at 32 entries, so a recolour has to be a
      substitution within the colours already present: preserve luminance, move hue, and
      never touch greys, which carry the armour and the ground shadows. Status bars are
      drawn from the same ranked hues, so a bar cannot drift from its knight.
      *Limitation: a palette supports as many knights as it has hue groups. Three of the
      four arena families have six; the glade has four, two of them thin, so its fourth
      knight is washed out.*
- [x] **Audio.** Done. `henge-audio` splits deciding *what* to play from *where it comes
      out*: cues are derived by watching the fight and are pure testable logic, while the
      backend is behind a trait so a browser or a server swaps only that half. All 49
      samples load. No audio device is a normal state, not a failure, so the game plays
      silently rather than refusing to start.
      *Next: more than four cues, and sounds carried on animation frames rather than
      inferred from state changes.*
- [x] **Text.** Done. The lookup table in `MAIN.EXE` is still not recovered, so the glyph
      order was read off the artwork instead: the banks run A-Z, then a-z, then 0-9, then
      punctuation, with a few unidentified ornaments at the end left unmapped rather than
      guessed at. The mapping lives in the pack as content, so a replacement font is a data
      change. Text draws as a silhouette in a chosen colour, because a glyph's own shades
      are legible over one arena's palette and invisible over the next.

## Next

- [ ] Title screen, character select, results
- [x] Health persisting between fights, so dying costs something. Wounds carry, only
      travelling mends them, and winning heals nothing: the same walking that repairs you
      is the walking that finds trouble. Dying ends the run.
- [x] Somewhere to go. Places live in the pack as data: a name, a backdrop, a map
      position and a menu, so adding one is editing JSON. Only the healer does
      anything yet, and what it charges is **days**, because days are the only
      currency a run has and inventing money would be inventing an economy. An
      option that is not built is listed and marked shut rather than left off,
      since a live-looking option that silently does nothing is worse than a
      closed door. Positions were read off the map art, not recovered: the
      original's node graph is still inside `MAIN.EXE`.
- [ ] Inventory, and something to spend a run's winnings on
- [ ] Save and load
- [ ] Web build (wasm), including running the bake client-side so no assets are ever served
- [ ] Settle the arena family pairing question (see FORMATS.md)

## The map's icon set

`MI.C` is the overworld's icons, and most of it is still unused:

| Frames | What |
|---|---|
| 0-4 | the knights' map tokens, one per player colour |
| 5-9 | the same tokens ringed, presumably whose turn it is |
| 10-14 | crystals on coloured bases |
| 42 | a grave marker |
| 43-46 | creature tokens on coloured bases |

These are authored against the **map's own palette**, so their indices are already correct
when drawn on the map and need no translation. That is worth knowing before reaching for
the workaround below: check whether a sheet belongs to the palette you are drawing it over
before assuming it does not.

- [ ] Use the rest: creature tokens for roaming enemies, crystals for whatever the quest
      turns out to need, the ringed variants for the active player

## Known defect: sheets do not record which palette they mean

A sheet's pixels are palette **indices**, and nothing in the manifest says which
palette they were baked against. Draw a sheet over a scene that loads a different
palette and every index means a different colour, which is not a visible error so much
as a plausible-looking one: the traveller's token drew as a smear of browns and blues
that read as map dithering, so it appeared to be missing rather than wrong.

The traveller's token no longer suffers from this, but only because it turned out to
belong to the map's palette after all: it was the wrong sprite from the wrong file, not a
palette problem. **The real fix remains to record each sheet's source palette in the
manifest**, so indices can be translated into whatever palette is loaded, by nearest
colour. Any sheet genuinely drawn over a foreign palette still has this problem.

- [ ] Record a `palette` id per sheet in the manifest, and translate on draw

## Reverse engineering still open

- [x] **Multi-part sprite composition.** Decoded. The per-frame record is
      `[u8 bank*4][u8 cel][i8 y][u8 flags][i16 x]`, and the 236 animation scripts are named
      data in DGROUP. See `TASKVM.md`; verified by compositing knight and creature frames.
- [x] Symbol name to address mapping in the debug info: 2,223 symbols with addresses,
      recovered by `tools/symbolmap.py`, which is what made the above fall out
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
   snapshots. *Done, and tested: a bout serialized, restored and simulated onward keeps
   agreeing with the original.*
5. **Randomness is explicit and seeded, never ambient.** `Overworld` already carries its
   own seeded generator. Combat has no randomness at all, and any that gets added must
   come from a seeded stream, not from the system.
   *Currently true.*
6. **The simulation performs no I/O and does no rendering.** `henge-core` depends only on
   serde. It compiles for a server, a browser and a headless test with no changes.
   *Currently true.*
7. **Actors are a collection, not named fields.** *Done: `Bout` holds a `Vec<Fighter>`.*

## Order of work

1. ~~`Vec<Fighter>` with pluggable input sources.~~ Done.
2. ~~Serialization across simulation state, with a snapshot test.~~ Done.
3. ~~A determinism test comparing state hashes across independent runs.~~ Done, and in CI.
4. Local network play for one arena, deterministic lockstep, 2 to 4 players.
5. Rollback on top of lockstep, once latency becomes the thing that hurts.
6. The persistent overworld server, which is a separate program that shares `henge-core`
   and never needs to touch the arena code.

Steps 1 to 3 are worth doing regardless of whether networking ever happens: they make the
game testable, reproducible, and debuggable. Nothing here is speculative work.
