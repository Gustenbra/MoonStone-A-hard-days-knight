# Roadmap

> For the exhaustive version, built around the symbols left in the original executable,
> see [COMPLETE.md](COMPLETE.md), and for the item-by-item record of what was recovered
> and what was designed, [BUILD_ORDER.md](BUILD_ORDER.md). This file is the working
> shortlist.

Ordered by what the project is missing most, not by what is easiest.

## The rule this project is built to

**Nothing is invented.** Every mechanic is recovered from `MAIN.EXE` by disassembly and
translated literally into Rust, with the image address cited in a doc comment beside it.
Where something genuinely is ours, it says so in the same breath. Where the original is
odd, buggy or dead, it is reproduced odd, buggy or dead, and the bytes are quoted so
nobody later "fixes" it.

That rule is why the combat feels right, and every time it has been bent the result was
a bug a player noticed within a minute and a test never would. The three worst of those
are written up in `REVERSING.md`, because they share a shape: **each was a rule henge
applied more widely, or more often, than the original applies it.** One clock for two.
One border gate for everyone. One step per tick instead of one per frame.

## Fully translated from the assembly

This is what "done" means here: read out of the image, translated line for line, and
pinned by a test that fails if it drifts.

### The fight

- [x] **The task VM.** `PerformCOMMAND` (0x97fb), `TaskComTable` (DS:0x9448), 19 opcodes,
      242 scripts. Animations in the original are not frame lists, they are small
      programs with jumps, loops, placement, flipping and collision hooks, and that is
      why the combat reads as positional rather than as trading canned attacks.
- [x] **Multi-part sprite composition.** The per-frame record is
      `[u8 bank*4][u8 cel][i8 y][u8 flags][i16 x]`, and the bank tables belong to the
      **encounter**, not the actor: one loader fills all four and `+0x18` only says which
      one a task starts on. See `TASKVM.md`.
- [x] **Both clocks.** The game has two and the engine was pacing everything off the
      wrong one. A combat frame is two BIOS ticks, 109.849 ms, which is six ticks of the
      54.6204 Hz timer the game programs and is `DELAY`'s own six; everything else runs on
      the 70.0863 Hz retrace. Fights had been running 1.2832x too fast.
- [x] **Movement, for every actor.** A controller runs once per displayed frame and moves
      once, by an entry in its own walk-speed table. All six tables in the image are read
      at bake time and checked against the binary: `TroggWALKR`/`U`/`D`, `TrollWALKR`,
      `MudmenWALK`, `BKnightWALKR`/`U`/`D`, and the knight's own `K_Walk*`. Nothing moves
      by a flat speed per tick, and the five creatures the original moves from their own
      scripts still do.
- [x] **The borders, and who they are for.** `CheckBorder` (0x40d0) and `SBORD` (0x4552)
      are the person's own knight's alone. `MonsterWalk` (0x4e8b) calls neither and
      nothing in the image clamps a column after a step is added, which is why
      `TroggTABLE` can seat creatures at -50 and 360 and have them walk in.
- [x] **Every creature's controller**, block for block: the three troggs, the troll, the
      ratman, the mudmen, the beast, the Balok, the demon, the dragon and its two claws,
      and the computer knight (`ControlBlackKnight` 0x4b79, including `BKBlock` and
      `_evadechop`). Read end to end from the entry point down every branch, not sampled:
      four of the nine were wrong in ways only a full read finds.
      * `ControlBeast` (0x2f5f): `BeastMove` writes the column and nothing else, the depth
        is `SetBEASTZ`'s at the turn alone, `BeastChargeOffsets` is its walk-speed table
        under another name, and `SetBeastTimer`'s count is written and never read.
      * `ControlTroll` (0x55c4): at a hundred and fifty exactly it stands, and `TrollChop`
        (0x56a3) reads the persistent `+0x28` so it never chops twice running.
      * `ControlMudmen` (0x52ee): **the burrow is the kill, not a retreat.**
        `Mudmen_IBury` is the rear-up -- the same frame `Mudmen_Appear` ends on -- and
        `MudmenBury` (0x54bb) drags the knight under where he stands when it finishes, on
        his plane and twenty to eighty pixels of him. `MudmenAppear` is the eruption at
        the start of the fight and runs once. The entangle is forty frames, fire and down
        together tear him out of it, and the shove costs the mudman a point.
      * `ControlBalok` (0x3599): `BalokHit`'s `+0x28 == 4` branch is `Balok_SlapRecover`,
        the crush snaps the Balok onto the knight before `KillKnight`, and
        `ControlBalokRelease` puts a live knight down seventy five pixels in front of it.
