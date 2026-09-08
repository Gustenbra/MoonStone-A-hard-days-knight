# Build order

Every item, once each, in the order you would actually do it. One flat list.

`COMPLETE.md` is the same work organised by subsystem, with the original's function names
against each part. This file is the checklist.

**77 items. 77 done, none partial, none remaining.**

Ordering is by dependency, not preference. Where two items do not depend on each other they
are grouped in the same phase and can go in any order, or in parallel.

---

## Phase 0: done

The engine and a vertical slice. Roughly a quarter of the game.

- [x] 1. Unpack the original executable, by running its own decompression stub
- [x] 2. Decode the packing used by every container
- [x] 3. Decode PIV / CMP / .P full-screen images and palettes
- [x] 4. Decode CEL / OB / F / FON / .C sprite banks
- [x] 5. Decode .T arena layouts, including signed prop coordinates
- [x] 6. Decode COLLIDE.HIT hit lines
- [x] 7. Decode VOC samples
- [x] 8. Asset packs: logical ids, pack stacking, provenance, shippability check
- [x] 9. The baker: original files to indexed PNG, WAV and JSON
- [x] 10. Palette-indexed 320x200 framebuffer, presented at 4:3
- [x] 11. Sprite drawing: transparency, mirroring, masking. Colour substitution was
      here too, and is gone: the original colours a fighter by writing palette entries,
      item 78, and a sprite is blitted in its own indices
- [x] 12. Text: glyph map read off the artwork, silhouette rendering
- [x] 13. Arenas: 56 of them, scenery, the header's own border list, depth sorting
- [x] 14. Combat: positional hit lines, committed attacks, damage, death
- [x] 15. Bouts of up to four fighters, in the simulation, deterministic and serializable
- [x] 16. Overworld: travel, day cycle, ambushes, terrain classification
- [x] 17. Runs: wounds carry, travel mends, death ends the run
- [x] 18. Four locations with menus, and a working healer
- [x] 19. Sound: 49 clips, four cues, silent without a device

---

## Phase 1: the blockers

This was the research phase, and it is finished, and so is the construction that followed
it: 20 through 27 are done, and phase 2 with them, and now phase 4.

Item 20 was recorded as settled and negative. **It was wrong**, and 21 is what proved it:
the search had been run against an image that was still packed, because `MAIN.EXE` is
packed twice and the unpacker peeled only the outer layer. Peel both and the symbol table
is plainly there. The correction is written up in `REVERSING.md`; it is left in the record
rather than quietly deleted, because the failure mode is worth passing on.

That unblocked 22 almost for free, and most of the phase with it.

- [x] 20. ~~Settled: there is no symbol address table~~ **Wrong; see 21.** The search was
      sound but it was run against the EXEPACK'd image, where the table sits inside
      RLE-compressed bytes and reads as loose name strings with junk between them
- [x] 21. **Recovered: 2,223 symbols with addresses.** `MAIN.EXE` is PKLITE outside and
      Microsoft EXEPACK inside; `tools/symbolmap.py` peels both and parses the eleven
      per-module blocks of the TASM symbol table. Code addresses take a seven-step monotone
      correction, derived and applied by the tool. 1,778 of the 2,223 corroborate
      independently: code symbols by landing on called branch targets, data symbols by
      being referenced from code, and several by content, with `CelFile1` pointing at
      `KN1.OB` and `map` at `MAP.CMP`
- [x] 22. **Located: `PerformCOMMAND` at 0x97fb**, `PerformLOOP` at 0x97f2. Confirmed by
      disassembly as a bytecode fetch-and-dispatch loop, not by name alone. The command
      table it indexes is `TaskComTable`, and the code's `mov bx, 0x9448` matches that
      symbol's address exactly
- [x] 23. **Recovered: nineteen commands, with operand widths.** `TaskComTable` is BSS,
      so the handler addresses live only as immediates in `INITTASK`, and those are
      link-time offsets needing the same correction `symbolmap.py` fits for code symbols.
      Corrected, all nineteen land exactly on a routine entry; uncorrected, none do.
      Widths come from each handler's own `add word ptr [di+2], n`. Three table slots are
      empty and one handler is a bare `RET`. `docs/TASKVM.md` has the set;
      `tools/taskvm.py` reads it back and disassembles any script
- [x] 24. **Recovered: `[u8 bank*4][u8 cel][i8 y][u8 flags][i16 x]`.** The earlier guess
      had x and y the wrong way round and read the bank selector as a plain index rather
      than the slot times four, which is why it composited to a heap. **Checked by
      compositing and looking**: the knight's eight-frame walk cycle, stance and sword
      swing all come out coherent, the mirrored form stays assembled, and the troll,
      trogg, ratman and balok composite from their own bank tables. All 221 named scripts
      in DGROUP parse end to end and terminate on `ff ff`
- [x] 25. **The VM, in `henge-core/src/taskvm.rs`.** Every command, the three end of
      frame forms and the part record, as a deterministic interpreter: integers only,
      `BTreeMap`, no I/O, one `step` per tick returning the parts to draw as bank slot,
      cel, offset and flags. The end of frame handler was transcribed from the code at
      0x993a in its own order, which turned up one thing the docs had not: `ff ff` loops
      like `ff fe` while a `TASKLOOP` count is running, and only ends the animation when
      it is not. `TASKGOSUB` is an emitted effect naming the routine, with all 37 targets
      in a table by name and none of them faked; `TASKSOUND` is a cue. Twenty one tests,
      including a loop, a gosub, the branch on hit points, and a state hash that survives
      a serialize and reload. `Bout::state_hash` folds the VM state in
- [x] 26. **Exported: all 221 scripts and every actor's bank tables.** `henge-formats`
      reads the handler table out of `INITTASK` and each width out of its handler, checks
      the set it recovers against the documented one, and parses the scripts into the
      engine's own `Instr` values, so the baker cannot write a script the engine would
      not accept. `data/scripts.json` holds the lot, 3,709 parts and 724 commands, the
      same counts `tools/taskvm.py` reports; `data/banks.json` holds the four bank tables
      for each of the eleven creature loaders, with a sheet, a frame base and the size of
      every cel, because a mirrored part is placed at `task_x - (x + cel_width)`. Needs
      `research/main.final.bin` and `research/symbols.json`; the baker says so when they
      are missing
- [x] 27. **The knight runs the original's scripts.** `Knight_SwStance`, the four
      `Knight_SwWalkR` frames cycled the way the original's controller cycles them,
      `Knight_SwSwing`, `Knight_SwShoulderHit` and `Knight_SwDeath`, composited from
      their parts through the bank tables. The hand authored frame lists are deleted.
      Hit lines are gone with them: the swing's blade is the parts the script flags
      `WEAPON`, swept as their own rectangles, which also means the stance's sword, flagged
      `BODY`, cannot cut. **Checked by looking**: the in-game walk and swing match the
      `tools/taskvm.py` contact sheets frame for frame, both facings, and a traced
      practice duel still lands hits, staggers, and ends in a death

## Phase 2: the bestiary

Unblocked all at once by 25, and now built: every creature stands, walks, strikes,
takes a blow and dies on its own scripts, and the road fields them. Two things
turned up on the way that the record should carry.

**The script set is 236, not 221.** The mudmen's prefix is `Mudmen`, not the `Mudman`
of `MudmanTABLE`, so their fourteen scripts and `Rat_TreeBrush` had been filtered out
of every count. All 236 parse and verify; the four `TASKGOSUB` targets the mudmen add
(`AddMudVoice`, `AddMudSound`, `AddCrushSnd`, `PlayScareMusic`) are in the table.

**The controller tables are recovered after all.** They are `BSS`, which is why the
load image showed nothing, but `SetKnightAnims` and `SetMonsterAnims` in `MOON` fill
them with immediates: a walk table per creature (`*Wal`, right at +0, up at +0x10,
down at +0x20), the attack scripts by attack kind (`*Att`), the blow-taken script by
the attacker's attack kind (`*Hit`), and the damage per attack kind (`*Dam`). And
each `InitKnightvs*` calls a `Set*Tables` that writes the stat block into the actor
record: hit points at `+0x38` and `+0x3c`, the tracker's approach and back-off ranges
at `+0x52` and `+0x54`, its plane tolerance at `+0x56`, the kind at `+0x35`. So the
numbers below are the original's, not chosen. `docs/TASKVM.md` has the tables.

Each creature is an `ActorDef` like the knight's: the closure of the scripts its
states reach, its loader's bank tables starting on table 2 (the actor record's `+0x18`,
where the knight's holds table 1), an origin, hit box and girth read off its own
standing frame, and the five states. Which of its attacks the one button gets is ours
and marked so in the baker. **Every one was checked by looking**: `--start arena
--foe <id>` in the arena browser, `,` and `.` to cycle, screenshots against the
`tools/taskvm.py` composites, and a `--trace` of a fight to a death.

