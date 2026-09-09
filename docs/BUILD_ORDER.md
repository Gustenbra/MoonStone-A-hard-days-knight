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
- [x] 13. Arenas: 56 of them, scenery in file order, the header's own border list,
      depth sorting for the fighters
- [x] 14. Combat: positional hit lines, committed attacks, damage, death
- [x] 15. Bouts of up to four fighters, in the simulation, deterministic and serializable
- [x] 16. Overworld: travel, the day as a distance, terrain off `_MAP:MapType`. The
      ambushes this item once listed were ours and are gone: item 84
- [x] 17. Runs: wounds carry, the nights mend (`AdjustTIME`), death ends the run
- [x] 18. Four locations with menus, and a working healer
- [x] 19. Sound: 49 clips, played where the scripts say, silent without a device

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
      in a table by name and none of them faked; `TASKSOUND` emits the sound id the
      handler at 0x9b38 would hand `PLAY_SFX`, and the 23 routines in that table that play
      a sample are transcribed in `henge-core/src/sound.rs`. Twenty one tests,
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

**Every sound in the game is a script command or one of 23 small routines.** A scan of
the image for every `call`/`jmp` that resolves to `PLAY_SFX` (`0x5964`) finds 26 sites and
no more: the `TASKSOUND` handler at `0x9b38` (131 commands in the 239 named scripts, 54
distinct ids, 31 distinct samples), the 23 sound routines the scripts call through
`TASKGOSUB`, the dice cup at `0xb338` and the gadget click at `0xd50a`. So the cue layer
that inferred a swing from a state change and a footfall from a frame number is gone:
there is no footstep routine in the original, the knight's walk scripts carry no sound
command, and his swish is `TASKSOUND 0x0b` on the second frame of `Knight_SwSwing`. Three
of the 23 routines are a bare `ret` or a dead `jmp` and are translated as silent, which is
why two of the 49 samples are unreachable. `docs/TASKVM.md` has the disassembly.

**The controller tables are recovered after all.** They are `BSS`, which is why the
load image showed nothing, but `SetKnightAnims` and `SetMonsterAnims` in `MOON` fill
them with immediates: a walk table per creature (`*Wal`, right at +0, up at +0x10,
down at +0x20), the attack scripts by attack kind (`*Att`), the blow-taken script by
the attacker's attack kind (`*Hit`), and the damage per attack kind (`*Dam`). And
each `InitKnightvs*` calls a `Set*Tables` that writes the stat block into the actor
record: hit points at `+0x38` and `+0x3c`, the tracker's approach and back-off ranges
at `+0x52` and `+0x54`, its plane tolerance at `+0x56`, the kind at `+0x35`. So the
numbers below are the original's, not chosen. `docs/TASKVM.md` has the tables.

**And they are read at bake time rather than transcribed.** `henge_formats::tables`
runs `SetKnightAnims` (0x1771, falling into `SetUpKnight` at 0x1786), `SetMonsterAnims`
(0x186b), the ten `Set*Tables` routines (`SetKnightSwTables` 0x1f6a, `SetTroggAxeTables`
0x20f7, `SetTroggHammerTables` 0x2183, `SetTroggSpTables` 0x2220, `SetBeastTables`
0x22e2, `SetRatmenTables` 0x23ae with `RatNewMoon` 0x241b, `SetUpDragonTables` 0x2538,
`SetBalokTables` 0x25d5, `SetUpMudmenTables` 0x2665, `SetTrollTable` 0x26fd), the
demon's inline record (`InitKnightvsDemon` 0x2771 to 0x27af) and the claw's
(`InitKnightvsDragon` 0x249c to 0x24cb) through an interpreter of the instructions they
use, and the baker builds every `ActorDef` from what they wrote: the stance and the
recovery, the three walk rows as `walk`, `walk_up` and `walk_down`, the `*Att` and
`*Dam` tables as `attacks`, the `*Hit` table as `hurt_by` with `hurt` its swing entry and
`death` that script's `TASKDEAD`, the `*Blo` table as `blocks`, and the hit points and
the tracker's three ranges. The ratman's moon table is the same routine run under 0x2d
and 0x31. The hand-written `KNIGHT_SCRIPTS`, `KNIGHT_ATTACKS`, `KNIGHT_HURT` and
`KNIGHT_BLOCKS`, and the creatures' `idle`, `walk`, `hurt`, `death`, `hurt_by`,
`health`, `approach`, `back_off`, `depth` and `moon` fields, are deleted. What a creature
entry still says is what its controller decides in code: the attack it plays and the
kind it writes, the rows its branches name, the seats, the wave, and what its
`*Struck1` handler takes off the knight, which is not always its `*Dam` table:
`TroggStruck1` (0x42e7) and `RatmanStruck1` (0x4295, 0x42b1) read the table, but
`TroggSpearStruck1` (0x432e) subtracts three, `TrollStruck1` (0x438a) seven,
`BalokStruck1` (0x4285) five, `BeastStruck1` (0x4430) five, `DemonStruck1` (0x4366,
0x4372, 0x4378) ten, eight and ten, `DragonStruck1` twenty for the bite (0x43bd) and
thirty for the fire (0x43c2), `ClawStruck1` (0x43d3) ten; `TrollDam` and `BalokDam` are
never read. The troll used to carry three and Balok four, from those dead tables.

Each creature is an `ActorDef` like the knight's: the closure of the scripts its
states reach, its loader's bank tables starting on table 2 (the actor record's `+0x18`,
where the knight's holds table 1), an origin and hit box read off its own
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
      has no swing: every run frame carries a weapon part, and those parts threaten
      whatever state it is in, because `TaskCol_MainLoop` (0x9f26) has no state test
      of any kind. The toss and the impale are item 51