- [x] **The blow.** `TASKWALKCOLLIDE` (0x9e06) and the weapon and body piles, `CalcDamage`,
      the `*Att`, `*Hit`, `*Dam` and `*Blo` tables, and the per-encounter rows each
      `InitKnightvs*` writes over them.
- [x] **What a blow on a fallen body does**, which is the striker's business and not the
      blow's: `KnightGotStruck` (0x4267) reads `mov si, [di+0xe]` before it jumps through
      `StruckTable`, so an axe trogg always takes the head and a hammer trogg always
      collapses him, whichever blow landed.
- [x] **The knockback.** `KnightSLAP`/`KnightSLAPR`, `BalokSLAP` and `DemonWHIP` as one
      eleven-word region, all eight writers of `SLAP` and all six of `SLAPY`, and the
      demon's whip hand-off that drags the knight in rather than throwing him.
- [x] **The screen shake.** `ShakeADD` (0x493f) through `COLCON` to `ShakeScreen`
      (0x495b), with its two callers and the jump-height test that keeps a low hop quiet.
- [x] **The colours.** Palette entries written, not pixels substituted: `ColourKnight`,
      `Colour2ndKnight`, each creature's block by the ground it stands on, `COLCON`'s
      cycles once per loop pass, and `KnightGlowOn` breathing a nearly-dead knight's own
      indices.

### Around it

- [x] **N fighters instead of two.** `Bout` holds a `Vec<Fighter>` and takes one `Intent`
      each. Four-player local works; a network peer slots into the same seam.
- [x] **Audio.** Deciding *what* to play is separated from *where it comes out*, so a
      browser or a server swaps only the backend. All 49 samples load, six tunes bake from
      the `xTUNEn.BIN` drivers. No audio device is a normal state, not a failure.
- [x] **Text.** `GFX:TextASCII` at image 107,446, and every message chain quoted from the
      image with its records' own coordinates. A glyph draws in its own indices through
      the same blit as every other cel, because that is all `GFX:TextP` (0x7aee) does.
- [x] **Intro, title, character select, the map, the ending.** All four screens and the
      win condition (`MOON:Henge`), with the intro's and ending's frame counts read off
      `INTR.EXE`'s own `DelayFrames` call sites rather than chosen.
- [x] **The overworld.** `MapIconsTABLE`, `ForestLairs`, `LairLocation`, `LairType`,
      `TakeMagicTABLE`, the spawn tables, `lev_adjust`, `XPlevels`, `Moons`. Hand-sited
      lairs had been a median 51 pixels out and every head count wrong.
- [x] **The four player knights' real names**, which are `SIR_GODBER`, `SIR_RICHARD`,
      `SIR_JEFFREY` and `SIR_EDWARD`; what had been shipping were the *computer* knights'.
- [x] **Health persisting between fights.** Wounds carry, only travelling mends them, and
      winning heals nothing: the same walking that repairs you is the walking that finds
      trouble. Dying ends the run.
- [x] **Somewhere to go.** Places are pack data, so adding one is editing JSON. An option
      that is not built is listed and marked shut rather than left off, since a
      live-looking option that silently does nothing is worse than a closed door.

### Deliberately not built, because the original has not got it

- [x] **Save and load.** There is no slot, no file and no routine anywhere in the 2,223
      symbols; the only `*Save*` hit is `SaveTYPE`, a task VM opcode, and `MOON.CFG` is a
      sound-card profile. So there is no menu item and nothing a player can reach writes
      one. The serialisation survives, explicitly labelled, as the headless harness's way
      of posing a run and as the determinism test's backbone.

## Still to do

Nothing below is a translation problem. The recovered mechanics that remain are small and
named; the rest is deciding what the game is.

### Recovered and not wired up