**Which creature waits on which ground is design.** It lives on each family in the
pack (`creatures`, brought round by the family's own turn counter): troggs and ratmen
under the trees, mudmen in the marsh, a troll in the waste. The beast, Balok, the demon
and the dragon are lair and set-piece encounters and do not stand by the road. A
creature's health and blow are at the original's scale of a twenty-point knight, and a
fight at any other scale moves them by the same ratio it moves the knight's blow.

- [x] 28. **Troll.** `Troll_Stance`, `Walk1`..`4` (`TrollWal`), `Troll_Bunt` for the
      button (`Troll_Chop` is the long one, 105 to 160 pixels out, for the behaviour to
      pick), `Troll_Hit`, `Troll_Dies`. Forty hit points, a blow of three, ranges
      150/90/5. `SetMonsterAnims` fills `TrollHit` with its own address eight times
      over, which reads as a slip; `Troll_Hit` is the only blow-taken script it has
- [x] 29. **Trogg with axe**, and the hammer trogg with it, since the axe loader's
      banks hold both. `TroggAxe_Stance`, `WalkR1`..`R3`, `Swing`, `WaistHit` (the
      `TroggHitAxe` entry for a swing), `Split` (where that script's `TASKDEAD` goes).
      Twenty hit points, a blow of three (`TroggDamAxe`), ranges 100/90/5; the hammer
      is the same at two damage and 70/65/5
- [x] 30. **Trogg with spear.** `TroggSpear_Stance`, `WalkR1`..`R3`, `Lunge`, `WaistHit`,
      `Split`. Fifteen hit points, ranges 130/120/5. It has no `*Dam` table; its blow
      is set where the lunge lands, in code not yet read, so three stands in
- [x] 31. **Ratmen.** `Ratman_Stance`, `Roll1`..`4` (`RatmenWal`; the leaps are its up
      row), `Slash`, `Knocked`, `KnockDead`. Five hit points and a slash of one, and
      **the record's earlier reading of the blow was a row out**: `SetRatmenTables`
      fills `RatmenDam` at `[bx+2]` with three and `[bx+4]` with one, those tables are
      nine words indexed by attack kind two to an entry, and the ratman's own kind is 4
      (`ControlRatCollide`), so one is the slash and three is the bite that waits on 37.
      The moon moves both: seven and three under 0x2d, twelve and five under 0x31, and
      that is now in the game (item 64)
- [x] 32. **Mudmen.** `Mudmen_Stance`, `Move1`, `Move3`, `Move1`, `Move2` (`MudmenWal`,
      exactly), `ArmAttack`, `Hit`, `Dies`. Thirty hit points, a blow of two, ranges
      80/75/5. `MudmenWALK` moves it eleven across and thirteen deep a frame, so it
      comes at you on a diagonal. `Mudmen_Appear` and `IBury`, the rising out of the
      ground, are behaviour and wait on 37
- [x] 33. **Demon.** It arrives (`Demon_Evolve`, which is what `InitKnightvsDemon`
      writes into its stance slot at `+0x10`, and whose last frame calls
      `AddDemonWhirl`), breathes through the four stances `Demon_Stance1`'s own
      `TASKSAVE` into `+0x10` cycles, and fights on the three ranges `DemonAttack`
      gives it: the slap inside a hundred (kind 0x10, nine frames of cooldown, and
      two turns of `demonbodge` between slaps), the zap out to a hundred and thirty
      (kind 4, six frames), and the whip out to a hundred and forty (kind 2, five).
      The whip's four-phase follow-through is the four `DemonFLAGS` bits
      `DemonOFollowT`, `DemonOWhipFollow`, `DemonUFollowT` and `DemonUWhipFollow`
      turn into `Demon_OWhipMiss`, `OWhipHit`, `UWhipMiss` and `UWhipHit`, with
      `OWhipKnight` and `UWhipKnight` when the crack finds him between 120 and 140,
      or 130 and 150, and the caught knight is handed `Knight_SwSlapped` outright
      rather than through a weapon part, as the original hands it. The zap's
      `KnightOFF` and `KnightON` are built: ten hit points off him, off the board,
      and back 0x89 pixels to the demon's side. 250 hit points, ranges 95/90/2; its
      blow is not in a `*Dam` table, four stands in.
      **`SETDEMONBORD` is recovered, and it is not a decoration.** It is the last
      routine in `GFX`, at image 0x7ff9, sitting after `CLIPHIEGHT` and before
      `INSTALLKBD`, and it writes a count of one and a single eight-byte record into
      the segment at DS:`0x88ff` and sets DS:`0x80b5` to match. That segment is the
      arena's own `.T` file: the loader at 0x8d93 reads a count and that many
      border records out of it and takes the deepest for the row the ground starts at, and
      `SBORD` walks the same list every frame clearing the walk bits that would
      cross it. So the demon's border is the ground you may fight it on, 0 to 309
      across and 10 to 99 deep, not a frame around the screen. **Corrected under
      item 60: it is not dead code.** Two calls land on 0x7ff9 once their
      displacements are put through the link-time correction, and the claim that
      none did was made without it: `InitKnightvsDemon` at 0x2752, after 0x2746 has
      loaded the arena, so the record really does replace the arena's list; and
      `GENERATELANDSCAPE` at 0x8cb5, before it dispatches to the family's loader, so
      there the `.T` overwrites it a moment later. It lives here on the demon's own
      `ActorDef::border`, applied by `Bout::apply_actor_borders`, which replaces the
      list rather than intersecting it and runs before the fighters are stood up
- [x] 34. **Beast.** `Beast_Drool1` to stand (its `+0x10` stance), `Run1`..`4`,
      `LowerHit`, `LowerDead`. Ten hit points, a tracker that closes to two pixels. It
      has no swing: every run frame carries a weapon part, so the first run frame is
      its attack until the charge and the toss (`Beast_BackToss`, `ChestToss`, which
      are the knight's own animation on the beast's banks) are built under 37
- [x] 35. **Balok.** `Balok_Stance`, `Jump` and `Jumping` for a walk, `UpperCut`,
      `UpperHit`, `Dead`. Thirty hit points, a blow of four (`BalokDam`), ranges
      80/60/10. The grab and the three things it does to a held knight wait on 37
- [x] 36. **Dragon: the set piece, and it was all readable.** `InitKnightvsDragon`
      (0x2438) and `ControlDragon` (0x3843) give the whole encounter, and none of it
      had to be invented. The dragon is not an opponent that walks up to you: its
      record sits at a fixed address, `InitKnightvsDragon` places the head at x 80
      and z 100 and then builds **two more actors of its own** through `FindTABLE`,
      `Claw1TABLE` and `Claw2TABLE`, at x 5 and ten rows either side of the head,
      which `DragonMoveClaw1` keeps there. `ControlClaw` never calls `CalcDamage`,
      so the claws take no harm at all; they slap anything that comes inside x 100
      on their plane (kind 0xa, `Dragon_ClawSlap`) and play `Dragon_ClawDead` when
      the dragon falls. `TrackKnight` shifts the head five pixels at a time inside a
      corridor 30 to 100 wide and follows the knight in depth. `DragonMove` lifts
      the head when he comes inside 140 and lowers it when he goes back out, over
      the thirteen and nine frames `[di+0x4a]` counts, walking the `DragonWal` rows
      at +0x10 and +0x20 as it goes; `Dragon_LiftHead1` and `LowerHead1` swap the
      stance itself by writing `Dragon_HighStance` or `Dragon_Stance` into `+0x10`.
      `DragonAttack` then picks by where the head is: head up and past seventy, or
      head up and already struck (`DragonFLAGS` bit 7, which `DragonStruck` sets on
      any blow from a knight), is `Dragon_HighBreath` with `AddDragonFIRE` beside
      it; head up and closer is `Dragon_HighBite`; head down is `Dragon_LowBreath`,
      whose own weapon parts run 43 to 259 pixels out, which is why the fire crosses
      the arena. `InitKnightvsDragon` also overwrites the knight's `*Hit` and the
      `DragonDam` rows for this fight: `Knight_Burn` for the fire, `Knight_SwSlapped`
      for a claw, 10 for the bite and the claw and 30 for either breath, against a
      knight who starts on twenty. 200 hit points and a maximum of 120.
      **`Dragon_Flight1`..`8` and the loader nobody had found.** They are not on
      `DRAGON5.CEL`: `_MAP:ContinueDragon` builds `DrBuffer` at DS:`0xccbc` by
      writing the pointer at DS:`0x8975` into all five of its slots, and that
      pointer is `MI.C`, the map icon bank. The loader at 0x88b5 stores the address
      the *next* file will be loaded at before loading it, and the file after
      `ki.cel` is `mi.c`; cels 34 to 41 of `MI.C` composite to a dragon seen from
      above with its wings beating. `DRAGON5.CEL` is slot 4 of the creature table
      and it is the fire. The flight bank is in the pack as the dragon's table 5 and
      `tools/taskvm.py --actor dragon_flight` draws it. **Not built**: flying it
      over the map, which is `_MAP`'s `InitDragon`, `DragonWander` and
      `DragonTRACK`, and `Dragon_BitKnight`, the chewing a bite that connects goes
      into (`KillKnight` is wired, the eating animation is not)
- [x] 37. **Per-creature behaviour, and it is translation, not design.** Every
      controller is a named routine in `MOON` and every one of them reads. They are
      in `henge-core/src/monster.rs`, dispatched by the `controller` each actor now
      carries, which is the original's own `CONTROLTABLE` indexed by the kind at
      `+0x35`. A creature does not press a button: its routine writes the kind into
      `+0x28` and the script into `DS:0x783a`, so here it hands the fighter an
      `Order` and the joystick path is never reached. Its cooldowns, timers and
      flags are the actor record's `+0x0a`, `+0x0b`, `+0x48`, `+0x49` and `+0x4a`
      and live on the fighter as a `Brain`, in the fingerprint with everything else.
      A controller runs once per script frame, which is when the original's task
      loop calls one at all, so a cooldown of ten is ten frames and not ten sixtieths
      of a second.
      - **`MonsterTrack`** as written: `CheckZAxis` for the plane, `CheckXAxis`
        against `+0x54` for `TrackBack` and against `+0x52` for in-range,
        `TrackOpponent` beyond that, and `FaceKnight` first. Every controller sets
        its opponent to the knight and to nothing else, which is why a dragon does
        not take its own claws for an enemy
      - **Trogg** (`TroggAttacks`): the overhead from a hundred to a hundred and
        twenty, the swing inside a hundred, ten frames between blows, and a roll of
        `GETPERCENT` at 30 or under that chops through a held block rather than
        swinging into it. `TroggAttack`'s finisher on a fallen knight is kept
      - **Trogg with spear**: the kind 0x10 branch, one lunge inside 130, twenty frames
      - **Troll** (`TrollAttack`): the club inside a hundred, the overhead from a
        hundred to a hundred and fifty, and never two overheads running, because it
        compares `+0x28` before it chooses
      - **Ratman** (`ControlRatCollide`): it does not use the tracker. Slash inside
        forty, bite from forty to fifty, leap at anything further, and fifteen frames
        of `HitDelay` after a blow of its own lands
      - **Mudman** (`ControlMudmen`): it comes at you on a diagonal, reaches between
        seventy five and a hundred, and goes under the ground inside that
        (`MudmenIBury`) to come up seventy five pixels to your far side
        (`MudmenAppear`). An arm that lands **takes hold of you**: `MudmenHit2`
        removes the knight's own task and draws him inside
        `Mudmen_EntangleKnight` for forty frames, and fire and down together are
        the only thing that tears him loose, exactly the two bits `MudmenEntangle`
        tests. Fail and it is `Mudmen_ChokeKnight` and `KillKnight`
      - **Balok** (`ControlBalok`): it closes in hops, uppercuts from seventy to
        eighty, grabs out to a hundred and twenty, and then stands off until you are
        past a hundred and eighty or you take a dagger out, which it reads off
        `[knight+0x34]`
      - **Beast** (`ControlBeast`, `BeastCharge`, `SetBEASTZ`, `SetBeastTimer`): it
        never tracks at all. It runs from one side of the arena to the other, turns
        round off the edge, waits the five to twenty frames `RND & 0xf | 5` gives
        it, and picks its next line: dead on him one pass, up to twenty eight rows
        off the next, because `BeastFLAGS` bit 0 alternates
      - **Demon** and **dragon**: items 33 and 36
      What is **not** built, and is honest about it: the ratman's ballistic leap into
      a tree and onto the knight's head (`RatmanInitLeap`, `RatHangKnight`,
      `RatmanOnHead`, `RatmanGouge`), which is a whole second fight; Balok's grab and
      the three things it does to a held knight, and the landing on him that plays
      `Knight_Explode`; the beast's `Beast_BackToss` and `ChestToss`; and the waves
      (`TotalMonsters`, `MaxMonsters`), which are still one at a time

## Phase 3: economy and character

Independent of phase 1. **Can start immediately, in parallel with the research.**

- [x] 38. Gold and prices. Coin off the fallen, a `bounty` per actor in the data,
      prices on the items, and both towns' merchants open. The hermit in the woods
      charges only days; the healer inside the walls takes a donation and spends it
      down, which is `_WIZARD:HealDon`
- [x] 39. Inventory: carrying, using, losing. A bounded pack on the run, items as
      data with a price and a virtue, flasks bought and drunk, and losing made real
      both ways: a flask is spent when drunk, and a cutpurse on the road takes coin
      or, failing that, something out of the pack
- [x] 40. **Character stats and abilities.** Strength, constitution and endurance at
      `+0x2e`..`+0x30`, each capped at five by `_WIZARD:CheckMaxAbility`, and the
      original's own arithmetic turning them into a fight: `10 * constitution +
      armour + 10` for the health at 0x28d, `strength + blade` added to a blow by
      `CalcDamage`, and `endurance * 2 + armour + 4` shifted left four for the day's
      step budget in `DistanceDONE`. All three are on the sheet, all three are
      raised and lowered by the mystic, the wizard and levelling
- [x] 41. **Potions beyond healing.** The ten magic slots `_WIZARD:MagicRND` hands
      out are items with a virtue apiece: the potion restores, the gem and the hawk
      fly, the ring wards, the scroll of haste doubles the day, acquisition seizes,
      protection turns an ambush away or backfires. Two are still inert and honestly
      so, because the talisman and the scroll of the Wyrm both act on the dragon and
      the dragon's set piece is item 36
- [x] 42. **Magic: spells, casting, costs.** `MagicCast` is a chain of `cmp bx,
      slot`, reproduced as `Run::cast`; the slots and their prices are
      `MagicPrices` as `SetUpStatus` fills it, and the casting is done from the
      character sheet, which is where the original does it
- [x] 43. **Curses.** Two, and both recovered. A scroll of protection that backfires
      (`KnightProtection`, one roll in eleven) sets the flag `ControlKnight` reads
      and reverses the joystick for that bout, cleared at the end of `Combat`. The
      wizard's toad is `[si+0x3a] = 3`, and it costs the three days it says it does:
      `_MAP:NextWHICH` tests that byte and passes straight to the next knight, so a
      toad's turn goes by without a step
- [x] 44. **The hawk and the gem.** Both are `EffectFLAG`: the token crosses the map
      with no step count, no slow ground and no ambush while either is up, and it is
      drawn as the token's own frame plus five for the gem and plus ten for the hawk,
      which in `MI.C` are the crystal row and the hawk row, one per knight's colour.
      The gem's flight comes back to where it began, the hawk's lands where you put it
- [x] 45. **Experience and levelling.** `XPlevels` is the cost of a point, indexed by
      the player count, and `AdjustLevel` spends it on one of the three abilities.
      The sheet carries the three `Increase` gadgets in the original's own order
      (`ab1`..`ab3`), lit only while the experience covers the cost and the ability
      is under five, as the status screen at 0xd3a7 lights them

## Phase 4: combat depth

Built, and most of it turned out to be translation: the knight's controller
(`ControlKnight`, `KnightAttack`, `CheckBlock`, `KnightGotStruck` and the
`*Struck1` routines it dispatches to) reads cleanly, and the pieces that had been
taken for design were in it. Three corrections to the record came out of reading
it, and `TASKVM.md` carries them: DS:0x700 **is** written, by `OSWITCHES` off the
title's gore row, and zero is gore on; `KnightBloSw` is the block table, not the
blood; and the scripts are 239, not 236, because `SpeedKnife`, `Knife` and
`Blood1` carry no encounter prefix and were filtered out. **Every part checked by
looking**: each of the eight attacks against its `tools/taskvm.py` composite frame
by frame, the dagger across the arena into a trogg, a held block against a
knight, the evade against a spear, a troll bleeding where the sword landed, and
the decapitation beside the bloodless collapse from the same fight.

- [x] 46. **Attack variety. Recovered:** `KnightAttack` strips the fire bit from
      the input word, doubles it and reads `Rjoystick` or `Ljoystick` by the
      facing, and the two tables are mirrors, so the direction held with fire,
      relative to where the knight looks, picks the kind: forward is the swing,
      forward and up the up thrust, forward and down the lunge, up the chop, down
      the evade, back the rear thrust, back and up the knife, back and down the
      block. Fire alone is slot 0 of `KnightAttSw`, the stance, which is to say
      nothing; here it is the swing, so that one button still fights and the
      plain opponent, which only knows one button, does too. That one cell is
      ours. The kind is the offset into `KnightAttSw`, and it is what indexes
      every `*Hit` and `*Dam` table, so a creature's blow-taken script now
      follows the knight's attack (`TroggHitAxe`: stabbed by a lunge or a rear
      thrust, cut at the waist by a swing, at the shoulder by the rest) and each
      of those scripts' own `TASKDEAD` picks the death: a trogg stabbed falls,
      one cut at the waist is split. The creatures' kinds are recovered from
      what their routines write into `+0x28` (`TroggSwing` 4, `TroggChop`
      0x10, the spear's lunge 2, the troll's bunt 4 and chop 0x10, the ratman's
      slash 4, Balok's uppercut 4, the dragon's bite 2, the demon's slap and the
      beast's charge 0x10; the mudman never writes one, and a swing stands in).
      Damage is the `*Dam` entry against the default attack's, so the chop is
      twice the swing (`CalcDamage` doubles it) and the rear thrust half; what
      `CalcDamage` adds for strength and the sword is item 40's. Also read and
      **not built**: the moment a blow lands the attacker is handed `+0x12`,
      which is built (`Knight_SwRecover`; the creatures' is their stance, so a
      swing that connects is cut short as the original cuts it); the encounter
      overrides `InitKnightvs*` make to the knight's table (against the spear
      trogg both guard slots become the evade, against the ratmen the block
      becomes `Knight_SwOThrust` and the evade `Knight_SwDThrust`, which are the
      two scripts not in the base table), which are item 37's business; and the
      cursed knight's inverted joystick in `ControlKnight`, item 43's
- [x] 47. **Blocking. Recovered**, from `CheckBlock`: the defender's block
      table (`KnightBloSw`, at `+0x1e`) is read at the attacker's kind and the
      entry has to equal what the defender is doing: a block stops a swing, an
      evade stops a chop, a lunge or a rear thrust. A block only counts from
      the front, which the code tests as the two not facing the same way; an
      evade stops one blow and is spent until the knight walks (bit 7 of
      `+0x48`, cleared in `ControlKnight` on a step). A stopped blow does no
      harm, the attacker bounces into his recovery, and the defender's guard
      replays. Only the knight has a table, and only a knight's or a trogg's
      blows are checked (`KnightKnightStruck1`, `TroggStruck1`,
      `TroggSpearStruck1`); every other creature's, and the dagger's, land
      through `KnightStruck1`, which never asks. One quirk is reproduced because
      it is what the code does: the table's empty rows are zero, an idle knight's
      kind is zero, so an up thrust from the front is stopped by a knight
      standing still and lands only on one mid-swing or turned away. One
      simplification is ours: the original never clears `+0x28` on a blow taken,
      so a reeling knight keeps the kind of whatever he was doing; here a
      reeling or recovering knight never blocks. **Not built**: what the
      original does with a block against the spear (`TroggSpearStruck1` plays
      `Knight_SwEvade` for it), and the Black Knight's own guard (`BKBlock`,
      `_evadechop`), which is his behaviour
- [x] 48. **Weapon state: the thrown dagger, and honestly not the rest.** The
      dagger is the one weapon state the scripts hold, and it is built end to
      end: `Knight_SwKnife` opens with a `TASKTESTEQ` on the dagger count at
      `+0x34` (`SetKnightEquipment` writes ten there) and goes back to the
      stance when it is zero; its last frame calls `KnifeThrow`, which takes one
      off the belt and spawns a task of kind 0x1a on `SpeedKnife` at the
      knight's position and facing, on his own banks (`KN4.OB`, slot 3), with
      `KnifeDam` (three) for its blow; `ControlKnife` hands the task `Knife`
      every frame, twenty pixels forward and the blade, until it touches
      something or leaves the screen (past 0x14a on the right, ten pixels past
      the left). In henge a `Missile` is a task in the bout beside the fighters,
      stepped by the same interpreter, folded into the fingerprint, and
      serialized with everything else; the daggers come off the run's sheet
      going in and what is left goes back onto it, so a thrown dagger is a
      dagger gone. **What the item covers that is not done:** `SWORDFLAG`, the
      drawn and sheathed state (`Knight_SwWalkOn` is the walk on with the
      sword drawn, and nothing here sheathes it), and a dropped or lost sword
      (`TakeSword`, `_no_sword`, `DisplayMSword` are the status panel's side of
      it, and what drops one in a fight has not been found)
- [x] 49. **Gore. Recovered**, and the switch is real: `OSWITCHES` toggles
      DS:0x700 off the title's gore row, `DisplaySelect` prints `ON` for zero,
      and every `TASKSKIP` and every part flagged 0x80 reads it. It goes through
      as `Bout::bloodless`, into every task step, fighter or missile, and into
      the plain opponent, which comes in for the finisher only with the gore on
      (`TroggAttack` checks DS:0x700 before it). What the flag reaches: the
      blood on every creature's blow-taken frames; `Knight_SwDeCap`, whose
      `TASKSKIP` becomes the collapse; the trogg's `*_Split` deaths, which skip
      to `*_CollapseDead`; and `Blood1`, the spray `AddBlood` starts at the
      strike point on bank table 4 (`BLO.CEL`), which `TrollStruck`,
      `BalokStruck` and `DragonStruck` call and nothing else does, and every
      part of which is gated. **Dismemberment:** the knight kneels for twenty
      frames of `Knight_SwDeath` with two `BODY` parts still on him, and a blow
      that finds them in that window is the finisher: a swing takes the head
      (`MudmenStruck1`, the path every creature's and the dagger's blow takes;
      `KnightKnightStruck1` does it for any blow from a knight), anything else
      is `Knight_SwCollapse`, the fall. The creatures' own decapitations are
      already in their `*Hit` rows and come with 46: `Ratman_HitOnHead` and the
      beast's `UpperHit` for a blow from above, each with its own death. **Not
      built**: `Knight_Explode`, which `TrollOHead` plays for a troll's chop on
      a dead knight, and the spear's `TroggSpear_Toss`, because both are the
      creature's finisher and that is item 37; `DrDropHead` and `DrDropClaws`,
      the dragon's, which are 36's; and the screen shake `ShakeADD` asks for

## Phase 5: the shell

Independent of everything. Makes it feel like a game rather than a demo.

- [x] 50. **Title screen.** The wordmark was in the font: `BOLD.F` has 76
      frames and the glyph map only ever used 66, and frames 73, 74 and 75 are the
      `Moonstone / A Hard Days Knight` logo and the two credit lines. The option list is
      `DoOptions`: four rows, a player count of one to four, a gore switch, practice combat
      and moon quest, clamping at both ends rather than wrapping, with the arrow at `ARX`
      50 and at the four heights `MOON:ARR` gives it, 85, 110, 148 and 168.
      **The rows themselves are recovered too**, out of the same span of DGROUP the
      unpacker used to leave stale: `MOON:OPT1a` is a chain of six ten-byte
      `{text, x, y, flags, next}` records that `DisplaySelect` hands to the message walker,
      and they read `Players` and `Gore` at x 86, the player count and `On`/`Off` at
      x 214, and `Practice` and `Select Knight` centred, at y 83, 108, 150 and 170.
      `Players N`, `Gore on`, `Practice combat` and `Moon quest` on an even eighteen-pixel
      step were this project's own wording and spacing, and are gone.
      What it is drawn over is recovered: `_LOADER:MoonPic` is the string `CH.PIV`
      and `0x87c3` loads it, keeps a copy, and draws the wordmark and the two credit
      lines on it, while `DisplaySelect` blits the wordmark again ten pixels higher.
      **There is no attract mode**, and the one that was here has been removed:
      `DoOptions` polls the input and dispatches, with no idle count and nowhere to go,
      so the original's title sits there until somebody presses something. Ours cycled
      ten of the intro's files as though each were a picture, and three of them are not:
      `bg1a`, `bg1b` and `bg1c` are the tile sheets `INTRO.STI` arranges into the opening
      panorama, so one shown raw was half a moon above a row of trunks
- [x] 51. **Character select, and it stands on nothing.** `SEL.CEL` and the rules from
      `ChooseKnight`, `ChooseRefresh`, `FindChosen` and `ChooseFIRE`. **There is no
      backdrop.** This screen was drawn over `CH.PIV` here for a long time, and that was an
      invention: `ChooseRefresh`'s first call writes `0x0f02` to the sequencer's map mask
      and `rep stosw` of zero over `0x2000` words, which clears every plane to palette
      entry 0. The only artwork on it is four portraits on black.
      `ChooseRefresh` runs `bp` from 0 to 3 and blits cel `bp + 2` at `CCOL[bp]` with
      `cx = 0x50`, through the ordinary cel blit and **with no colour substitution at
      all**: the portraits are already painted, and ours used to draw them through a
      recolour built from the knight shades, which is colouring coloured artwork and is why
      they came out as smears. `CCOL` is 12, 88, 164 and 240.
      **`SelectPAL` is recovered**, which is what makes the rest of it work. It is the one
      palette in the game that lives in the executable rather than in a picture, it sits in
      the bottom of DGROUP, and that span was unreadable until the unpacker was found to
      stop before the EXEPACK stream ends (`docs/REVERSING.md`). The baker reads it out of
      the image as `palette.select` and checks it: greys at 1 to 4 that all four portraits
      share, the bold face's five entries at 5 and 9 to 12, and each knight's own colours,
      which come out blue, gold, emerald and red and so agree with `KnightGlowColours`
      arriving from somewhere else entirely. The heading is `MOON:CRText`, out of the same
      span: `Select a Knight`, flag 1, which is centred, at y 5.
      **The chosen knight glows.** `SEL.CEL` cel 1 is a 64 by 76 hollow frame every pixel
      of which is index 15, no portrait touches 15, and the screen behind is entry 0, so
      entry 15 on this screen is that frame alone. `ChooseKnight` calls
      `COLOURGLOW(0x0f, 0x088, 1, 0)` and `SelectPAL` puts `0x066` at 15, so the frame
      breathes between those two for as long as the screen is up and nothing else moves.
      That effect was recovered once, wired to a screen standing on `CH.PIV`, where entry
      15 is the night sky, and taken out again because it repainted the whole background.
      The backdrop was the mistake; with it gone the glow is back.
      **The four do not differ in stats**: `InitKnights` separates them by name, colour and
      which corner of the map they start in, and hands all four the same block. The data
      allows four different ones; what ships is the original's.
      **And they are named now.** `BNAME`, `GNAME`, `ENAME` and `RNAME` at image 0x128ca,
      0x128e0, 0x128f6 and 0x1290c are `SIR_GODBER`, `SIR_RICHARD`, `SIR_JEFFREY` and
      `SIR_EDWARD`, in blue, gold, emerald and red order: `InitKnights` writes the name
      pointer and the corner on the arm for each colour index, `ChooseFIRE` writes `NAMEy`
      and that same index on the arm for each portrait, and the two agree, so the initial
      is the colour's rather than the name's. The underscore is the blank glyph: `TextASCII`
      sends both `'_'` and `' '` to glyph 69, and the original keeps one because `TypeName`
      finds where typing starts by scanning for the first space. `SIR BANNER`, `SIR DWAIN`,
      `SIR BALAIN` and `SIR GUNTHER`, which this project used until now, are
      `Enemy1Name`..`Enemy4Name`, which `InitGameStart` gives to the seats nobody takes:
      they are the computer knights.
      **Ours on this screen**, and marked so: the line saying whose turn it is, the name
      under each portrait, the stat line along the bottom, and the `Player N` that stands
      in an empty slot. The original draws none of them; it draws the chosen knight's name
      at (50, 50) once he is taken, out of `BNAME`, `GNAME`, `ENAME` or `RNAME`
- [x] 52. **A real status panel.** The knight record and `DisplayKnight` give the whole
      sheet: strength, constitution and endurance at `+0x2e`..`+0x30`, life points, gold,
      daggers, experience, health and its maximum, the weapon and the armour, with the
      arithmetic that turns them into a fight. One plate per fighter along the bottom of an
      arena, and the sheet itself on a key
- [x] 53. **Recovered: the pointer and the gadget table, both.** `_STATUS:MovePointer` is
      the whole pointer: two pixels a frame in whichever direction is held, clamped to
      `0..0x13a` by `0..0xc2`, with fire clearing `PointerFLAG`. It is driven by the
      **stick, not by a mouse**. `PO.CEL` is the art, one 16 by 18 arrow the packs had
      decoded and never drawn. The gadgets are `GadgetSlot` (98 records of twenty bytes,
      a zero width meaning empty), `CLEARGADGETS`, `AddIconGadget` (position, an id, a
      payload word, a pointer to a ten-byte text record, and a size taken from the
      **icon's own cel header**), `CHECKGADGET` (the same two-rectangle overlap helper at
      0x9f0d that `CheckGROOC` uses, with the pointer's rectangle **one pixel square**)
      and `HotGadget` (what fire does with the one underneath). All of it is in
      `henge_core::pointer`, hit test reproduced as written including its asymmetry.
      **Ours:** what a gadget's payload means. The original's nibbles name its own
      trading screen's operations; here a gadget carries an id the screen that registered
      it interprets, because our screens are already menus with a highlight. The pointer
      reaches the title's option list, the four portraits on select, a town's menu and the
      character sheet, which is the screen `_STATUS`'s own gadgets belong to. A real mouse
      moves it as well, because a window with a mouse in it should behave like one
- [x] 54. **Recovered whole: one record, one chain walk, three routines.** A message is a
      linked list of ten-byte records: text pointer, x, y, flags, next. `GFX:TextPTop`
      reads the flags (bit 0 centre between `TextLeftBorder` and `TextRightBorder`, bit 2
      right, bit 3 the bold face's three-pixel kerning) and `TextPDone` follows `+8` until
      it is zero. The three kinds are three routines in `_LOADER` and they differ by one
      thing each: `WAITMESSAGE` takes **no argument** and reads `WaitMES[WaitCOUNT]`,
      stepping and wrapping at fourteen; `OCCURMESSAGE` takes a chain; `INSTRUCTMESSAGE`
      takes a chain and installs its own six-word palette ramp before the fade, so it
      arrives in another colour. All three blit `MESSAGE.PIV`, which turns out to be the
      stone circle in silhouette against a night sky. Every caller was found by scanning
      for the calls, so which door shows which kind is recovered and not assigned: the two
      cities and the Valley are occurrences, the stone circle and the game over are
      instructions, and the wizard's tower is the one door that takes one off the wait
      pile. **Also recovered on the way: `GFX:TextASCII`**, the 95-byte glyph map the
      project had read off the artwork instead. It agrees with the reading exactly; the
      only correction is the bold font's glyph 71, a slash and not a bar, and glyph 63 is
      the one entry no character maps to. The metrics come with it: `TextP` advances by
      the cel's own width, less three for the bold face, which is why a recovered line
      now fits the screen it was written for. **Ours:** the colour an instruction message
      comes up in, since the fade machinery its ramp drives is not built; and showing the
      fourteen on the between-days screen, which is where they were already
- [x] 55. **`INTR.EXE` read to the end, and the intro is the original's.** It unpacks with
      `tools/symbolmap.py` unchanged: four modules, **319 symbols**, 262 corroborated.
      What had stopped everything is that **the image that tool writes is still packed**:
      its tail is Microsoft EXEPACK's run-length stream, so every zero-filled span of the
      program is four bytes standing for hundreds. That is the whole of why the code
      addresses appeared to need a fitted seven-step correction and the data addresses
      appeared to be 14,911 bytes out. Expand the stream and **every symbol, code and
      data, lands on its own byte with no correction at all**. After that:
      **`.STI` is a tile map** and the opening is a **vertical pan**: `FindTile` cuts tile
      *n* at `((n % 10) * 32, (n / 10) * 25)`, the 32x25 grid ten across a `CMP` uses, and
      the map is ten big-endian words to a row with the sheet chosen by `n / 80`, so
      `INTRO.STI` is 48 rows: **a 320 by 1200 panorama** out of `bg1a`, `bg1c` and `bg1b`,
      panned 0 to 1000 on its own ramp of eleven thresholds and eleven speeds.
      `INTRO1.STI` is not a map at all: it is byte for byte `F09.T` and `SW9.T`.
      **The captions have coordinates**: ten-byte records with bit 0 centring the line,
      which is why every x is zero, and the story cards go over `MESSAGE.PIV`, not over a
      plate, so the black bands that used to cut a picture in half are gone entirely.
      **The credits are the loading screens** and their pairing is read off the table at
      `DS:0x152` rather than guessed, so `Programmed by` gets Anthony Mack and Nicholas
      Snape and `Music and Sound by` gets Audio Visual Magic. **The cast animates** on the
      intro's own scripts, whose opcode table has one slot more than the game's, and the
      scene lengths are those scripts' own tick counts. **`MINDSCAP` is a PIV** with no
      extension, which is the only reason nothing had baked it. And **the intro is the
      first half of `INTR.EXE`, the ending the second**: given a command tail it plays
      `The End`, `co.sti` and `bg5`, `bg7`, `bg8` instead, so those three plates are
      deliberately not in the intro. **Ours:** how long the logo and each credit screen
      holds, since in the original that is a floppy's seek time; the rounding of 9.1
      frames a second onto sixty ticks; and the dark ring round a caption, standing in for
      the glyph shading silhouette text throws away
- [x] 56. **Save and load, ours by design.** The original has none: `MOON.CFG` is a
      sound-card profile and there is no slot, no file and no routine anywhere in the
      2,223 symbols. What made it small is that the simulation was already built for it.
      A save is `Run` plus `Overworld` plus the title's settings plus `WaitCOUNT`, with a
      magic string, a format number and a fingerprint over the lot; `Overworld` gained
      `Serialize` and both gained a `state_hash` beside `Bout`'s. **Both seeds go in**, so
      a reloaded run is robbed and mends on the same steps a continued one would have.
      Three refusals that are told apart on purpose and none of which loads half a game:
      not a save, a save this build cannot read, and a save whose contents do not match
      its fingerprint. **No path of any kind is in the file**, and a test asserts it. The
      format lives in `henge_core::save`; reading and writing the file is
      `henge-desktop`'s, because core does no I/O and keeps its one dependency

## Phase 6: the world

**Done.** The map's own tables were already in the game (57, 58, 60, 61); now every
door on it opens, the lairs are on it, and the moon runs.

Most of this turned out to be translation rather than design, because `_TAVERN` and
`_WIZARD` are two whole source modules with addresses and the temple is one routine in
`_STATUS`. Three corrections to the record came out of reading them, and they are noted
against 31, 64 and 69. **Every screen was checked by looking**: the tavern over its own
painted panel of five stakes, the dice table with the throw drawn where `RollDice` blits
it, the town healer and the mystic behind their own greetings, Math on his balcony, the
druids in the circle, the between-days screen on a full and on a gibbous moon, a lair on
the map, a lair entered, and the spoils page after its guardian fell.

One thing that is worth knowing before anyone reads the numbers below: **the coordinates
of everything on the map except the hermit are recovered.** `MOON:MapIconsTABLE`,
`ForestLairs`, `LairLocation` and `LairType` were all inside the 2,906 bytes of DGROUP the
load image used to carry as a stale duplicate. That span is readable now
(`docs/REVERSING.md`), and `henge-bake` reads all four out of it: the two towns,
Stonehenge, the Valley of the Gods, Math's tower, and the twenty four lairs with their
guardians and head counts. What that replaced, and how far out it was, is in
`docs/COMPLETE.md` 4.1 and 4.3. The hermit is not in the original at all and is the one
place on the map still sited by hand.

- [x] 57. **Recovered: `_MAP:MapType`, 40x26 bytes, one family code per 8x8 block of the
      map picture.** Codes 0, 2, 4, 6 are plain, forest, swamp and waste, which is the
      order `MOON:ColourBackdrop` compares against. `_MAP:CalcKnGrid` builds the index from
      the traveller's token: `((x+4)>>3, (y+10)>>3)`, the middle of his feet. Checked by
      drawing the grid's own boundaries over the map artwork, where they follow the
      treeline, the marsh edge and the mountain ridge. The location graph is **recovered
      whole**: the nine kinds of place and their menu lines, the two towns' walk-to
      points, and `MOON:MapIconsTABLE` itself, which was thought to be uninitialised data
      and is not; it came back with the rest of the bottom of DGROUP and gives all nine
      places their corners
- [x] 58. **Recovered: four tables of eight, and a counter, not a roll.** `PlainTable`,
      `ForestTable`, `SwampTable` and `WasteTable` each hold eight filename pointers, and
      generating an arena reads `Table[counter]`, then `inc counter` and `and counter, 7`.
      The six lair layouts per family are not in those tables and so never come up on the
      road. Also recovered: `TileTable` gives plain and forest the same `FO1` scenery
      sheet, and a placement whose selector byte is 4 draws from `FO2` instead, whatever
      the family. Checked by compositing an arena all three ways and looking: only that
      one makes a coherent picture
- [x] 59. **The moors, settled: it is not a fifth family, it is the one this
      project calls the glade.** `_LOADER`'s public list runs `LOADREGION`,
      `GENERATELANDSCAPE`, `GENERATEMOORES`, `GENERATEFOREST`, `GENERATESWAMP`,
      `GENERATEWASTE`, `LOADTILEV`: one dispatcher and four families, in landscape
      code order. `GENERATELANDSCAPE` (image 0x8cb3) reads the code out of
      `[0x694e]` and calls through a four-word table whose entries are link
      addresses, so it takes the same correction every other stored code address
      does; corrected, they are 0x8cd3, 0x8d02, 0x8d31 and 0x8d60. The first of
      them loads `GLB1.CMP` and reads `PlainTable[PLAINCOUNT]`, which is
      `GL1.t`..`GL8.t`. So the moors is landscape code 0, the `GL` layouts over the
      `GLB1` sky, and `MapType` holds only 0, 2, 4 and 6, so there is no fifth code
      and no missing `MO*` file. It renders, and it always did, under a name of
      ours.
      **Two real faults came out of looking, and both are fixed.** The baker
      decided which family a layout belonged to by the first two letters of the
      *family's own name*, which worked only by the accident that this project
      named the moors after its `GL` files; a family renamed to what the original
      calls it would have sent all fourteen of its layouts to the fallback and
      drawn them over the forest's sky. Each family now declares its own file
      prefix, and a test holds the two together. And the baker dropped any layout
      with an empty placement list, which was meant for the `F09`/`SW9` stubs and
      also threw away `SWL2.T`, a real swamp lair floor that has bounds like its
      neighbours and no scenery on purpose: the fourteenth lair had no ground to be
      fought on. The stubs are caught by their bounds, which is what was always
      catching them, and the pack is 56 layouts rather than 55
- [x] 60. **Recovered: nothing on the map is impassable; ground is slow instead.**
      `_MAP:MapSLOW` is a second grid on the same index holding a two-bit mask, and
      `_MAP:CheckSLOW` refuses the step when `counter & mask` is not zero, having already
      charged it to the day. Forest and marsh are half speed, the mountain spine a quarter.
      The only hard limit is a rectangle: `_MAP:HawkBorders` clamps the token to
      `0..=310` by `0..=190`.
      **That is the overworld, and it is right. The arena is nothing like it, and what
      this item used to imply about arenas was wrong.** An arena's `.T` header is a count
      and that many impassable rectangles, not one walkable box, and the ground is what is
      left below them. `CheckBorder` (image `0x40d0`) and `SBORD` (`0x4552`) both work by
      clearing bits in a per-actor byte of allowed directions, `+0x26`, recomputed every
      frame: bit 0 right, bit 1 left, bit 2 down, bit 3 up. `CheckBorder` probes the actor
      twenty five pixels ahead in whichever way he faces and holds him inside columns 10
      to 320, writing the column back to the limit it crossed, and holds the task anchor
      between depths 30 and 155; `SBORD` walks the arena's own rectangles and refuses up
      to anyone whose anchor plus `0x2f` has reached a rectangle's bottom in that
      rectangle's own columns. So **the tree line is a ceiling, not a wall**: walk into it
      and you keep your other three directions and slide along it.
      Three things fell out of reading it. `FO7`, `SW6`, `SWL2` and `GLL4` carry more than
      one rectangle, and reading their headers as one had been costing them their
      scenery, `SWL2` all of it: it is not an empty lair floor, it has eighty nine
      placements. `AddKnight` stands each arrival a quarter, a half or three quarters of
      the way from the deepest rectangle down to row 200, through `FindQuarterBORD`,
      `FindHalfBORD` and `Find3QuarterBORD`, which is where the fighters now start.
      And **only the knight is bordered in the original**: `SBORD` has one caller and
      `MonsterWalk` is not it. Running the creatures through the same gate is ours.
      **Still not right**: row 200 is the foot of the screen, so the original fights over
      the whole of it, while this engine draws its own status strip over the bottom
      thirty two rows. The strip is ours, the original has no equivalent of it, and a
      fighter who walks all the way down now goes behind it
- [x] 61. **Settled, and negative: the overworld map does not scroll or pan.** `MAP.CMP`
      is one 320x200 picture, `_MAP:SHOW` hands the token's position straight to the
      blitter with nothing subtracted, and `HawkBorders` bounds that position to exactly
      one screen. `_MAP:ScrollINPUT`, despite the name, reads the keyboard. The `SCROLL`
      and `PAN` symbols carry no addresses and belong elsewhere; `SCROLLX` in `_STATUS` is
      the status panel's own icon cursor
- [x] 62. **Lairs: twenty four of them, and the quest starts here.** The table is 24
      records of 18 bytes built by the initialiser at 0x1e00, and what it does is
      recovered whole: `NUM16` four times to plant one key per family, six records
      apart, then `LairFill` rolling `MOON:LairRND` for each floor (half gold, a fifth
      magic, the rest both, and nothing empty because the fourth threshold falls through
      to the third's call). Gold is the wizard's own gift routine called with `dx` set,
      ten to thirty one; magic is his bestowal called twice. `LairWon` pays a point of
      experience the first time only, and `CheckLairClear` writes 0xffff over a lair
      that is beaten *and* stripped, so one you could not carry out of is still on the
      map to come back to. **`MOON:LairFile` is recovered**, which is more than the
      record had: it sits four bytes past the end of the stale duplicate and gives the
      24 arena layouts in order, `fol1`..`fol6`, `wal1`..`wal6`, `swl1`..`swl6`,
      `gll1`..`gll6`. That order is also the key order, so lairs 0 to 5 are the
      forest's and hide the forest key. **Ours**: where each stands, which is a search
      over the real terrain grid for cells whose whole neighbourhood is that family's
      ground, clear of every other place, spread by farthest-point sampling; and which
      guardian each holds, which is the family's own road creatures for the first three
      and then the beast and Balok, the two the road never produces. The demon and the
      dragon are left out, because their set pieces are items 33 and 36 and because two
      hundred and fifty hit points against a twenty point knight is not a fight
- [x] 63. **Moon phases, and the between-days screen.** `MOON:EncounterFini` counts the
      day, moves the moon every fourth and closes the cycle every eighth, so thirty two
      days come round; `InitGameStart` writes 0x2d, so a quest opens on the full moon.
      The screen is the routine at 0x8e5b: `Next Day` at y 95 over `CH.PIV`, the night
      sky the select screen uses, with cel `Moons[MoonCount]` of `KI.CEL` blitted at
      (119, 12). **Ours**: the eight byte `Moons` table itself, which is in the
      unreadable part of DGROUP, so the cycle is five pictures over eight steps waning
      and waxing back, which is the one arrangement that uses every picture and returns
      to where it began; and putting the hint under it. The fourteen hints are
      `_LOADER:WaitMES` verbatim, in its own order, cycled the way `WaitCOUNT` cycles
      them, and the original shows them while a disk loads, which is a thing henge does
      not do
- [x] 64. **What the moon gates, and it is more than the record thought.**
      `SetRatmenTables` reads the phase every time a ratman fight is built: five hit
      points and a slash of one most nights, seven and three under 0x2d, twelve and five
      under 0x31. That is on the ratman's own definition in the pack rather than in the
      engine, so a pack can give the moon to any creature. `MOON:CalcDamage` doubles a
      knight's blow while he carries the moonstone whose night it is, and `MOON:Henge`
      ends the game for a knight standing in the circle with it; both are built and
      neither can fire yet, because nothing hands out a moonstone until item 71. The
      code and the game's own hint disagree about the ratmen and the code is what is
      reproduced: `RatNewMoon` is the strongest of the three
- [x] 65. **Tavern.** The whole of `_TAVERN` is the dice table and the henge, so the
      tavern *is* the dice game: `TavernOpenScene` turns an empty purse out of the door
      before anything is drawn, and the six gadgets `TAV.PIV` paints are five stakes of
      one to five gold and an exit. The live menu goes exactly over that painted panel
- [x] 66. **Temple and mystic.** The temple is `_STATUS:SellToTemple` and `GoldSell`:
      one off the record, the price shifted right once into a purse that saturates at a
      hundred and fifty, and a sword sold out of the hand leaves a long sword in it. Its
      gadget list is `se7`..`se17`. The mystic is `MYS.PIV` and the routine at 0xb935:
      a donation, an ability picked before the roll, and `MysticUpDown` walking
      `DonationTAB` for the delta the donation buys, good at fifty or under. Its lines
      are `MY1a`..`MY7b` verbatim. **Not built**: the temple's other counter, which buys
      and sells a moonstone (`pu18`, `se18`, `BuyMoonstone`), because item 71 owns the
      moonstones; and the original's coin-at-a-time donation gadget, in place of which
      three fixed amounts are offered
- [x] 67. **The wizard.** `WizardIntro` on the way in, one roll plus the grudge against
      thirty, seventy and ninety for magic, an ability, gold and the toad, and the
      fourteen `WizardText` lines cycled by `WIZGOLD_CNT` and `WIZMAG_CNT`, all
      verbatim. The grudge is real: 0xff for a knight he has never met, which rolls one
      lower and cannot reach the toad, and seventy after every visit, coming down by ten
      a day, so the second visit of a day is dangerous and the third close to certain.
      The toad costs what it says: `_MAP:NextWHICH` tests `[si+0x3a]` and passes to the
      next knight, so three turns go by without a step. His tower is the one the map
      paints in the northern waste, which is ours
- [x] 68. **The stone circle.** `MOON:Henge` tests the moonstone bits against tonight's
      moon before it offers anything else, and short of that the druids take an
      offering: a life point, a full mending and a ratman's bite lifted, for anything
      magic that is not a weapon, armour or one of the quest's tokens. The winning
      branch is built and cannot fire until item 71 hands out a moonstone. `The druids
      prepare for the ritual` is `_TAVERN:HengeWait`; `HengeInstruct` is in the
      unreadable part of DGROUP, so the rest of what they say is ours
- [x] 69. **The dice game, and it was not design after all.** `_TAVERN:RollDice` rolls
      three bytes with `and ax, 7; cmp ax, 5; jg` and re-rolls, `DiceSort` bubble sorts
      them, and the sorted throw is compared two words at a time against `DiceODDS`,
      eleven records of three faces and a multiplier at DS:d125. Nothing pays for a pair
      unless the pair is the first face; three of the first face pays thirty and three
      of the third pays twelve, so the table is not monotone and a rule of thumb would
      have got it wrong. The purse saturates at a hundred and fifty. The three faces are
      blitted at (115, 15), (49, 38) and (75, 88) over `DICE.PIV`, which is a picture of
      three dice already on the wood, and the words go on the plank beside them where
      `BETLOSER`, `PLAYERPOT` and `CONT` are written. **Not built**: the shake, which is
      `DD_ShakeDice` and `DD_ThrowDice`, two animation scripts of a hand over the table

## Phase 7: the quest

**Done, and it was translation after all.** The plan said "almost none of the rest is
recoverable from the symbols, so most of it is design and playtesting rather than
porting". That was wrong, and it is the third time on this project that a phase written
off as design turned out to be sitting in the executable: `MOON:Valley`, `MOON:Henge`,
`MOON:KnightWonGame`, `MOON:WhoLived` and the routine at 0x617 are the whole chain, and
MOON's text pool at image 0xd6e0 has the words for every step of it.

**Every screen was checked by looking**: the Valley's ring on the map, the gate shut with
the message that shuts it, the Guardian standing in a marsh arena, the moonstone page
after he fell, the keys of a real lair on the character sheet, the beating that costs
three life points, the stone circle on the stone's own night, the victory page over
`BG8.PIV` and the game-over page over the map.

- [x] 70. **The four keys, and what they are for.** 62 planted them; this is the door
      they open. `MOON:Valley` is `cmp byte ptr [si+0x14], 0xf` and nothing else will do:
      three keys is `NoKeysMessage`, which is recovered verbatim, `You must have all four
      keys / to enter the / Valley of the Gods`. The map line is recovered too,
      `_MAP:knvalley` `Enter Valley of the Gods`. Beating the Guardian writes
      `mov byte ptr [si+0x14], 0`, so **the keys are spent**, and a second moonstone means
      four more lairs. The keys are also on the character sheet now, which is
      `_STATUS:StatCheckKeys`: `KI.CEL` cels 5 to 8 at x 0x4c, 0x5e, 0x70 and 0x82,
      eighteen apart, one slot per bit, an empty slot for a key you have not found. Its
      row, y 0x6f, is the one thing not kept: henge's panel puts the armour there
- [x] 71. **The moonstone, and where it comes from.** The Valley of the Gods, which is
      the fourth thing on `MOON:StackMessages`' list and had no door until now. Behind
      the gate is the demon: `MOON:FightDemon` calls `InitKnightvsDemon`, which writes
      250 health, one monster and `ColourBackDrop` 4, so **the Guardian is fought on
      marsh**. Winning is `add word ptr [si+0x36], 3`, the keys cleared, and
      `al = 1 << (rnd & 3)` OR'd into `+0x16`: **one of the four stones, at random**.
      What it says is `ValleyEnter`, recovered verbatim. Losing is
      `sub byte ptr [si+0x31], 2`, on top of the one `MOON:Combat`'s own closing
      `call WhoLived` already took, so a beating there costs three of the five life
      points and leaves the keys where they are. **Ours**: where the Valley stands, by
      the same terrain search the lairs use but on marsh and as far from either town as
      the marsh allows; and the picture behind the gate. Also recovered and not built:
      `BuyMoonstone` and `SellMoonstone` are a trade between two knights' records, not a
      shop, so the recovered prices (`Buy Moonstone for 20 GP`, `Sell Moonstone for 10
      GP`, `Buy Key for 12 GP`, `Sell Key for 6 GP`, and the half is `GoldSell`'s own
      `shr ax, 1`) are on the items and no counter in a one-knight run will take them
- [x] 72. **Win condition and ending.** `MOON:Henge` was already built and waiting;
      handing out a moonstone is what let it fire. `KnightWonGame` shows `VICTORY`,
      `You have completed / the quest`, and then **quits to DOS** with a byte in `al`:
      the low nibble is which moon (0x2e -> 2, 0x2d -> 4, 0x31 -> 3, else 1) and the high
      nibble which knight (3 -> 0x10, 0 -> 0x20, 1 -> 0x30, 2 -> 0x40). Whatever reads
      that byte is in `INTR.EXE`, which is item 55 and has never been examined, so the
      ending screen is **ours**; `Tally::code` works the byte out anyway. It is drawn
      over `BG8.PIV`, the only full-screen picture MOON names by file, which sits in the
      text pool immediately after the victory lines. That placement is an inference and
      not a traced reference, and it is marked as one in `quest.rs`
- [x] 73. **Scoring and the final tally.** **Ours, all of it**: the original counts
      nothing and neither of its two endings is a page you can read. Seven lines, every
      number already on the run: the day, wins out of fights, lairs cleared, gold and
      experience, life points left, keys, and the moonstone. One label is the
      original's, `Life points left`, off the status panel
- [x] 74. **Losing properly.** **Recovered: `MOON:WhoLived`**, which every fight ends on
      because `MOON:Combat`'s loop closes with `call WhoLived`. A knight on nothing is
      not finished: his health goes back to its maximum and `sub byte ptr [si+0x31], 1`
      takes a life point. The run ends when the last one goes, which is what
      `_MAP:CheckEncounterDone` tests when it decides a knight on the map is a grave
      (`cmp byte ptr [si+0x31], 0; jg`), and the routine at image 0x617 answers with
      `GameOverMes`, recovered as `Player      ` and `GAME OVER`, and `jmp StartAgain`,
      which is the title screen. So five deaths to a run rather than one, and the game
      goes back to the title rather than restarting where it stood. A run that ends
      indoors walks back out to the map first, because that is where the tally is

## Phase 8: polish

Any time. None of it blocks anything.

- [x] 75. **Palette fades and colour cycling. Recovered, all four routines.** The
      original keeps a live palette of 32 twelve-bit colours and hands it to the DAC as
      `nibble << 2`; `ADDCOL` queues `COLCON` on the frame list, and `COLCON` walks six
      `CYCLES` slots and six `GLOWS` slots once a frame. A **cycle** is a first index, a
      last index, a direction and a period, and every period frames the entries in that
      span rotate by one. A **glow** is one index walking one step a channel towards a
      target colour every period frames, and on arrival the target and the colour it
      started from swap, so it breathes; `DYNAMIC` is that step and it moves red, green
      and blue independently. The **fades** are a separate pair and both are sixteen
      steps, one a frame, linear: fade in accumulates `target * 16` into a 16.8 channel
      and shows the high byte, fade out reads the DAC back and subtracts to black. All of
      it is in `henge_assets::palette`, applied to the palette on its way to the screen,
      so the framebuffer is untouched.
      **What the game actually installs is two things, and both are built**:
      `_MAP:MapEffects` calls `COLOURGLOW(0x1f, 0x0ff, 1, 0)` and `COLOURCYCLE(0x15,
      0x17, 1, 0x0c)`, whose handle it keeps in `RiverHANDLE`, so on the overworld
      entries 21 to 23 rotate every twelfth frame and that is the water moving;
      `MOON:ChooseKnight` calls `COLOURGLOW(0x0f, 0x088, 1, 0)`, so the select screen's
      night sky breathes towards a teal. Both are in the pack as `data.palette.effects`,
      keyed by screen, so a replacement pack can animate its own palettes.
      `MudmenGlowOn` is `COLOURGLOW(0x0e, 0x100, 2, 0)` and hangs off the fight rather
      than the screen, exactly as `InitCombat` installs it.
      **Every screen change fades in**, and the two screens that go out on their own
      fade out: the between-days screen, which is `FADEOUTDAY`, and a message chain,
      which all three of `WAITMESSAGE`, `OCCURMESSAGE` and `INSTRUCTMESSAGE` end on. The
      original's other fade outs cover a disk read that does not happen here.
      **Recovered, and wired since item 78**: `KnightGlowOn`, which glows palette
      entries 6, 7 and 8 towards the knight's own brighter triple when he is down to ten
      health, and 9, 10 and 11 for a second knight. It could not be wired while henge
      recoloured by hue substitution, because those entries were backdrop colours here;
      in the original they *are* the knight, `ColourKnight` having written his three
      armour colours into `BattlePal+12` (0x00a/0x007/0x004 blue, 0xf80/0xc50/0xa30
      gold, 0x8c6/0x593/0x251 emerald, 0xf22/0xb22/0x700 red, 0x206/0x103/0x001 for a
      fifth), and since 78 they are here too
- [x] 76. **Gamepads, with calibration and debounce; and rebindable controls.**
      The reading is recovered and the binding table is ours.
      **Recovered.** The whole game runs on one five-bit word, `0x01` right, `0x02` left,
      `0x04` down, `0x08` up, `0x10` fire, and `Rjoystick` and `Ljoystick` index the eight
      attacks on it. `_KBD:JOY0` and `_KBD:JOY1` build it from the gameport: `out 0x201,
      0xff` fires the one-shots, the loop counts until each falls with a cap of `0x400`,
      an axis that reaches the cap is a stick that is not plugged in and contributes
      nothing, and the count is compared against `JOY_XMIN`, `JOY_XMAX`, `JOY_YMIN` and
      `JOY_YMAX`. The button is `dl = al & (al >> 1)` over the pair of bits for that
      stick, so either button fires. `Fix_JoyStick` calibrates in two prompts, recovered
      verbatim (*Move joystick to / the top left / and press the / fire button.*, then
      *the bottom right*), `GetJoyTL` and `GetJoyBR` record the raw counts and
      **`AdjustJoy` pulls each threshold one eighth of the measured range inwards**, so
      three quarters of the travel is dead. `BOUNCEBUTTON` waits for fire down and then
      for fire up, so a press is worth exactly one thing. `GetInputDevice` throws away
      left with right and up with down before anything sees the word. And the reader at
      image `0x81ec` gives **the original's own keys**: player one Enter and the arrows,
      player two Tab, W, X, A and D, OR'd with `JOY1` and `JOY0` respectively.
      All of that is `crates/henge-desktop/src/input.rs` and unit tested.
      **Ours.** The binding table, because the original's keys are five `mov ax,
      <scancode>` instructions and there is nothing to port. It is data: actions to
      sources, by winit's and gilrs' own names, saved to `henge-controls.json` beside the
      save. `--bind 0:fire=Enter` rebinds from the command line, `--controls-write`
      writes the file, `--original-keys` starts from the original's layout, and the
      calibration is saved alongside. The default calibration's two corners are ours too:
      `AdjustJoy`'s eighth off a modern pad's true extremes would want the stick pushed
      seven eighths of the way, so the default pretends the corners were answered at six
      tenths and puts *that* through `AdjustJoy` unchanged.
      **Pads come from `gilrs`**, which was not already in `Cargo.lock`; winit has no
      gamepad support at all, so there was nothing to prefer it to. It reports no pads
      rather than failing when there is no input subsystem, which is the headless case.
      **On Linux it needs `libudev-dev` at build time**; build with
      `--no-default-features` or turn the `gamepad` feature off without it.
      **Not verified**: anything that needs a pad plugged in. There is none here. The
      binding table, the calibration arithmetic, the debounce, the dead zone, the
      timed-out axis, the opposite-direction cancellation and the winit key names are all
      tested; the actual reading of a physical stick is not
- [x] 77. **Music. Recovered: the tune files gave up their notes.** The plan said trace
      the driver or commission new music. Tracing worked.
      Each `xTUNEn.BIN` is a relocatable x86 driver with the song welded into it:
      `LOADMUSIC` at image `0x900d` reads one to segment `0xd7f`, `Install_Timer` points
      `int 60h` at offset zero, and the timer handler's first instructions are `mov ah,
      1; int 60h`, so the tune ticks at 1193182 / 0x5555, or 54.62 Hz. `ah = 0` starts and
      `ah = 2` stops. `MusicTable` at DS `0x84f0` is eighteen four-byte records: **six
      tunes by three sound cards**, and the letter of the filename says which card.
      `a` writes register then value to 0x388 and 0x389 with the AdLib's own six dummy
      reads between them; `b` is the PC speaker on ports 0x43, 0x42 and 0x61; and **`r`
      is a Roland on an MPU-401 at 0x330, which turned out to be plain MIDI**.
      So `tools/tunes.py` runs the game's own `RTUNEn.BIN` under the same 8086 harness
      the executable was unpacked with, answers the MPU's status port, and writes down
      what the driver sends: note, channel, velocity, start and length, on the driver's
      own tick. All six come out. Four of them loop, and the loop point is found by
      comparing what is sounding tick for tick: tune 2 at 39.7s, tune 3 at 112.1s, tune 4
      at 22.5s, tune 5 at 28.1s.
      **Where each plays is recovered too.** Five callers of `LOADMUSIC`, each followed
      by `mov ah, 0; int 60h`: `load_DiceBACK` starts tune 3 in the tavern's dice game and
      `LeaveTavern` stops it, the routine that opens the henge starts tune 2, `LoadWizard`
      starts tune 2 and `e5$` stops it, `MysticUpDown` starts tune 4 and `MysticFini`
      stops it. **Nothing else in the game has music**: not the map, not the arenas, not
      the title. That table is in the pack as `data.music.places`.
      **Ours: the sound.** The recovered stream is MIDI, so it names a Roland's
      instrument numbers and nothing else; how those actually sounded belonged to a
      synthesiser this project does not have. So the notes are the original's and the
      voices are ours, a handful of wavetables and envelopes chosen by General MIDI
      family with a noise burst for the drum channel, in `henge_audio::music`. It is
      labelled that way everywhere rather than passed off as a recording.
      **Recovered and not built**: `_bestow_done` starts tune 5 after Math's gift, which
      is a moment inside the wizard's tower rather than a room of its own; and tunes 1
      and 6, which `MAIN.EXE` never loads. They ship on disk A with the intro, so they
      are `INTR.EXE`'s, but its own `LOADMUSIC` call could not be traced to a tune
      number, so neither is placed rather than being placed by guess. Both are in the
      pack and both play.
      **Not verified by ear.** There is no sound device here, so the tunes were checked
      by measurement instead: every one renders to the length its loop says, peaks at
      0.6 of full scale with no clipping and no gap longer than a frame, and the spectral
      peaks land on note frequencies (tune 4 on 698.5 and 66.0 Hz, which are F5 and C2;
      tune 3 on 310.8, 278.2 and 245.8, which are the D sharp, C sharp and B of the B
      major chord its own note stream plays). `henge-bake --render-music <dir>` writes
      them out as WAVs for anyone who does have speakers
- [x] 78. **The fight palette. Recovered: `BattlePal`, and the hue substitution is
      gone.** Henge used to tell four knights apart by a substitution table built from
      the backdrop's hues, which was this project's invention, and it drew every
      creature in whatever the backdrop's palette happened to hold at its indices: a
      gold knight came out brown and a forest trogg in the mudmen's grey and gold,
      because `FOB1.CMP` was saved with a red knight at 6 to 8 and the mudmen's block
      at 9 to 15. The original writes those entries at the start of every bout.
      `ColourBackDrop` (image 0x460f) copies the backdrop picture's palette from
      `DS:0x80bb`, where the picture loader at 0x875e leaves it, into `BattlePal`
      (`DS:0x7a80`), and dispatches on the code each `InitKnightvs*` hands it in `ax`:
      beast 0, mudmen 2, demon 4, second knight 6, dragon 0xa, trogg 0xc and 0x10,
      ratmen 0x12, balok 0x18, troll 0x20. The creature routine writes its block from
      entry 9, the second knight's writes `ColourKnight` at 9 to 11, and all of them
      fall into `ColourMainKnight` (0x47e4): `ColourKnight` (0x480e) at 6 to 8, the
      ground from `ColourBackdrop` (0x4879), black at 0, `c00` at 15. The words
      themselves, the second knight's `HE*.OB` banks painted in 9 to 11, and the
      trogg's three blocks by landscape code are all in `henge-bake`'s
      `battle_palette`, with the addresses; `henge_core::battle_palette` applies them in
      the original's order; `world.rs` composes the palette every frame and blits every
      fighter in its own pixels. `recolour.rs` and `blit_lut` are deleted.
      **Two things came out of reading it that were not the question.** The demon is
      fought over `WAB1.CMP`: its loader (0x8bfa) opens with `LoadWasteBack`, and
      `BlueDemon`'s twenty three words are the waste's browns with blues for its greens,
      so the Valley now fights on the waste rather than the swamp. And every backdrop in
      the release was saved with its family's ground table already at 16 to 28, which
      is why the ground writes were invisible and why the baker can check them.
      **Ours, and said so**: the browser's brawl of three or four knights. The original
      never fields more than two and the palette has room for two, so a third and
      fourth wear the second's colours. **Not touched**: `ColourEn4Knight` in `_TAVERN`
      (0xb4b2), which writes a four shade triple into the henge picture's entries 8 to
      11 for the knight at the stones, and `ColourStatus`, which the status panel
      already does

---

## If you only did three things

**Nothing is left but 55**, which is half done: the other half is the intro's tile maps
and its animated cast, and neither is built or faked.

**75, 76 and 77 were the last three, and all three were translation.** The plan
allowed for commissioning new music; it was not needed. `xTUNEn.BIN` looked like an
opaque driver blob because it is one, but three of them ship for every tune and the
Roland one speaks MIDI, so running it under the harness that already existed gave the
notes back. `COLCON`, `COLOURCYCLE`, `COLOURGLOW`, `DYNAMIC` and the two fades are one
screenful of assembler between them. `JOY0`, `AdjustJoy` and `BOUNCEBUTTON` are the
same. The only invention in the three items is a binding table, a dead zone that suits a
modern stick, and a synthesiser to play the recovered notes on.

**37, 33, 59 and most of 36 were the three things, and they were all translation.**
Every creature's controller is a named routine in `MOON` and every one of them reads;
`SETDEMONBORD` is nine stores at the end of `GFX`; the moors is the family this
project had already named after its own files. The lesson is the one 20, 64 and 69
taught: grep before you invent.

## What is not portable

Some of the original has no Rust equivalent worth writing: expanded memory paging, disk
swapping, the manual copy protection, DOS file handling. Accounted for in `COMPLETE.md` so
the symbol coverage is checkable, and deliberately absent here.

## What is design rather than translation

The ending screen and the final tally, which creature waits on which ground,
where everything but the two towns stands on the map, and which guardian each lair
holds. These were never recovered from the executable, so finishing them means designing
and playtesting, not translating. Worth knowing before anyone estimates the end of this
list.

**37, 64, 69 and the whole of phase 7 were on this list and are not any more.**
Item 37 came off it last: `ControlTrogg`, `ControlTroll`, `ControlRatmen`,
`ControlMudmen`, `ControlBalok`, `ControlBeast`, `ControlDemon`, `ControlDragon` and
`ControlClaw` are nine routines in `MOON` and none of them needed inventing. What the moon
gates is spelled out in `SetRatmenTables`, `CalcDamage` and `Henge`; the dice game's odds
are eleven records in `_TAVERN`; and the quest is `Valley`, `Henge`, `KnightWonGame` and
`WhoLived`, with its words in MOON's text pool. All three were taken for design because
the plan was written against the 334 `PUBLIC` names, before the other 1,889 symbols were
recovered. **Anything still marked design here is worth a grep before it is invented**,
and that has now been true three times running.