- [x] 35. **Balok.** `Balok_Stance`, `Jump` and `Jumping` for a walk, `UpperCut`,
      `UpperHit`, `Dead`. Thirty hit points, a blow of four (`BalokDam`), ranges
      80/60/10. The grab, the shake, the bite, the squeeze and the landing that
      bursts a knight are item 51
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
      `tools/taskvm.py --actor dragon_flight` draws it. **All of it is built now,
      and the earlier reading of the fight was not the code.** `ControlDragon`
      (0x3843) has no cooldown and no walk: the head's lift and lower are two
      jumps on `ADDJUMP` (0x38af and 0x3929: to x 100, the knight's row and -70
      over 13 frames, or -30 over 9), `DragonHeadMove` (0x3979) steps the arc
      and rocks the row five toward him each pass, and the only waits are the two
      `dragonbodge` words, two frames of the stance between breaths. `TrackKnight`
      (0x3be8) runs before every attack decision and five times inside each
      breath by `TASKGOSUB`, and its restore at 0x3c84 puts `+0x52` into `+0x54`,
      so the first high breath leaves the back-off at sixty for good. The low
      breath starts no fire task: its own weapon parts run 43 to 259 out.
      `DragonHit2` (0x3ad5) is the bite closing: the knight's task killed and
      record freed (0x96c9), `Dragon_BitKnight` put on in the bite's place, the
      chewing, `KillKnight` and `StopCombat`. `Dragon_Dead` calls `DrDropHead`
      (0x3bd2, the head's task thirty eight rows down), `StopCombat` and
      `DrDropClaws` (0x3be1, `DEAD_CLAWS` to `0xffff` and the claws' tasks
      killed on their next pass). What the knight takes is `DragonStruck1`
      (0x43ad), `DragonFire1` (0x43c2) and `ClawStruck1` (0x43d3), twenty,
      thirty and ten through `TalismanWrym`, onto the rows `InitKnightvsDragon`
      writes over his table, and the head's own blows put him on the head's row
      less one. **The flight is flown**: `henge_core::dragon` is `InitDragon`
      (0xa571), `ContinueDragon` (0xa5b3), `DragonWander` (0xa66b) through
      `DragonControlDone` (0xa707), `CheckEncounterDone+128` (0x816) and
      `DragonEncounter` (0xa3e2), and the routine at 0xcf6 with `_dragon_won`
      (0xd23) is the fight's aftermath. From the second moon the dragon takes the
      air at the start of every turn, after a knight `RND` names (up to four
      rolls over the living), sweeps the map two pixels a frame at his row
      (`DR_YADD` is one in the load image), and comes down the frame its nine by
      five shadow (`MI.C` frame 0x14, ten left of `DR_X`) covers the knight it
      is after, with neither the gem nor the hawk up. A knight who kills it
      grounds it for good (0xd38, 0xd50); one who does not leaves it flying.
      The desktop draws it over the map on `DrAnim[DR_WALK]` from the pack's
      table 5, and the trace line carries `dragon@x,z after seat`. **Not built**:
      the hoard, `WhoLived+57` (0xaf7) for a dragon and `DisplayDragon` on
      `StatTYPE` 0xa, and the knight picker page the Scroll of the Wyrm opens
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
      - **Trogg** (`ControlTrogg` 0x2ddf, `TroggStart` 0x2e03, `TroggMove` 0x2e22,
        `TroggAttack` 0x2e64, `TroggAttacks` 0x2ea7, `TroggSwing` 0x2eed, `TroggChop`
        0x2eff): translated block for block, with the listing beside the code. The
        overhead from a hundred to a hundred and twenty, the swing inside a hundred,
        a roll of `GETPERCENT` at 30 or under that chops through a held block
        (`cmp word [knight+0x28], 8` at 0x2ee6) rather than swinging into it, and
        `TroggAttack`'s finisher on a fallen knight. **The cooldown is ten frames of
        standing and the blow on the eleventh**: 0x2ea7 is `cmp byte [si+0x4a], 0;
        je attack; sub byte [si+0x4a], 1; jmp 0x2d52`, an unconditional jump to the
        stance after the decrement, where `DemonAttack` (0x502f) is `sub; jne` and
        frees the demon on the tenth. An earlier reading gave both the demon's
        shape. The two branches a blow raises are literal too: `TroggStruck+3`
        (0x2f1c) zeroes `+0x4a`, `TroggHit+8` (0x2f55) writes ten into it, and
        neither runs `FaceKnight`. `DeCapFLAG` (DS:0x7841) is a word of the bout's
        (`Bout::decap`): `InitCombat` (0x307) zeroes it, `TroggAttack+0x3b` (0x2e9f)
        sets it on the decision to go for the head, `SetDecapFLAG` (0x3e76) sets it
        from a script, and 0x2e8b and `BKnightAttack+0x18` (0x4c2b) read it
      - **Which way a creature faces, and the whole chain of it.** Nothing about it
        is inferred; every link is a read instruction:
        1. `FaceKnight` (0x3cf3): `mov ax, [me+2]; cmp ax, [knight+2]; jl` writes 1
           into `+8` when the creature is left of the knight and 3 otherwise
           (equal x faces left). `MonsterTrack+0x20` (0x56f9) calls it before it
           looks at a single range, so every tracking controller re-faces on every
           run; `ControlRatCollide+0x48` (0x3144) calls it directly; `TrackKnight`
           (0x3be8, the dragon's) calls `MonsterTrack` and then writes 1 at 0x3c1f
           whatever it said. `ControlBalok` (0x35fe, 0x3604), `BeastCharge`
           (0x2ff8, 0x3009), `MudmenAppear` (0x5480, 0x548b) and `FlipKnight`
           (0x3d13, from `RatmanHit`) are the other writers among the creatures
        2. Walking never turns anyone. `MoveL` (0x4dd5), `MoveR` (0x4e09), `MoveU`
           (0x4e39), `MoveD` (0x4e64) and `MonsterWalk` (0x4e8b) do not touch `+8`.
           `MoveBACK` (0x5783) reads it: facing right and walking left, or facing
           left and walking right, gives `bp = -1`, and `NextWalk` (0x4ef7) does
           `add byte [si+0xa], al` with it, so giving ground plays the walk cycle
           backwards with the creature still facing the knight. `monster::move_back`
        3. The controller's tail (0x2d52): `mov ax, [di+2]; mov bx, [di+4];
           mov cx, [di+6]; mov dh, [di+8]`, and the task loop `TASKHANDLE` (0x9702)
           stores them with the new script: `[task+2] = si; [task+4] = ax;
           [task+6] = bx; [task+8] = cx; [task+0x14] = dh` (0x9735..0x9741). The
           task's facing is refreshed every time a controller hands over a script,
           which for the trogg is every run, since `ControlTrogg+6` (0x2de5) writes
           the stance into `DS:0x783a` before anything else. `ADDTASK` (0x968a)
           copies `+8` the same way when the task is made
        4. `perdone` (0x99bd), the end of every frame `PerformCOMMAND` builds,
           writes the task back: x, y, z, and `mov al, [di+0x14]; mov [bx+8], al`
           at 0x99d2, so the record always reads as the task by the time a
           controller looks. `TASK_FLIP` (0x9a6d) writes `task+0x14` and copies it
           to `+8` at 0x9a85; only `Knight_SwDeath` and `Beast_TurnAround` use it
        5. The blit: `PerformCOMMAND+0x5e` (0x9859) tests bit 1 of `task+0x14`;
           `TASKRIGHT` (0x985f) places at `task_x + x`, `TASKLEFT` (0x9864) at
           `task_x - (x + cel_width)`. `taskvm::place`
        6. The knight: `ControlKnight` writes 1 at 0x3f77 when right is held and 3
           at 0x3f86 when left is, before `CheckBorder` and `SBORD` get to refuse
           the step, and nothing else in his controller writes it. `KnightSLAP`
           (0x44e5) sets it to `SLAP`, the slapper's facing, on a demon's or
           Balok's slap. `SetKnightCombat` (0x2962, the facing store at 0x297d)
           stands him at x 250, y 0, z 100, facing 3; the creatures come from the
           spawn tables `InitNewMO` (0x27ee) walks, eight bytes `[x][y][z][facing]`:
           `TroggTABLE` is (-50, 0, 100, 1), (360, 0, 150, 3), (340, 0, 50, 3),
           (-80, 0, 120, 1), always facing into the arena. **Built**, as
           `ActorDef::seats` and `ActorDef::seat`. Only x and facing reach the
           fight: `AddPlayer` (0x2989) falls into `AddKnight` and its
           `mov [di+6], ax` at 0x29b6 writes the rotating standing depth over the
           table's `z`, every time, so the `z` column of every one of these tables
           is dead and so is `SetKnightCombat`'s own `0x64`. `FaceKnight`
           overwrites the creature's facing on its first frame, which is the
           original doing that and not henge losing it
        7. `TASKWALKCOLLIDE` (0x9e06) reads the facing too: `xdir = facing & 3`,
           and a fighter is refused the step into another actor's body only in the
           direction he faces (0x9e58..0x9e74: facing right, only an actor to his
           right; facing left, only one to his left), with the body boxes
           `+0x22`/`+0x24` and `+0x4e`/`+0x50`. **Built**, as
           `arena::walk_collide`, called from `Fighter::walk` before
           `CheckBorder` and `SBORD` exactly as `ControlKnight+217` (0x3f9d),
           `MonsterWalk+19` (0x4e9e) and `MudmenMove+65` (0x53c0) call it, and
           `and`ed into the same direction byte. `Bout::separate`, which pushed
           overlapping fighters apart, and `ActorDef::girth`, which only
           `separate` read, are **deleted**. Its two halves have different
           depth tolerances (ten sideways, twenty up and down) and `jns` at
           0x9ec2 puts the equal case on the `up` bit; both are reproduced.
           `+0x22`/`+0x24` and `+0x4e`/`+0x50` are `FindWidth`'s (0x9dbc)
           running min and max over the drawn parts, carried into the record by
           `perdone` (0x99d8); this engine has no such accumulator and uses the
           authored body box, which is the stand-in `SBORD` already used
        8. A blow turns whoever takes it in exactly three places, and the whole
           table was read to say so. `KnightGotStruck` (0x4267) picks a
           `*Struck1` entry out of DS:0x7843 by the *striker's* kind
           (`mov si, [di+0xe]; mov al, [si+0x35]`), and of the entries only
           `DemonStruck1` (0x4360) through `DemonSlap` (0x437f,
           `mov al, [si+8]; xor al, 2; mov [di+8], al`, on kinds 0x10 and 2
           and no other) and `ClawStruck1+26` (0x43ed, `mov byte [di+8], 3`)
           write `+8`. `BalokStruck1`'s own copy of the demon's three
           instructions (0x427d) is **dead code**: the `jne` at 0x427b reads
           the flags `add bx, ax` left at 0x4272 and `0x7843 + kind` is never
           zero, so the jump over them is always taken and a balok's slap does
           not turn the knight. **Built**, as `monster::struck_facing` and
           `Bout::turn_struck`
        9. And one striker turns its victim from its own side of the blow:
           `RatmanHit` (0x34f0), the `+0xc` branch of `ControlRatmen`, compares
           `[si+8]` with `[di+8]` at 0x351c and calls `FlipKnight` (0x3d13)
           when they match, which is a knight clawed in the back being spun
           round. `FlipKnight` writes the *task* (`xor byte [si+0x14], 2`) and
           copies it into the record (0x3d31), so both turn together. The leap
           (`+0x48 & 1`) and the tail from a tree (`+0x48 & 8`) take their own
           branches first and never reach it, and a ratman that hits one of its
           own kind (`+0x35 == 0x12`) does nothing at all. **Built**, as
           `monster::ratman_flips` and `Bout::ratman_hit`
        What the video of the trogg swinging away from the knight showed was a
        rule of ours, since removed: turning a creature toward the step it was
        taking, which turned a trogg giving ground to the right to face right and
        chop at nothing. There is no such rule in the original; see 2. The chain
        was walked again against the image afterwards and 1 to 7 all still read
        as the listings do; 8 and 9 are what that pass added.
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
      **The repertoire under all this is built now** and has an item of its own,
      51 below: the ratman's leap, tree, head and gouge; Balok's hop, grab and
      landing; the beast's toss and impale; and the dragon's own two,
      `Dragon_BitKnight` and `DrDropHead`, with 36

## Phase 3: economy and character

Independent of phase 1. **Can start immediately, in parallel with the research.**

- [x] 38. Gold and prices. Coin off the fallen, a `bounty` per actor in the data,
      prices on the items, and both towns' merchants open. The healer inside the
      walls takes a donation and spends it down, which is `_WIZARD:HealDon`. A
      hermit in the woods who charged days was ours and is gone with the two potions
      he sold
- [x] 39. Inventory: carrying, using, losing. A bounded pack on the run, items as
      data with a price and a virtue, potions bought and drunk, and losing made real
      both ways: a potion is spent when drunk, and the druids keep what they are given.
      The cutpurse on the road that stood here was ours and is gone (item 84).
      **The goods are the original's
      ten and nothing else**: a Flask of healing and a Draught of life were invented
      here, and with them `Virtue::Heal`, which mended by a fixed amount; all three
      are gone, and the recovered `Potion of healing` (the routine at 0xcad0, which
      sets health to its maximum or gives a life point to a whole man) is the only
      thing there is to drink
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
      protection turns a challenge away or backfires. Two are still inert and honestly
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
      with no step count and no slow ground while either is up, and it is
      drawn as the token's own frame plus five for the gem and plus ten for the hawk,
      which in `MI.C` are the crystal row and the hawk row, one per knight's colour.
      The gem's flight comes back to where it began, the hawk's lands where you put it
- [x] 45. **Experience and levelling.** `XPlevels` is the cost of a point, indexed by
      the player count, and `AdjustLevel` spends it on one of the three abilities.
      The sheet carries the three `Increase` gadgets in the original's own order
      (`ab1`..`ab3`), lit only while the experience covers the cost and the ability
      is under five, as the status screen at 0xd3a7 lights them.

      **`XPlevels` itself is read now.** It is at DS:`0x4d4`, image `0x12884`, and
      the eight bytes are `03 00 02 00 01 00 01 00`: `Adjplayers` (0x13c2) clamps
      the player count to one to four, indexes the table with it and puts the
      answer in `[0x718]`, which `HGAbility` (0xcc23), `_MAP:KnightXP` (0xac7a) and
      `BKAddstuff` (0x4d9) all subtract. So a point costs three won bouts alone,
      two for a pair and one for three or four. The figure that stood here was
      four, and it was ours.

      **`BKwon` (0x49f) and `BKAddstuff` (0x4b0)** are the experience that happens
      *inside* a bout, and they are `Run::duel_won`: a point for putting the other
      knight down, spent where it is earned, on a flat roll (`and ax, 3` clamped to
      two, not `WIZABL`'s weighting), with ten hit points added on top for
      constitution and **no ceiling asked about at all** (the `inc byte
      [bx+si+0x2e]` at 0x4ca is the one increment in the game that does not go
      through `CheckMaxAbility`, so a knight who keeps winning duels goes past
      five). A duel pays through this and not through the road's own tally, which
      is what `World::is_duel` decides

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
      ours. The walk row is `ControlKnight`'s `A1$` to `A4$` (0x3fd6 to
      0x4012): up sets 0x10, down 0x20, left or right 0, in that order, so a
      knight walking straight up is drawn on `Knight_SwWalkU1..4` and straight
      down on `Knight_SwWalkD1..4`, and `MoveU`/`MoveD`/`MoveR`/`MoveL`
      (0x4e39, 0x4e64, 0x4e09, 0x4dd5) do the same for a creature; that is
      `ActorDef::walk_row`. The kind is the offset into `KnightAttSw`, and it is what indexes
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
      dagger gone. **What the item covers, looked at again and closed.**
      `SWORDFLAG` is a `PUBLIC` name with no address at all, between `DRAGON`
      and `PLAYERPOINTER` in the blob, and no routine in the image has been
      found that touches it. There is no sheathed animation set: all 38 of the
      knight's scripts are `Knight_Sw*`, so drawn against sheathed is not a
      state the shipped game can draw, and `Knight_SwWalkOn` is his walk-on and
      not the drawing of a sword. The weapon state the code does keep is
      `+0x40`, the sword's item id, which `CalcDamage` reads at 0x2d7d, 0x2d86
      and 0x2d8f for its 2, 3 and 5 and which `TakeSword` (0xccd4) writes,
      giving 0x19 to whoever lifts the magic sword and **0x16 to the other
      knight**, which is the only place in the game a sword is taken away. That
      is `Knight::weapon`, and it is built
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
      beast's `UpperHit` for a blow from above, each with its own death.
      **`Knight_Explode` and `TroggSpear_Toss` were on the not-built list and
      are built now**, with item 50: `TrollStruck1` (0x438a) falls into
      `TrollOHead` (0x4397) for the overhead chop alone and plays
      `Knight_Explode` when that blow left nothing, and `TroggHit+12` (0x2f59)
      has the spear, and only the spear, take a dead player knight's task away
      and play `TroggSpear_Toss` with the gore on. `DrDropHead` and
      `DrDropClaws`, the dragon's, are built with 36. **Not built**: the screen
      shake `ShakeADD` asks for

- [x] 50. **The computer knight. Recovered.** What was here was an invention:
      close the distance, swing, cool down twenty, one attack and no answer to
      anything. The original is `ControlBlackKnight` at image 0x4b79, which
      `InitGameStart+241` (0x1cfe) puts in `CONTROLTABLE` slot 8 while slot 6
      holds `ControlKnight`, the joystick one, and kind 8 is what
      `InitGameStart` writes into `+0x35` of all four knight records (0x1c69,
      0x1c88, 0x1cab, 0x1cca). So every knight the machine plays runs this and
      no knight a person plays ever does.

      The shape, block by block. `ControlBlackKnight` picks the other knight as
      `Opponent` (`cmp si, [KnightTable]`, 0x4b8c, then `[0x897b]` or
      `[KnightTable]`), copies `+0x28` into `ATT` without clearing it (0x4bb7,
      where `TroggStart` zeroes the same field at 0x2e08), and calls
      `MonsterTrack`. Off the plane it walks (`BKnightMove`, 0x4bd3, whose
      `and byte [si+0x48], 0x7f` at 0x4bdc gives the evade its one use back).
      In range it reaches `BKnightAttack` (0x4c13), which for a fallen opponent
      zeroes the cooldown, checks ninety and `DeCapFLAG`, and
      swings for the head: no gore test and no cooldown, unlike `TroggAttack`,
      and it does not raise the flag itself, the decapitation script's own
      `SetDecapFLAG` does.

      Against a standing opponent it reaches `BKBlock` (0x4c40) first. A
      `GETPERCENT` roll under `Progression[day & 7]` skips straight to the
      attack; otherwise, if the two are not facing the same way, a swing coming
      at it inside a hundred and twenty is met with kind 8, the block, and a
      chop or a lunge with kind 0xe, the evade (`_evadechop`, 0x4cad). That
      pairing is exactly what `KnightBloSw` pairs them with, which is a check
      on the reading.

      `BKAttack` (0x4cc3) rolls again, against `Progression[day]` unmasked this
      time, and gives ground on a roll at or under it, spending a second roll
      (`RND` at 0x4cd7) whose result is dead: it builds `bx = 0x5a + (rnd & 7)`
      and jumps to `BKnightMove`, which reads only the walk bits. Over it, the
      kind is by range with `ATT` vetoing a repeat: the swing inside ninety
      (`K0$`, 0x4ce5), the overhead chop inside ninety five (`KK1$`, 0x4cff),
      the lunge inside a hundred (`K2$`, 0x4d19, taken whatever `ATT` holds
      when the belt is empty), the thrown dagger past that if there are daggers
      (`K3$`, 0x4d39, on `+0x34`, which `SetKnightEquipment+38` fills with ten
      and `KnifeThrow+3` empties), and closing if there are none (`K4$`).

      The two branches the task loop raises are `BKnightStruck` (0x4d50), which
      runs `CheckBlock` and, when the blow is stopped, replays `Att[+0x28]` and
      clears bit 7 of `+0x48` so the evade is usable again at once, and
      `BKnightHit` (0x4da8), whose rule is not the player's: `KnightHit1`
      (0x4123) carries a swing through only for an up thrust or an evading
      victim, while `BKnightHit` carries it through for a chop, a lunge, or any
      blow on a kind 6 knight, unless the victim is evading.

      **`Progression` is the difficulty ramp, and the day count drives it.**
      Twenty bytes at DS:0x7c01, `14 0a 08 07 06 05 05 ...`, indexed by the word
      at DS:0x5b1, which `InitGameStart+60` (0x1c49) zeroes and `EncounterFini`
      (0x1167) steps every fourth encounter alongside `MoonCount`. Nothing else
      reads or writes it. So a computer knight hesitates one time in five on the
      first day and one in twenty from the sixth on, and past the table's twenty
      bytes `BKAttack`'s unmasked read falls into `demonbodge`, zero, and it
      stops hesitating.

      **A duel is two knights.** `PracticeCombat5` (0x00fe, 0x0104) writes the
      first two records into `[0x8979]` and `[0x897b]`, `MOON:Combat+115`
      (0x03c4) writes the challenger and the challenged into the same pair, and
      `ControlBlackKnight`'s opponent pick has room for those two and no third.
      The arena browser fielded three or four, the extras drawn from the second
      knight's palette entries because there are only two knights' worth; it
      fields two now.

      **Checked by looking**: two knights in the forest arena, the computer one
      throwing daggers at a hundred and ninety, closing when they run out,
      alternating the swing and the chop inside ninety, and dropping into the
      duck as the other one chops.

      **Two of the creatures' own finishers came with it**, because both are
      the `+0xc` and the `*Struck1` branches this item had to read anyway:
      `TrollOHead` (0x4397) and the spear's half of `TroggHit` (0x2f59). See
      item 49, where they were on the not-built list.

- [x] 51. **The weapon pile, and the strike point.** `COLLIDE.HIT` was decoded
      and then unused. It is the weapon pile's own shape: `TaskPlace$` (0x98c1)
      pushes every part flagged `WEAPON` onto `WeoponPile` and every `BODY` part
      onto `BodyPile`, ten bytes each, `TaskCol_MainLoop` (0x9f26) walks one
      against the other within ten rows of depth, and `COLCHK` (0x9fcd) walks
      the cel's polyline point by point: `CHECKL` (0xa0da) takes one, `NOWID1`
      (0xa0ed) adds the weapon record's x and 0xa108 its y, and a mirrored part
      is `neg ax; add ax, [WIDTH]` (0xa0e7) against the cel's own width. The
      baker now attaches those polylines cel by cel to the bank tables (recipe
      15) and a swing is swept along the blade rather than round the weapon
      cel's rectangle. Fourteen banks carry lines, `kn4.ob` and every creature's
      weapon bank among them; a bank the file says nothing about keeps the
      rectangle, so an actor the original never had still fights.

      **The strike point is `CXx` and `CY`** (0xa130, 0xa13f), which are the
      point of the sweep that landed and not the middle of an overlap, and
      `TaskCol_MainLoop` writes them into the struck actor's `+0x58` and `+0x5a`
      (0x9f87, 0x9f8d) beside `+0xc` and `+0xe`. That is what the blood comes
      out of.

      **What is still not the original**: the body side. `COLCHK` ends by
      testing the weapon point against the body cel's own pixel mask (`CBITLP`,
      0xa190, over the plane the cel's row stride at 0xa0b5 addresses), and this
      tests the swept line against the authored body box. Doing it properly
      means the packed sheets carrying a per-cel mask, which they do not.

- [x] 52. **What a creature can actually do to you: the repertoire under item
      37, and the jump engine three of them stand on.**

      **The jump engine** is `CalcJUMP` (0x2a8e), `ADDJUMP` (0x2b8d) and
      `ControlJump`/`CONJUMP` (0x2cde): six slots at DS:`0x76b2`, stride
      `0x14`, filled from a twenty byte template at DS:`0x77cc`. It is
      `henge_core::jump`, and it is not the task VM's `TASKJUMP`, which is a
      different thing that only `Beast_BackToss` uses. `CalcJUMP` aims at the
      opponent, offset by the caller's stand-off (Balok passes `0x50`, the
      ratman its own `+0x52`), and sizes the arc off the larger of the two
      gaps: half of it is the rise and an eighth is the frame count, floored at
      three and four, and the frame-count floor drags the rise to ten with it
      (0x2b48). `ADDJUMP` has three branches and they are not each other's
      mirror: jumping **up** starts with a speed and loses it to gravity,
      jumping **down** starts at rest and gains, and `NORM`, the flat hop that
      a rise of five or less takes, is the only one that divides unsigned and
      the only one that reads the template's `+0x12`. `ControlJump` steps the
      height by the speed as it was **before** that frame's gravity came off
      it, which is one frame of lead the whole arc keeps.

      **The ratman**, block for block: `RatmanLeap` (0x31a9), `RatmanInitLeap`
      (0x3215), `RatNormalLeap` (0x325c), `RatmanLeaps` (0x325f),
      `RatmanLeaping` (0x3270), `RatWithinTree` (0x32af), `RatmanInTree`
      (0x32b8), `RatLeapOutTree` (0x32e0), `RatHangKnight` (0x32ed),
      `RatmanOnHead` (0x3353), `RatmanGouge` (0x3383), `RatmanGouged` (0x3395),
      `RatmanReleaseKnight` (0x343d), `RatmanStruck` (0x3465),
      `KnightStruckRatInAir` (0x34b5), `RatmanHit` (0x34f0), `RatLeapHit`
      (0x353f) and `RatTailHit` (0x357d). The **tree is an actor**:
      `InitKnightvsRatmen+82` (0x236f) builds one out of `SetDecapFLAG+7`
      (0x3e7d), the general put-an-actor-here routine, on `Rat_TreeBrush`, at x
      `0xa0` with `y` = `HalfSCAPE - 0xc8` and `z` = `HalfSCAPE`, gives it kind
      `0x14` and `ControlMisc` (0x3ead), which does nothing at all, and keeps
      it in `TreeHANDLE` (DS:`0x69ae`). Only `RatmanLeap+30` (0x31c7) reads it,
      and only the first rat to want to leap takes it (`RatFLAGS & 4`); it
      stays up there thirty frames, hovering on `Ratman_HoverR` past sixty and
      `Ratman_HoverD` inside it, with its tail out as a weapon part. A rat that
      lands on the knight sits on his head for his endurance (`+0x30`) plus six
      (0x3561), gouges when that runs out, and throws itself a hundred and
      fifty pixels clear leaving five points off him. Three of the knight's
      eight attack kinds **kill** a rat outright whatever its hit points say: a
      block and an evade (0x3477, 0x347d) and an up thrust that catches one in
      the air (0x34b5). The kind gate at 0x3468, which makes a rat unhurt by
      anything that is not a joystick knight, is read and deliberately not
      reproduced.

      **Balok**: `BalokJump` (0x366f), `BalokJumping` (0x36d1), `BalokHit`
      (0x377a), `BalokGrabbed` (0x379b), `ControlBalokGrab` (0x37ae),
      `ControlBalokBite` (0x37c1), `ControlBalokCrush` (0x37dc) and
      `ControlBalokRelease` (0x37f0). Its walk is the arc, five frames between
      hops. `Knight_Explode` is built: a hop past halfway, under forty off the
      ground and within ten pixels of him is `KillKnight` and the knight bursts
      (0x3711). The grab takes hold of him, shakes him once, then lets him go
      if he lived and eats him if he did not, the bite and the squeeze
      alternating on the word at DS:`0x77a0`; neither takes a hit point itself,
      because both scripts call `KillKnight` through `TASKGOSUB` partway
      through.

      **The beast**: `BeastStruck1` (0x4430), the knight's `*Struck1` entry for
      a beast and the only blow in the game that picks its animation off which
      way the two are *facing*. Alive, he is tossed; dead with the gore on, he
      is impaled on the beast's own task. The toss is the knight's own
      animation in the original and draws out of the beast's bank tables,
      because `TASKCELBUF` chooses from a global `TaskCelTable` the loader
      filled and not from anything the actor carries; this engine looks a bank
      up on the actor's own definition, so the toss runs as a task of its own
      out of the beast's definition while the knight is on standby, and ends
      where the chain ends at `Knight_SwStance`.

      **Three globals came with them**, on the bout as `monster::Shared`:
      `ratman_flags` (DS:`0x779c`), `HitDelay` (`0x779e`) and `BalokFLAGS`
      (`0x7794`). `HitDelay` was a per-creature cooldown here and is one word
      for the whole fight, which is what `RatmanHit+35` says.

      **Two things this needed that were not there.** `DecTimer` (0x2a63) is
      the one `TASKGOSUB` target that touches nothing but the actor record and
      whose frame reads what it wrote two instructions later, so it is applied
      in the VM rather than handed out as an effect; without it `Balok_Stance`
      branched to `Balok_Blink` every frame and Balok's animation never ended,
      so its controller was never asked again. And the weapon pile is tested
      with **no state test**, because `TaskCol_MainLoop` has none: the six
      shipped scripts that carry a weapon part outside an attack are the
      beast's four run frames and its turn, Balok's hop, the ratman's four leap
      frames and its two tree frames, and every one of them is a creature that
      is meant to hurt you with it.

      The dragon's own two, `Dragon_BitKnight` and `DrDropHead`, are built with
      36: the bite that closes replaces itself with the chewing on the spot, as
      `TASKHANDLE` runs `DragonHit2` on the frame after the touch, and the dead
      head drops thirty eight rows.

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
      **And nothing else is on this screen.** A line saying whose turn it is, a name under
      each portrait, a stat line along the bottom and `Player N` written where a taken
      knight was all stood here, and `ChooseRefresh` draws none of them: it clears, walks
      `CRText`, blits the portraits whose bits are still set, blits the frame on the
      chosen one, and, while `TypeFLAG` is set, draws the name being typed at (50, 50)
      through the one-record entry at 0x7a70. All four are removed.
      **Typing your own name is built, and it is the rest of `ChooseFIRE`.** That routine
      at 0x16be does not finish a seat's turn: each arm writes the knight's buffer into
      `NAMEy` (0x530, 0x51a, 0x546, 0x55c) and calls `TypeName` at 0x13f0, and only then
      clears the knight's bit. So the knight being named is still free, still drawn and
      still framed while the typing runs. `TypeName` sets `TypeFLAG`, points `ScanREF` at
      `ChooseRefresh` so the whole screen redraws on every keystroke, puts 0x5c in
      `CURSOR`, and finds the caret by scanning the buffer for the first space, which is
      what the underscores are for. `ScanKEYS` takes fire or scancode 0x1c as done,
      scancode 0x0e as a backspace, and `ASCIIKEY` for everything else;
      `cmp word ptr [SPACE], 0xd` is a **thirteen character** field and `NameDone` writes
      a NUL at the caret, so backspacing the default away really does leave a knight with
      no name. `ASCIIT` at 0x14f2 is the scancode table, uppercase throughout, and its
      entry for the space bar is 0x5f, the underscore.
      **Ours on this screen**, and marked so: the line saying whose turn it is, the name
      under each portrait, the stat line along the bottom, and the `Player N` that stands
      in an empty slot. The original draws none of them; it draws the chosen knight's name
      at (50, 50) once he is taken, out of `BNAME`, `GNAME`, `ENAME` or `RNAME`
- [x] 52. **A real status panel.** The knight record and `DisplayKnight` give the whole
      sheet: strength, constitution and endurance at `+0x2e`..`+0x30`, life points, gold,
      daggers, experience, health and its maximum, the weapon and the armour, with the
      arithmetic that turns them into a fight. It is a screen of its own and the only place
      the original draws any of it, so it is the only place henge draws any of it either:
      the plate-per-fighter strip along the bottom of an arena that used to be here was
      ours and has been removed, and the sheet is on a key
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
      **The payload is recovered too, and is no longer ours.** `STRP`'s low nibble is the
      operation (`HotGadget`: 5 cast, 1 take magic, 3 raise, 0xa buy, the rest through
      `test ax, 0x20` to `TakeGold`) and its high nibble is the bit within a field that
      holds several things; `STPL` is the field. `henge_core::status` carries both, along
      with `SetUpStatus`' seven string arrays and the `StatTYPE` each side of the panel
      reads one of. `_WIZARD:DonateLoop` numbers its own four gadgets separately (2 leave,
      3 take, 4 less, 5 more) and is a second enum for that reason. A real mouse moves the
      pointer as well, because a window with a mouse in it should behave like one.
      **`SHOWPOINTER` is the routine at image 0xcf31, and it is one plain cel blit**:
      `les si, [0x892f]; sub ax, ax; mov bx, [0xe492]; mov cx, [0xe494]; call 0x5d7f`,
      which is the same blit every other sprite in the game goes through. `PO.CEL` is
      drawn artwork and is drawn in its own pixels; a flat silhouette with an
      eight-direction dark halo under it stood here and is gone, and `sprite::draw_mask`,
      which flattened it, now has no callers at all.
      **And the pointer belongs to six screens, not to every menu.** Scanning every call
      to 0xcf31 gives `MOON:WDLOOP+66` and `MOON:HWLOOP+66`, the two town menus,
      `_TAVERN:TavernLoop+57`, `_WIZARD:DonateLoop+52`, `_STATUS:StatLOOP+19` and
      `_STATUS:FiDisplay+10`; `MovePointer` is called from `StatLOOP` alone. **The title
      and the select screens are not among them and have no gadgets either**: `DoOptions`
      at 0x1241 and `ChooseLoop` at 0x15a0 poll the stick themselves, with no
      `CLEARGADGETS`, no `ADDGADGET` and no `CHECKGADGET` in either. Both had a box per
      row here so a mouse could drive them, and both are gone
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
      now fits the screen it was written for. **The ramp is recovered too**: the six
      words at 0x8f3b to 0x8f54 go to `DS:0x80bb + 2` onwards, which is entries 1 to 6 of
      the loaded picture's palette, and the fade in at 0x8f5c is from that palette; so an
      instruction is the same box with `MESSAGE.PIV`'s four purples and the outline
      entry gone red, for one message. **And so are the chains the stale span hid**:
      `HengeInstruct` (0x129f9, five records ending on the shared `Press fire to
      continue` at DS:0x5dd), `SCR_PRO` (0x12965, the knight's name copied into `promes0`
      first), `VICTORY` (0x12a21) and `NextDayMes` (0x1b19a, two records). `SHMES1`..`8`
      are the strings, not records. **What takes a box down is the caller's**: seven
      callers follow the routine with `WaitFIRE` at 0x8251 and then a fade out, and
      every other one loads over it; `henge_core::message::Until` carries which, no
      timer clears the first kind, and fire does nothing to the second. **Ours:** how
      long a box that covers a disk read holds (`LOAD_TICKS`), since nothing here reads
      one
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
      deliberately not in the intro. **The ending half is built too**: the ten scenes at
      0x00fa in `henge_core::ending`, `CO.STI` baked as a second 320x1200 panorama beside
      `INTRO.STI`, the ending's own fifteen cast scripts read out of the same image with
      the ending's bank table, the rise off `bg7` on the eight decelerating frames
      `0x3ddb` gosubs, and the two recolourings the exit byte drives (0x3b9d on the
      knight nibble, 0x3b23 on the moon's). `--start ending` puts it up headlessly.
      **Ours:** how long the logo and each credit screen
      holds, since in the original that is a floppy's seek time; the rounding of 9.1033
      frames a second onto the engine's 70.0863 ticks, which is 7.70 and so eight; and the
      dark ring round a caption, standing in for the glyph shading silhouette text throws
      away
- [x] 56. **Save and load: deleted from the game, kept as a test harness.** The original
      has none: `MOON.CFG` is a sound-card profile and there is no slot, no file and no
      routine anywhere in the 2,223 symbols, the only `*Save*` hit being `SaveTYPE`, a task
      VM opcode. So **F5 and F9 are gone**, there is no menu item, and nothing a player can
      reach writes or reads one. The serialisation survives, explicitly labelled, as the
      headless harness's way of posing a run: `--save <path>`, `--load`, and `S` and `L` in
      an `--input` script, all command line and nothing else. It is also the determinism
      check, which is the part that was always worth having: a snapshot is `Run` plus
      `Overworld` plus the title's settings plus `WaitCOUNT` with a magic string, a format
      number and a fingerprint over the lot, and the test round trips it through text and
      then runs five hundred more steps on both copies to see that they stay identical.
      Three refusals told apart on purpose, **no path of any kind in the file**, and a test
      asserting that. The module is `henge_core::harness`, named so nobody mistakes it for
      a game feature; reading and writing the file is `henge-desktop`'s, because core does
      no I/O and keeps its one dependency

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
of everything on the map are recovered.** `MOON:MapIconsTABLE`,
`ForestLairs`, `LairLocation` and `LairType` were all inside the 2,906 bytes of DGROUP the
load image used to carry as a stale duplicate. That span is readable now
(`docs/REVERSING.md`), and `henge-bake` reads all four out of it: the two towns,
Stonehenge, the Valley of the Gods, Math's tower, and the twenty four lairs with their
guardians and head counts. What that replaced, and how far out it was, is in
`docs/COMPLETE.md` 4.1 and 4.3, and so are the four home villages, which are frames 0x15
to 0x18 of the same table. **Nothing on the map is sited by hand any more**: a hermit in
the southern woods was the last one and the original has no such place, so he is gone.

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
      charged it to the day. What the table holds is in item 84: the forest is mask 1
      (half), the wastes 0, 2 and 3 (open, half in bursts, a quarter), the swamp mostly
      open with patches of all three, and the map's own border row and columns 3 and 2.
      The only hard limit is a rectangle: `_MAP:HawkBorders` clamps the token to
      `0..=310` by `0..=190`, and a step into it is charged too.
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
      placements. `AddKnight` (`0x298c`) stands each arrival a quarter, a half or three
      quarters of the way from the deepest rectangle down to row 200, through
      `FindQuarterBORD` (`0x29e0`), `FindHalfBORD` (`0x29cb`) and `Find3QuarterBORD`
      (`0x29f7`), which is where the fighters start, and the row it measures from is
      DS:`0x80b5`, which the layout loader at `0x8d93` fills with the deepest `bottom` in
      the header, floored at 30.
      And **only the knight is bordered in the original**: `SBORD` has one caller and
      `MonsterWalk` is not it. Running the creatures through the same gate is ours.
      **Now right.** Row 200 is the foot of the screen and the original fights over the
      whole of it. This engine used to draw its own status strip over the bottom thirty two
      rows, which is why only two of the three standing places were usable and why a
      fighter who walked down to row 199 disappeared. The strip is gone: the original has
      no in-fight readout at all (`Combat`, `0x351`, is ten calls and none of them draws
      one; see `COMPLETE.md` 8.3), so all three places are in use and the arena is the
      whole 320x200 screen. `AddCNT` at DS:`0xa68` is how the three are handed out: the
      counter runs 1, 2, 3 and never rests on 0, so the places come round as three
      quarters, one quarter, one half, and nothing resets it between bouts
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
      ten to thirty one; magic is his bestowal called twice. `LairWon` (0x05ac) pays a point of
      experience the first time only and then **falls into `LairGEM` (0x05c3), which is
      `mov ax, 2` and the status panel**: the lair's own page, `StatTYPE` 2, is where the
      floor is handed over and there is nowhere else. `CheckLairClear` writes 0xffff over
      a lair that is beaten *and* stripped, so one you could not carry out of is still on
      the map to come back to. **`MOON:LairFile` is recovered**, which is more than the
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
      (119, 12). **The eight byte `Moons` table is read now**: DS:0x5a9, image
      0x12959, `2d 2f 2e 30 31 30 2e 2f`, indexed by `MoonCount & 7` at
      `EncounterFini+0x49` (0x118d), and looked at in the select screen's palette cel
      0x2e is the gibbous and 0x2f the half, so the original's own cycle runs full,
      half, gibbous, crescent, sliver and back, the two middle pictures out of order.
      `moon::MOONS` is that table and the monotone cycle that stood in for it is gone.
      **Ours** was putting the hint under it. The fourteen hints are
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
      one to five gold and an exit. **The menu that went over that panel is gone**; the
      six gadgets themselves are built, see item 85
- [x] 66. **Temple and mystic.** The temple is `_STATUS:SellToTemple` and `GoldSell`:
      one off the record, the price shifted right once into a purse that saturates at a
      hundred and fifty, and a sword sold out of the hand leaves a long sword in it. Its
      gadget list is `se7`..`se17`. The mystic is `MYS.PIV` and the routine at 0xb935:
      a donation, an ability picked before the roll, and `MysticUpDown` walking
      `DonationTAB` for the delta the donation buys, good at fifty or under. Its lines
      are `MY1a`..`MY7b` verbatim. **The coin-at-a-time donation gadget is built**, which
      is `_WIZARD:InitDonation` (0xbb34), `DonateLoop` (0xbbe6) and `DonationRefresh`
      (0xbc9e): four gadgets whose `[si+0x10]` is 4, 5, 3 and 2, at (0x90, 0xa9),
      (0xa2, 0xa9), (0x83, 0xb9) and (0xad, 0xb9), moving one coin at a time between
      `GOLDP` and `DONATION`, with the two purses, the two words and the two numbers where
      `DonationRefresh` puts them. The three fixed amounts that stood in for it are gone.
      **Two corrections, item 85:** the "temple" above was the Waterdeep mystic's list;
      the high temple is the status panel on type 6, with `TTemple` at 0xce11 dividing
      buying from selling at the pointer's x, and it buys and sells the moonstones and
      the keys too (`BuyMoonstone` 0xce5a, `SellMoonstone` 0xce60, against its own stock
      at DS:0xed96, not against another knight). And the healer and the mystic have no
      menu in front of the bowl: the greeting, `WaitFIRE`, the bowl, the verdict
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
      `BETLOSER`, `PLAYERPOT` and `CONT` are written. **The shake is built too**:
      `DD_ShakeDice` (`DS:0xce59`) and `DD_ThrowDice` (`DS:0xce6d`) run on `dice.cel` in
      `DiceHANDLE`, `ShakeDice` (0xb18a) adds the task with `dl` 0x22 and the handler at
      0xb1a1 is the whole state machine, `TavernLoop` (0xb137) runs it, and `DiceRND`
      (0xb21d) does not roll or draw until `DiceTHROW` reaches 2, so the faces arrive
      when the hand lets go. `henge_core::dice`. **Still not right**: the five stake
      gadgets belong on `dice.piv`, where `load_DiceBACK` adds them, and this pack keeps
      them on the tavern screen's menu

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
      `_STATUS:StatCheckKeys` (0xc44e): `KI.CEL` cels 5 to 8 at x 0x4c, 0x5e, 0x70 and
      0x82, eighteen apart, one slot per bit, an empty slot for a key you have not found.
      **The row is kept and there is nothing in its way.** This entry used to say the row,
      y 0x6f, was the one thing not kept because the armour sat there; it does not.
      `DisplayKnight` puts the armour cel at (0x1d, **0x70**) and the keys start at x 0x4c,
      so the two neither share a row nor overlap. The moonstones share the key row, at
      x 0x67 for all four bits, which does overlap the second and third key slots and is
      the original's own arithmetic
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
      handing out a moonstone is what let it fire. `KnightWonGame` at 0x10cf pushes the
      exit byte, hands `VICTORY` to `OCCURMESSAGE` at 0x8eeb, waits one vertical blank,
      calls `WaitFIRE` at 0x8251, and then **quits to DOS** with that byte in `al`: the
      low nibble is which moon (0x2e -> 2, 0x2d -> 4, 0x31 -> 3, else 1) and the high
      nibble which knight (3 -> 0x10, 0 -> 0x20, 1 -> 0x30, 2 -> 0x40). `INTR.EXE` reads
      it (item 55). **Proved, not assumed**: `INTR.EXE`'s entry does
      `mov ax, es:[0x82]; sub ax, 0x3131`, checks both digits are `'1'` to `'4'`, and jumps
      to its ending half when there is a tail; the routine at its 0x3b9d branches on the
      second digit and patches four twelve bit words into a plate's palette, red for 1,
      blue for 2, gold for 3, green for 4. `KnightWonGame`'s high nibble is 3 -> 1,
      0 -> 2, 1 -> 3, 2 -> 4, and seats 3, 0, 1 and 2 are the red, blue, gold and emerald
      knights: four for four. `Tally::code` is the byte, **and it now has somewhere to
      go**: a win puts `VICTORY` up, waits for fire the way `WaitFIRE` does, and hands the
      byte to `Ending::new`, which is `INTR.EXE` run with it as a command tail (item 55).
      `OCCURMESSAGE` calls 0x8e90 first, which is the `rep movsb` that puts `MESSAGE.PIV`
      back, so the victory message itself is the same stone circle every other message
      goes over. `bg8.piv` sits in MOON's text pool between the victory lines and the next
      message and **nothing in the image refers to its address**; it is one of the three
      plates `INTR.EXE`'s ending half uses, not MOON's. An ending screen over `bg8` used
      to stand here and is gone: the real one is the ending sequence's own last scene
- [x] 73. **Scoring and the final tally. There is none, and now there is none here
      either.** `GAMEOVER`, `GAMETABLE`, `TOTALS`, `PPOINT`, `PINDEX`, `FMEM_POINTS` and
      `FMEM_COLAREA` are PUBLIC names carrying no addresses, so there was never a page to
      port. A seven-line tally over `bg8.piv` stood here: the day, wins out of fights,
      lairs cleared, gold and experience, life points left, keys and the moonstone. All
      of it was ours and all of it is removed. `Tally` is now the ending and the seat,
      which is what `KnightWonGame` needs to make its exit byte
- [x] 74. **Losing properly.** **Recovered: `MOON:WhoLived`**, which every fight ends on
      because `MOON:Combat`'s loop closes with `call WhoLived`. A knight on nothing is
      not finished: his health goes back to its maximum and `sub byte ptr [si+0x31], 1`
      takes a life point. The run ends when the last one goes, which is what
      `_MAP:CheckEncounterDone` tests when it decides a knight on the map is a grave
      (`cmp byte ptr [si+0x31], 0; jg`), and the routine at image 0x617 answers with
      `mov si, GameOverMes; call 0x8f17`, which is `INSTRUCTMESSAGE` and so brings the red
      ramp with it, then `WaitFIRE`, then `jmp StartAgain`, which is the title screen. So
      five deaths to a run rather than one, and the game goes back to the title rather
      than restarting where it stood. A run that ends indoors walks back out to the map
      first, because that is where the message goes up.
      **`GameOverMes` is `GOmes1` at y 95 and `promes4`, `Press fire to continue`, at
      y 180**, and that is the whole chain. `Player      ` sits nine bytes before `GOmes1`
      in the data, nothing in the image refers to its address, and it was once read as the
      chain's first line. It is dead data.
      0x617 is reached from `_MAP:ScrollINPUT`, where scancode 0x10 quits, and from
      `_MAP:NextWHICH`, where the last knight with no life points left falls through to it

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
      **Every screen change fades in**, and the two screens that are dismissed rather
      than walked away from fade out: the between-days screen, which is `FADEOUTDAY` and
      which `NextWHICH` reaches through `WaitFIRE`, and a message chain,
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
      Those ten keys are now what the table **ships** with; the arrows-and-space layout
      that used to be the default was ours and is gone, and so is `--original-keys`, which
      had nothing left to mean.
      **The four thresholds are recovered too.** `JOY0` compares against four words at
      `DS:0x817e` to `0x8184`, and the image has them initialised at `0x1a52e`: **16, 16,
      80, 80**. That is the answer before anybody calibrates, so it is
      `Calibration::default`, replacing a pair of invented corners at six tenths of
      deflection. The four are exactly `AdjustJoy` of 6 and 90 (range 84, an eighth is 10),
      which checks the reading and also fixes the scale a modern axis is put on: -1 maps to
      6 and +1 to 90, so calibrating a pad that reports clean extremes gives the shipped
      pair straight back. The one inherent piece is that a gameport count is a busy-loop
      count and a pad reports a fraction, so something has to map one onto the other.
      **The menu route in is recovered.** `OptionKeys` at `0x1282` tests scancode 0x24,
      `J`, first of all and jumps to `Fix_JoyStick`, which jumps back at `0x59d7`. So `J`
      on the title screen calibrates and F11 is gone. Escape is the same story: tested at
      `OptionKeys+11` (`0x128d`) and again at `StartAgain+18` (`0x00ba`), both returning, so
      it quits from the title screen and nowhere else.
      All of that is `crates/henge-desktop/src/input.rs` and unit tested.
      **Ours.** The binding table itself, because the original's keys are ten `mov ax,
      <scancode>` instructions and there is nothing to port. It is data: actions to
      sources, by winit's and gilrs' own names, saved to `henge-controls.json`.
      `--bind 0:fire=Space` rebinds from the command line, `--controls-write` writes the
      file, and the calibration is saved alongside. A key the table claims is a control is
      never also a developer key, which is why the arena flip moved off Tab onto `F2`.
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
- [x] 79. **The map is the picture, and a place is asked for rather than walked into.
      Recovered, and it replaced an invention on both counts.** `MAP.CMP` is one 320x200
      image and four things go on top of it: `_MAP:DisplayLairs` (0xa26c) blits `MI.C`
      frame 0x14 at every live lair, `_MAP:DisplayOtherKnights` (0xa22c) the rival
      tokens, `MOON:CheckLairEncounter` (0x88f) frame 0x1f over the lair the token
      overlaps, and `_MAP:SHOW` (0xa1f0) the traveller. The frame body at 0xa2d7 is
      exactly that list, in that order. A status bar across the bottom, a purse plate in
      the corner, the cutpurse's notice (and since, item 84, the cutpurse) and one-colour
      silhouettes of `MI.C` 0x15 and up are all gone; those frames are one-pixel outlines in index 31 that the original blits
      nowhere and keeps only for `MOON:GetWIDTH` to measure a box out of.
      **Entering.** `_MAP:FOLLOW` calls the walker at 0x6b5 once a frame, which clears
      the five eight-byte slots at `DS:043c` and pushes `[x][y][kind]` for everything the
      token overlaps; nothing reads the stack until `_MAP:ScrollINPUT` sees fire
      (`test ax, 0x10`, 0xa3c9) and calls `_MAP:DisplayStack` (0xae27). None goes back to
      the map, one falls into `StackDecision` and enters it, and more draws
      `_MAP:CreatePaper`: `MI.C` cel 0x20, a 174x51 plank, at `PaperX, PaperY` = (50,
      100), the knight's own token at `+5, +5`, his name and `_MAP:knightopt` at `+15,
      +5`, then one step of fifteen and a numbered line every six pixels, the digits from
      `KEYNUM` = `'1'`. Answered with the top row's `1` to `9`, and the loop's only exit
      is a number naming a live slot. `_MAP:OrderOpt` composes each line from `knlair`,
      `knkn` or `StackMessages[kind - 0x15]`, and those lines are baked beside the places
      they belong to. **Checked by looking**: standing where Highwood's box and a glade
      lair's overlap raises a two-line paper, `1` opens Highwood and `2` the lair; a
      number past the end leaves the paper up; one thing underfoot opens with no paper at
      all; and the lair you stand on gains its glowing ring
- [x] 80. **The between-days screen waits, and `FADEOUTDAY` is built.**
      `_MAP:NextWHICH` at 0xa454 calls the screen (0x8e5b), then `WaitFIRE` (0x8251),
      then the fade out (0x5b65). It used to dismiss itself after 150 ticks. The screen
      itself is `CH.PIV`, the `NextDayMes` chain and one cel of `KI.CEL` at (119, 12), and
      nothing else: a day number and one of the fourteen `WaitMES` hints are gone, since
      the fourteen belong to `WAITMESSAGE`, a different screen shown while a disk loads.
      `NextDayMes` (0x1b19a) is **two** records, `Next Day` at y 95 and the shared `Press
      fire to continue` at y 182, and the second is drawn now. `WaitFIRE` reads fire
      (`test bx, 0x10`) and nothing else, so the screen no longer goes on any key; and
      `WaitCOUNT` is stepped only at 0x8ecf inside `WAITMESSAGE`, so turning a day over
      no longer steps it
- [x] 81. **Waves, and a lair is a lair now.** The number in a lair record ran from three
      to fourteen and only as many as a bout seated ever arrived, so a lair of fourteen
      ratmen was three ratmen. All of the machinery is named and all of it reads, and
      `henge_core::wave` is a transcription of it: `TotalMonsters` (DS:0x96a),
      `MaxMonsters` (0x96c), `NumberInCombat` (0x96e), `SIDE` (0x970), `INITMO` (0x972),
      `INITANIM` (0x974), `AdjustLevel` (0x2824) with `lev_adjust` (DS:0xa0a) and `KLTAB`
      (DS:0xa4c), `SetMonsterCombat` (0x27e4), `InitNewMO` (0x27ee) and `CountTheDead`
      (0x213). The full account with the disassembly is in `docs/COMPLETE.md` 3.2. In
      short: **one creature at a time** for everything but the ratmen, who come two at a
      time (`InitKnightvsRatmen`, 0x2337, is the only `MaxMonsters` of 2 in the game); the
      next one walks in on the frame the last one's death script reaches its
      `TASKGOSUB CountTheDead`, from the side `SIDE` names and at the depth `AddCNT` hands
      out; and what ends the fight is the player going down or nothing owed with nothing
      standing, which is `CountTheDead`'s own two tests and not a head count.

      Three corrections came out of reading it. **`TotalMonsters` is not the head count**,
      it is what is still owed, so a fight shows `MaxMonsters + TotalMonsters - 1`
      creatures. **A trogg fight is not one creature either**: `InitKnightvsTroggAxe`
      writes 3, so three troggs come one after another, and four people playing is worth
      one more (`cmp [0x91e], 4`). And **`AdjustLevel` scales the lair to the knight**: the
      creature's row of `lev_adjust` is subtracted from the count, so a knight fresh off
      the select screen raids a lair of fourteen troggs and meets nine, while a knight who
      swings for twenty with ninety experience meets more than the table says.

      Checked by fighting one: a headless run through `lair.forest.4`, fourteen ratmen at
      `MaxMonsters` 2 and `TotalMonsters` 9 after the row, fields ten of them two at a time
      and ends when the tenth falls, and the screenshots show a live ratman walking in from
      the left over the corpses of the ones before it. The `--trace` line carries the three
      counts now, so a wave is checkable as behaviour rather than by squinting.
- [x] 82. **The four home villages, and one invention out of the map.** `MapIconsTABLE`
      frames 0x15 to 0x18 at (18, 11), (286, 11), (0, 187) and (303, 192), each gated on
      `[di+0x20]` by `MOON:CheckGROOC` (0x732) so a village belongs to one knight and the
      other three cannot see it; `ForestVillage` (0x112a), which `MooresVillage` and
      `WasteVillage` are two more names for and which `TakingMoon` sends all four frames
      to, gives one life point and will not take a knight past three. It loads no backdrop,
      so a village is a line on the paper and not a screen. Removed with it: **the
      hermit**, a second healer this project invented and stood in the southern woods, and
      the Flask of healing and Draught of life he and the merchants sold. Nothing on the
      map is sited by hand any more, and the baker has a test that says so
- [x] 83. **The clock. Recovered: the frame is one vertical retrace, so the tick is
      70.0863 Hz and not sixty.** The wait is the unnamed public routine at image `0x5a24`,
      sitting in the gap between `AdjustJoy` (`0x59f8`) and the start of `GFX` (`0x5a6e`):
      `mov dx, 0x3da`, spin while bit 3 is set, then spin until it is set again, which is
      exactly one retrace. Eight places call it and four of them are main loops, once a
      pass: `Combat` at `0x0354`, `MapLOOP` at `0x0a306`, `ScanKEYS` at `0x0145a` and
      `FindLandscape` at `0x0afed`; the other four are `ShakeScreen` (`0x0496b`),
      `KnightWonGame` (`0x01117`), `FightDemon` (`0x01031`) and the fade-out loop
      (`0x05bb0`). Nothing else paces a loop.
      So the rate is the video mode's. **The image never reprograms the timing**: there is
      no write to the Miscellaneous Output register at `0x3c2` anywhere in it, and the only
      CRTC writes are index 0x0c, the start address, at `0x5a34` and in `ShakeScreen` at
      `0x4965`. A 320x200 VGA mode therefore runs at the BIOS 400-line timing, 25.175 MHz
      over 800 dots over 449 lines, which is **70.0863 frames a second**, the same figure
      `henge_core::intro` already quoted for the story card's 420 retraces. The engine's
      tick is now 14,268,123 ns.
- [x] 84. **The day is a distance, and nothing on the road is rolled.** The map loop
      from `PlayerKnight` (0xa355) to `DistanceDONE` (0xa4b2) calls nothing that rolls.
      `MapMovement` (0xa35d) does `inc word [0xcc98]` on every frame a direction is held,
      before it reads `SlowFLAG` and before `FOLLOW` (0xa29f) calls `HawkBorders`, so a
      refused step and a step into the edge both count; `GoTheDistance` (0xa422) compares
      `[0xcc98]` with `[0xccac]` and goes to `NextWHICH` (0xa434) when it is reached, before
      the move, so the step that spends the distance is not walked. `[0xccac]` is
      `DistanceDONE+12` (0xa4be): `mov al, [di+0x3e]; shl ax, 1` four times, once more for
      haste, so the opening knight's six is ninety six frames. `NextWHICH` clears the three
      effect flags (0xa962), zeroes `[0xcc98]`, steps `WHICH` and masks it with 3, and when
      it wraps calls the routine at 0x1148, which has no name in the table: `[0x898b]` up
      by one, and when it passes three `[0x5b1]` up by one (the computer knight's nerve),
      `MoonCount` up by one and masked with 7, `GiveBK`, `[0x8989] = Moons[MoonCount]`, then
      `AdjustTIME` (0x119f) for every knight. Then the between-days screen at 0x8e5b,
      `WaitFIRE` and `FADEOUTDAY`. Every encounter ends the turn: `Combat+60` (0x38d) and
      `EncounterAllDone` (0x113e) both do `mov ax, [0xccac]; mov [0xcc98], ax`, and a
      village, a town, the wizard, the circle, the Valley and the dragon all come back
      through the second. **There is no ambush.** Every fight the map starts is something
      the token stands on and fire is pressed over (`ScrollINPUT`, `test ax, 0x10` at
      0xa3c9, into `DisplayStack` and `StackDecision`, 0xae9f), or a rival who walks into
      you (`BKCollision`, 0xaab1), or the dragon (`DragonEncounter`, 0xa3e2); `CheckGROOC`
      (0x653) and the walk at 0x6b5 are the overlap test, not a roll. Removed with this:
      the one in ninety roll per step, the two hundred and twenty step day, the per-step
      healing (`AdjustTIME` is the only mending outside a healer and a potion), the
      cutpurse on the road, the `--peaceful` switch that turned the first two off, and
      the baker's `AMBUSHES` table of which creature each ground produced, which was
      design. `MapSLOW` read out of DS:`0xc42a`: 308 cells of mask 1, 165 of 2, 145 of 3,
      382 open; all but eight forest cells are 1; the wastes are 0, 2 and 3; the swamp is
      89 open, 57 of 1, 45 of 2, 29 of 3; the top row is 3 and the two side columns 2.
      **Not built, and recovered:** the three computer knights. `InitGameStart` (0x1c0d)
      fills all four records at DS:0x6c9e with `Enemy1Name` to `Enemy4Name`, kind 8
      (`ControlBlackKnight`), `[+0x20] = 4` and the corners (15, 100), (300, 100), (160,
      20), (160, 180); `ChooseKnight` turns the first `NUM_PLAYERS` of them into people and
      `InitKnights` (0x157) moves those to their villages. `DisplayOtherKnights` (0xa22c)
      then draws the other three every frame: frame `[si+0x20]` (4 is the purple token),
      0x21 for a grave when `[si+0x31]` is gone, `+0x2b` for a toad. They take turns
      (`MapLOOP+19` to `TrackLair`), fight lairs, heal, level and challenge you. Drawing
      them where `InitGameStart` left them would be true for one day and false after, so
      nothing is drawn until their turns are built
      **This mattered because every recovered duration in the tree is a frame count**, so
      sixty ran all of them about fourteen percent slow: every cooldown, every script
      frame, every sixteen-step fade. Two numbers moved with the clock rather than against
      it: `intro::MESSAGE_TICKS` is 420 again, which is the recovered number itself now
      that a tick is a retrace, and `intro::TICKS_PER_FRAME` is eight, because the intro's
      own pacer (`INTR.EXE` at `0x021f` reads `0000:046c`, adds two, and `0x022f` spins to
      it) is two BIOS ticks or 9.1033 frames a second, and 70.0863 over that is 7.70.
- [x] 85. **The town's five gadgets open five routines, and none of them is a menu.**
      `MOON:HWLOOP` (0xe35) and `WDLOOP` (0xd7a) are a `cmp word ptr es:[si+0xe]` ladder
      on the gadget fire was over, and each rung is `AddClickSound` and one call:
      ```text
      MERC  0e9c / WMERC 0ded   fade; mov ax, 5; call 0bdd3       the status panel, type 5
      TAV   0e90 / WTAV  0de1   call 0b007; fade                  _TAVERN
      HEAL  0eba / WHEA  0dfc   fade; call 0ba66; fade            _WIZARD, HEA.PIV
      HTEM  0eab                fade; mov ax, 6; call 0bdd3       the status panel, type 6
      MYST  0dd5                call 0b935; fade                  _WIZARD, MYS.PIV
      CEXIT 0e0b                call 0a4be; jmp EncounterAllDone
      ```
      then `jmp HWINIT` (0xe14) or `WDINIT` (0xd5c), which reloads the town and writes the
      pointer to (0x122, 0x64) or (0x1e, 0x64) before `InitHighWood`. `0xbdd3` is the
      unnamed entry of the panel: `mov [StatTYPE], ax`, the pointer to (0xa0, 0x64),
      `SetUpStatus`, `ReDisplay`, `StatLOOP`. So:
      **The merchant is the panel on type 5.** `DisplayMerchant` (0xc7ad) puts chain mail,
      plate and battle armour (cels 0x1c to 0x1e at (0x17, 0x91), (0x40, 0x91), (0x6a,
      0x90), ids 0x0e, 0x10, 0x12, `STRP` 0x1a, 0x2a, 0x4a, `STPL` 0x42), the broad sword
      and the claymore (0x17 at (0x32, 0x6b) id 0x16, 0x18 at (0x2f, 0x7d) id 0x18, `STPL`
      0x40) and thirteen daggers (0x15 from (0x20, 0x59) every nine, id 0xa, `STPL` 0x34)
      in the right arch, and `HotGadget`'s 0xa arm is `BuyGoods` (0xcd33): `BuyArmour`
      (0xcd4a) takes 0x1e, 0x32 or 0x4b, writes the suit into `+0x42` and adds 0xa, 0x14 or
      0x1e to the health before the routine at 0x28d, whatever was worn; `BuyWeapon`
      (0xcdb3) takes 0xa or 0x19 only while `+0x40` is below 0x17 or 0x18; `BuyDagger`
      (0xcdf7) takes two while `+0x34` is under ten. The left arch is `Identify`, so a
      potion there is looked at and not drunk. `Run::buy_goods`.
      **The high temple is the panel on type 6**, `Sell` on the left and `Purchase` on the
      right, and its right arch is `DisplayMagic` and `DisplayMSword` over DS:0xed96
      (`ReDisplay` 0xbf66), twenty four zero bytes that only `TTemple` writes: the temple's
      stock is what knights have sold it. `HGCastMagic` (0xca83) and `HGTakeMagic` (0xcbaa)
      both go to `TTemple` (0xce11) on type 6 before any permission bit, and `TTemple`
      divides the screen at `cmp word ptr [PointerX], 0xa0`: right of it a purchase at
      `MagicPrices[STPL]` (refused silently over the purse, a ring adding 0x14 health, a
      key or a moonstone moved as a bit by `BuyMoonstone` 0xce5a), left of it a sale by
      `SellToTemple` (0xce66) and `GoldSell` (0xce77): half the price into a purse capped
      at 0x96, `+0x40` back to 0x16 for the magic sword. `Run::trade_at_temple`, and the
      stock is `Run::temple`. The type the code passes for the stone circle is 3
      (`Henge+74`, 0x109a), which this tree had called the temple; renamed.
      **The tavern is the hand over `TAV.PIV`, with the six gadgets beside it.** The
      routine at 0xb007 refuses an empty purse (`cmp word ptr [si+0x32], 0; jg` at
      0xb00b), loads `dice.cel`, `CLEARGADGETS` (0xb053) and adds five stakes at (0x10c,
      0x26 / 0x42 / 0x5e / 0x79 / 0x93) 0x2b by 0x18 with `+0xe` 1 and `+0x10` the stake,
      and the exit at (0x10c, 0xb5) 0x2d by 0x10 with `+0xe` 2. `TavernOpenScene` (0xb0f5)
      writes `PointerX` 0x118, loads **`tav.piv`** (`Tav1`, DS:0xce0f, through 0xaff3),
      and `TavernLoop` (0xb137) runs the hand, writes `GOLDASCII` at (0x11a, 0xe) and
      shows the pointer; the handler at 0xb1a1 reaches `SetBET` (0xb1e8, the stake out of
      the purse now, refused over it) or sets `XFL` for the exit. **`dice.piv` is loaded
      by `RollDice+22` (0xb24a) and nowhere else**: it is the result, three faces at
      (0x73, 0xf), (0x31, 0x26), (0x4b, 0x58) and the `WIN` or `LOST` chain at x 0xae on
      rows 0x7e, 0x8c, 0x98 (the number writer at 0x7c24 and `GPTEXT` cut each string at
      ` gp.`), `DiceWait` (0xb31c) waiting twenty retraces and then `WaitFIRE`, and
      `DiceRND+3` running back into `TavernOpenScene`, which turns an emptied purse out.
      So the hand shakes on the table while a stake is chosen and the dice picture is
      what a throw's result is shown on, which is the other way round from the note
      that stood in section 5 and from the recipe this item was set as. `henge_core::town`
      and `henge_desktop::town`; the dice room and the tavern menu are deleted.
      **The healer and the mystic are the greeting, fire, the bowl, the verdict, fire.**
      0xba66 and 0xb935 load `HEA.PIV` or `MYS.PIV`, `LoadGoldCels`, write `Heal1a`
      (DS:0xdb69, rows 0xa3, 0xac, 0xb5) or `My1a` (DS:0xdd32, rows 0xa5 to 0xbd), and
      call `WaitFIRE` (0x8251); then the pointer to (0xa0, 0xaa) and `InitDonation` with
      `ax` 1 (the healer) or 0 (the mystic), which is `BAG`, the purse cel. The healer
      tests `cmp ax, 3` and then `DONATION`, the mystic `DONATION` and then `ax`, so an
      abandoned full bowl is `Heal2a` at one and `Heal3a` at the other. `HealDon` or
      `MysticUpDown` and the verdict chain (`ExitHealer` 0xbb1b, `MysticJudge` 0xb9fc),
      `MysticFini` (0xba20): fifty retraces, `WaitFIRE`, fade, return. `DonateLoop`
      reads `PointerFLAG`, which `MovePointer` clears while fire is down, so a held button
      pours coins two retraces apart; none of its four arms clicks. **The `Donate` and
      `Back` menu in front of the bowl is gone**, and `Donation` on the panel is right
      aligned to `TextRightBorder` (0x140) as `TextPTop` does, not to the 0x106 the call
      passes.
      **`TakeSword` (0xccd4) on a lair floor goes through the hand**: `[si+4] = 1`,
      `[di+4] = 0`, `[StatHAND1+0x40] = 0x19`, and the `0x16` written to `StatHAND2` is
      skipped on type 2. It used to take the sword as an ordinary item into the pack.
      **Also found on the way:** the tunes. 0xba66 is the healer, not `MysticUpDown+39`,
      and 0xb935 the mystic, not `_bestow_done+7`, so the healer plays tune 4 and the
      mystic tune 5 (the "moment inside the wizard's tower" was the mystic); the baker's
      table is keyed by door now. And `play.sh` passed `--force` as the baker's second
      positional, which baked the pack into a folder called `--force`; flags are skipped.
      **The 54.62 Hz timer is a different clock and drives only sound.** `Install_Timer`
      (`0x584f`) sets counter 0 to mode 3 with divisor `0x5555` and hooks int 8 to `0x5934`,
      whose whole body is `mov ah, 1; int 60h` for the music, the same with `int 61h` for
      the effects unless the card is 3, and a `Times3` counter that chains the original
      int 8 every third tick so DOS keeps its 18.2 Hz time. It touches no game state, and
      `henge_audio::music` already carried that rate inside the score, so the tunes did not
      move

---

## If you only did three things

**55 is finished, both halves.** The intro's tile maps and its animated cast were the
half that was open, and `INTR.EXE`'s ending half went in beside them: `CO.STI` as a
second panorama, the ten scenes at 0x00fa, and the exit byte `MOON:KnightWonGame` builds
handed straight to them. Item 69's shake went in with it: `DD_ShakeDice` and `DD_ThrowDice` are baked
and `henge_core::dice` is the loop that runs them. What is left on the list is the panel
pages nobody opens yet, which are the merchant's and the mystic's, and the dice screen's
own stake gadgets, which this pack still keeps on the tavern's menu.

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

Which creature waits on which ground, where everything but the two towns stands on the
map, and which guardian each lair holds. These were never recovered from the executable,
so finishing them means designing and playtesting, not translating. Worth knowing before
anyone estimates the end of this list.

**The ending screen and the final tally were on this list and are not any more**, and not
because they were recovered: because there is nothing there **in `MAIN.EXE`**. Both of its
endings are one message over `MESSAGE.PIV` and `WaitFIRE`, and nothing in it counts
anything. What was designed here has been removed rather than finished. The real ending is
in the other executable and is built: see item 55.

**The status panel's menu, the town front doors' menu and the donation's three amounts
were all here, and none of them survived a grep either.** The panel's menu was replaced by
`SetUpStatus` (0xcf45), `SetUpID` (0xd2b9) and `AddIconGadget` (0xc9ab), which turn every
icon on the screen into a gadget that says one of twenty seven recovered lines and runs one
of five recovered operations. The town menus were replaced by `InitHighWood` (0xec9) and
`InitWaterDeep` (0xf4a), five boxes each over words the artwork already carries. The
donation was replaced by `InitDonation` (0xbb34) and `DonateLoop` (0xbbe6), which move one
coin at a time. **Three more for the tally.** Two things this project's place screens did
went with them: picking the ink for a menu by counting the colours in the box behind it,
and the day-and-purse strip along the bottom of every place screen, which was the same
invention already taken off the map.

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