- [ ] Tune 5 after Math's gift (`_bestow_done`).
- [ ] `BuyMoonstone` and `SellMoonstone`. Recovered, but they are a trade between two
      knights' records rather than a shop, so no counter in a one-knight run can reach
      them. Blocked on multiplayer, not on reading.
- [ ] The dragon's hoard take mechanic. The page draws; taking from it does not.
- [ ] `Beast_BackToss`'s dead second row (`InitKnightvsBeast` 0x2288, kind 0xe) is
      written and unreachable in the original too. Reproduced, and worth leaving alone.

### Unrecovered, and probably unrecoverable

- [ ] The frame rate of six loops — `TavernLoop`, `StatLOOP`, `DonateLoop`, the two
      stalls, and the title's own — which make no wait of any kind. The image names no
      rate for them, so they sit on the retrace with a `// TODO` saying exactly that
      rather than a number nothing supports.
- [ ] The overworld node graph. Map positions were read off the artwork by eye; the
      original's graph is still inside `MAIN.EXE`.
- [ ] The arena family pairing question (see `FORMATS.md`).

### The game, rather than the disassembly

- [ ] **Inventory, and something to spend a run's winnings on.** The one item on this
      list that would change how the game *plays* rather than how faithful it is. Bounty
      accumulates and buys nothing.
- [ ] Web build (wasm), including running the bake client-side so no assets are ever
      served.
- [ ] The rest of `MI.C`: creature tokens for roaming enemies, crystals for whatever the
      quest turns out to need, the ringed variants for the active player.

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

## Sheets do not record which palette they mean, and the original does not either

A sheet's pixels are palette **indices**, and nothing in the manifest says which palette
they were baked against. Draw a sheet over a scene that loads a different palette and
every index means a different colour, which is not a visible error so much as a
plausible-looking one: the traveller's token once drew as a smear of browns and blues
that read as map dithering.

The token turned out to belong to the map's palette after all. A `palette` field per
sheet was then added and used to translate text by nearest colour, and it is gone again,
with the recipe stepped: the original blits every cel, glyphs included, in its own indices
through 0x5d7f and chooses which screens it writes on, and the translation was wrong where
it did anything, sending the small face's index 1 to the map palette's nearest purple
instead of the white the original shows. Anything drawn over a palette it was not painted
for is a wrong screen, not a missing table, and the fix is to draw what the original draws.

## Adding a creature

The pattern, because it has been got wrong twice and the symptom looked nothing like the
cause both times. The long version is in `REVERSING.md`.

1. **Walk script rows first.** Their length is the walk cycle: `NextWalk` (0x4ef7) masks
   the cycle with 7 and skips any index whose script row word is zero.
2. **One `[x, z]` walk-speed entry per row entry.** Where the original has a table, read
   it at bake time from the symbol its mover names rather than typing the numbers.
3. **Where an axis has no table**, use the controller's own literal, *per displayed frame*,
   and cite the instruction that writes it.
4. **Leave both empty** for anything that moves from its scripts.
5. **Write the record kind** (`+0x35`). `StruckTable` is indexed by it, and a creature
   whose kind is never written is a creature whose encounter rows can never be reached.

Uneven table entries are not decoration. They are what lets a creature come to rest at
distances a single stride would skip, and several of the range bands it has to land in are
narrower than one stride.

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
| Tick rate | the map's own 70.0863 Hz retrace | the fight's own 54.6204 Hz timer |
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
   *Currently true, and it survived the clock change: one accumulator and a tick length
   looked up per pass, so what is left over on a change of clock is always less than one
   tick of the clock just in use. The simulation never sees a fractional step.*
3. **Input is a value, separated from state.** `Intent { dx, dy, attack }` is the entire
   input surface. A local keyboard, an AI, and a network packet are interchangeable.
   *Currently true. This is the network seam, and it already exists.*
4. **All simulation state is serializable.** Needed for joining, resync and rollback
   snapshots. *Done, and tested: a bout serialized, restored and simulated onward keeps
   agreeing with the original.*
5. **Randomness is explicit and seeded, never ambient.** `Overworld` carries its own
   seeded generator, and the fight's own rolls come from `Bout::rng`. The screen shake is
   the one place a random number is drawn outside it, and it is drawn in the renderer,
   where nothing reads the result back into the fight.
   *Currently true.*
6. **The simulation performs no I/O and does no rendering.** `henge-core` depends only on
   serde. It compiles for a server, a browser and a headless test with no changes.
   *Currently true.*
7. **Actors are a collection, not named fields.** *Done: `Bout` holds a `Vec<Fighter>`.*

## Order of work

1. ~~`Vec<Fighter>` with pluggable input sources.~~ Done.
2. ~~Serialization across simulation state, with a snapshot test.~~ Done.
3. ~~A determinism test comparing state hashes across independent runs.~~ Done, and in CI.
4. ~~Network play, deterministic lockstep, 2 to 4 players.~~ Done: `henge-net`, and the
   lobby on the title's fifth row. See **Where online play has got to** below.
5. **The four knight records unified**, so each person takes their own turn on the map.
   This is the one thing between what is built and the original's own four-player quest.
6. Rollback on top of lockstep, once latency becomes the thing that hurts.
7. The persistent overworld server, which is a separate program that shares `henge-core`
   and never needs to touch the arena code.

## Where online play has got to

**Built and proved.** `henge-net` is the whole of it: a framed TCP link, a lobby with a
name and a roster, deterministic lockstep with an input delay, a rolling fingerprint
check, and NAT-PMP and UPnP so the host's port opens without anybody visiting a router's
web page. The title has a fifth row, `Play Online`, which is the only row in that list
that is not out of the image. Two machines have been driven through a whole quest headless
(`--host` / `--join` / `--begin`, and `--trace`) and agreed on every fingerprint for
thousands of ticks, through the map, a town and a fight.

What crosses the wire is one byte per seat per tick: the five bits `GetInputDevice` builds.
A press is not sent at all, it is the rising edge of that byte, computed identically on
every machine. Nothing in `henge-core` knows a peer exists and no recovered number changed.

**The one thing that is ours and not the original's**, and the reason step 5 above exists:

> An online game plays **one** quest. `Run` is `KnightTAB`'s record zero plus everything
> that belongs to the game rather than to a knight, and `Run::rivals` is the other three
> records in a leaner shape that only the computer's turn reads. So the run belongs to
> seat zero's knight on every machine, the arena seats every person in the game as the
> knight they chose on `ChooseKnight`, and `WHICH`'s turn on the map is seat zero's.

Two smaller consequences of the same split, both in `henge-desktop`:

- **The lobby settles nothing about knights.** Pressing Begin opens `ChooseKnight`, the
  same screen a game at one keyboard gets, inside the lockstep gate from its first tick;
  the seats take their turns on it in `choose_player` order and `begin_quest` builds the
  same run out of `choose_knight` on every machine. The lobby used to carry a knight row,
  which meant two screens for one choice and a screen of ours where the image already has
  one.
- **Enter, backspace and the nine number keys have one slot between the four seats**,
  because they are not controls and the original has no bindings table for them. They are
  filled from seat zero's word on every machine, not from the seat at this keyboard: a slot
  filled from "mine" holds a different word on each machine, and a machine would then leave
  a message box on a different tick from its peers. `ChooseKnight` is the one screen where
  every seat needs its own, and it reads them per seat instead.

The original does not work that way. `NextWHICH` at 0xa434 walks `WHICH` 0 to 3 and takes
whoever is alive; `0xa4a9`'s `cmp ax, [NUM_PLAYERS]` is what decides whether that turn is a
person's or the computer's, so seats below `NUM_PLAYERS` are people and the rest are not,
and all four are the same 0x62-byte record.

**The work is therefore to make our four the same record too.** The cheapest shape that
keeps every existing caller working: `Run` stays the record whose turn it is, and gains a
bench of the other three; `NextWHICH` swaps the outgoing knight's per-knight fields out and
the incoming one's in. Every `self.run.gold` in the shell then keeps meaning "the knight
whose turn it is", which is what it already means, and `Rival` grows the fields a person's
seat needs that the computer's did not: the pack, the magic slot, the ward and curse flags,
the wizard's grudge, and the sword. Once that is done, a lockstep game needs no new
netcode at all: the input is already all four seats' and the fingerprint already covers the
whole run.

Steps 1 to 3 are worth doing regardless of whether networking ever happens: they make the
game testable, reproducible, and debuggable. Nothing here is speculative work.
