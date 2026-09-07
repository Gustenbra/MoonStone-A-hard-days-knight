# Build order

Every item, once each, in the order you would actually do it. One flat list.

`COMPLETE.md` is the same work organised by subsystem, with the original's function names
against each part. This file is the checklist.

**77 items. 43 done, 2 partial, 32 remaining.**

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
- [x] 11. Sprite drawing: transparency, mirroring, masking, colour substitution
- [x] 12. Text: glyph map read off the artwork, silhouette rendering
- [x] 13. Arenas: 57 of them, scenery, walkable bounds, depth sorting
- [x] 14. Combat: positional hit lines, committed attacks, damage, death
- [x] 15. Bouts of up to four fighters, in the simulation, deterministic and serializable
- [x] 16. Overworld: travel, day cycle, ambushes, terrain classification
- [x] 17. Runs: wounds carry, travel mends, death ends the run
- [x] 18. Four locations with menus, and a working healer
- [x] 19. Sound: 49 clips, four cues, silent without a device

---

## Phase 1: the blockers

This was the research phase, and it is finished, and so is the construction that followed
it: 20 through 27 are done, and phase 2 with them. Phase 4 is open.

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
      row), `Slash`, `Knocked`, `KnockDead`. Five hit points and a blow of three at new
      moon; `SetRatmenTables` raises both with the moon, which is not in the game yet
- [x] 32. **Mudmen.** `Mudmen_Stance`, `Move1`, `Move3`, `Move1`, `Move2` (`MudmenWal`,
      exactly), `ArmAttack`, `Hit`, `Dies`. Thirty hit points, a blow of two, ranges
      80/75/5. `MudmenWALK` moves it eleven across and thirteen deep a frame, so it
      comes at you on a diagonal. `Mudmen_Appear` and `IBury`, the rising out of the
      ground, are behaviour and wait on 37
- [ ] 33. **Demon: partial.** It stands (`Demon_Stance1`), slaps (`Demon_Slap`, whose
      own five frames carry the whip; the script runs on into `Demon_Zap` and
      `Demon_Whip` on disk but its `TASKGOTO` to `Stance2` ends it first), is hurt
      (`Demon_Hurt`) and dies (`Demon_Death`). 250 hit points, ranges 95/90/2; its
      blow is not in a `*Dam` table, four stands in. **Missing**: `SETDEMONBORD`, the
      screen border; `Demon_Evolve`, the entrance; `Demon_Whirl` and `AddDemonWhirl`;
      the zap and the whip as separate attacks; `KnightOFF`/`KnightON`
- [x] 34. **Beast.** `Beast_Drool1` to stand (its `+0x10` stance), `Run1`..`4`,
      `LowerHit`, `LowerDead`. Ten hit points, a tracker that closes to two pixels. It
      has no swing: every run frame carries a weapon part, so the first run frame is
      its attack until the charge and the toss (`Beast_BackToss`, `ChestToss`, which
      are the knight's own animation on the beast's banks) are built under 37
- [x] 35. **Balok.** `Balok_Stance`, `Jump` and `Jumping` for a walk, `UpperCut`,
      `UpperHit`, `Dead`. Thirty hit points, a blow of four (`BalokDam`), ranges
      80/60/10. The grab and the three things it does to a held knight wait on 37
- [ ] 36. **Dragon: partial.** `Dragon_Stance`, `HighBite`, `Hit`, `Dead`, on the
      standard states; it creeps a pixel a tick on its standing frame so a plain
      opponent can reach it, which is ours. 200 hit points against a maximum of 120,
      as `SetUpDragonTables` writes them; a bite of ten (`DragonDam` gives 10 for a
      lunge and 30 for a swing). **Missing, and not invented**: the set piece.
      `BATTLEDRAGON` runs it with `DragonFLAGS`; the head lifts and lowers through
      the `DragonWal` rows; `LowBreath` and `HighBreath` are the fire with
      `TrackKnight`; the two claws are their own actors at fifty hit points each
      (`Claw1TABLE`, `Claw2TABLE`, `Dragon_Claw`, `ClawSlap`, `ClawDead`); and
      `Dragon_Flight1`..`8` are its map animation on `DRAGON5.CEL`
- [ ] 37. Per-creature behaviour. The tracker's ranges are recovered and carried on
      every actor (`approach`, `back_off`); what each creature does inside them
      (`TroggAttacks` picks by distance, the ratman leaps, the mudman rises, the beast
      charges) is read in outline and not built

## Phase 3: economy and character

Independent of phase 1. **Can start immediately, in parallel with the research.**

- [x] 38. Gold and prices. Coin off the fallen, a `bounty` per actor in the data,
      prices on the items, and both towns' merchants open. The healers inside the
      walls charge coin as well as days; the hermit in the woods still charges only days
- [x] 39. Inventory: carrying, using, losing. A bounded pack on the run, items as
      data with a price and a virtue, flasks bought and drunk, and losing made real
      both ways: a flask is spent when drunk, and a cutpurse on the road takes coin
      or, failing that, something out of the pack
- [ ] 40. Character stats and abilities
- [ ] 41. Potions beyond healing. Healing flasks landed with 39, as an item virtue
      in the data; anything a potion does other than mend waits on 42
- [ ] 42. Magic: spells, casting, costs
- [ ] 43. Curses
- [ ] 44. The hawk and the gem
- [ ] 45. Experience and levelling

## Phase 4: combat depth

All of it can start now. The attack tables (`KnightAttSw`: lunge, swing, knife, block,
right, up and over thrusts, evade, chop, and the damage of each in `KnightDamSw`) are
recovered with the bestiary's, so 46 is translation.

- [ ] 46. Attack variety, several by direction and button
- [ ] 47. Blocking and parrying
- [ ] 48. Weapon state: drawn, sheathed, dropped, thrown
- [ ] 49. Gore and dismemberment

## Phase 5: the shell

Independent of everything. Makes it feel like a game rather than a demo.

- [x] 50. **Title screen and attract mode.** The wordmark was in the font: `BOLD.F` has 76
      frames and the glyph map only ever used 66, and frames 73, 74 and 75 are the
      `Moonstone / A Hard Days Knight` logo and the two credit lines. The option list is
      `DoOptions`: four rows, a player count of one to four, a gore switch, practice combat
      and moon quest, clamping at both ends rather than wrapping, with the arrow at `ARX`
      50. What the original drew it over is in `INTR.EXE` and still unknown, so it goes
      over an intro plate, and attract mode cycles the other ten
- [x] 51. **Character select.** `CH.PIV` and `SEL.CEL`, both of which the pack had decoded
      and never shown, and the rules from `ChooseKnight`, `ChooseRefresh`, `FindChosen` and
      `ChooseFIRE`. `KnightGlowColours` gives each knight's three shades, so the four are
      blue, gold, emerald and red because the original says so, which is also what the
      initials on `BNAME`, `GNAME`, `ENAME` and `RNAME` stand for. **The four do not differ
      in stats**: `InitKnights` separates them by name, colour and which corner of the map
      they start in, and hands all four the same block. The data allows four different
      ones; what ships is the original's
- [x] 52. **A real status panel.** The knight record and `DisplayKnight` give the whole
      sheet: strength, constitution and endurance at `+0x2e`..`+0x30`, life points, gold,
      daggers, experience, health and its maximum, the weapon and the armour, with the
      arithmetic that turns them into a fight. One plate per fighter along the bottom of an
      arena, and the sheet itself on a key
- [ ] 53. Mouse pointer and clickable widgets
- [ ] 54. The message system: wait, occurrence and instruction messages
- [ ] 55. The intro sequence (`INTR.EXE`, never examined)
- [ ] 56. Save and load. The original has none, so this is ours to design

## Phase 6: the world

38 is done, so the merchant is open and the tavern has something to charge for.
57, 58, 60 and 61 are done: the map's own tables are in the game.

- [x] 57. **Recovered: `_MAP:MapType`, 40x26 bytes, one family code per 8x8 block of the
      map picture.** Codes 0, 2, 4, 6 are plain, forest, swamp and waste, which is the
      order `MOON:ColourBackdrop` compares against. `_MAP:CalcKnGrid` builds the index from
      the traveller's token: `((x+4)>>3, (y+10)>>3)`, the middle of his feet. Checked by
      drawing the grid's own boundaries over the map artwork, where they follow the
      treeline, the marsh edge and the mountain ridge. The location graph is **half
      recovered**: the two towns' coordinates and the nine kinds of place and their menu
      lines are out of the executable, but `MOON:MapIconsTABLE` itself is uninitialised
      data and is not in the load image, so where the other seven sit is not recovered
- [x] 58. **Recovered: four tables of eight, and a counter, not a roll.** `PlainTable`,
      `ForestTable`, `SwampTable` and `WasteTable` each hold eight filename pointers, and
      generating an arena reads `Table[counter]`, then `inc counter` and `and counter, 7`.
      The six lair layouts per family are not in those tables and so never come up on the
      road. Also recovered: `TileTable` gives plain and forest the same `FO1` scenery
      sheet, and a placement whose selector byte is 4 draws from `FO2` instead, whatever
      the family. Checked by compositing an arena all three ways and looking: only that
      one makes a coherent picture
- [ ] 59. The moors arena family, which we do not render at all
- [x] 60. **Recovered: nothing on the map is impassable; ground is slow instead.**
      `_MAP:MapSLOW` is a second grid on the same index holding a two-bit mask, and
      `_MAP:CheckSLOW` refuses the step when `counter & mask` is not zero, having already
      charged it to the day. Forest and marsh are half speed, the mountain spine a quarter.
      The only hard limit is a rectangle: `_MAP:HawkBorders` clamps the token to
      `0..=310` by `0..=190`
- [x] 61. **Settled, and negative: the overworld map does not scroll or pan.** `MAP.CMP`
      is one 320x200 picture, `_MAP:SHOW` hands the token's position straight to the
      blitter with nothing subtracted, and `HawkBorders` bounds that position to exactly
      one screen. `_MAP:ScrollINPUT`, despite the name, reads the keyboard. The `SCROLL`
      and `PAN` symbols carry no addresses and belong elsewhere; `SCROLLX` in `_STATUS` is
      the status panel's own icon cursor
- [ ] 62. Lairs: placement, entry, contents, the guardian fight
- [ ] 63. Moon phases on a weekly cycle, and the between-days screen
- [ ] 64. What the moon gates
- [ ] 65. Tavern: what it offers
- [ ] 66. Temple and mystic services
- [ ] 67. The wizard: abilities, gold and magic bestowal
- [ ] 68. What the stone circle does
- [ ] 69. The dice game. Rules unknown, so design

## Phase 7: the quest

**The point of the game.** Needs lairs (62) and inventory (39). Almost none of this is
recoverable from the symbols, so most of it is design and playtesting rather than porting.

- [ ] 70. The four keys, one per lair
- [ ] 71. The moonstone: where it is, what retrieving it takes
- [ ] 72. Win condition and ending
- [ ] 73. Scoring and the final tally
- [ ] 74. Losing properly, rather than a run simply stopping

## Phase 8: polish

Any time. None of it blocks anything.

- [ ] 75. Palette fades and colour cycling
- [ ] 76. Gamepads, with calibration and debounce; and rebindable controls
- [ ] 77. Music. The tune files are x86 driver blobs with data welded into code, so either
      trace the driver or commission new music

---

## If you only did three things

**37** is what makes the nine creatures fight like themselves rather than like a
knight in a costume, and the ranges it needs are already on every actor. **65** is
cheap now that 38 gave the tavern something to charge for. **45** is
next to free: experience is already counted and displayed, and `AdjustLevel` says exactly
what spending it does.

## What is not portable

Some of the original has no Rust equivalent worth writing: expanded memory paging, disk
swapping, the manual copy protection, DOS file handling. Accounted for in `COMPLETE.md` so
the symbol coverage is checkable, and deliberately absent here.

## What is design rather than translation

Items 37, 56, 64, 69, and all of phase 7, and which creature waits on which ground.
These were never recovered from the executable, so finishing them means designing and
playtesting, not translating. Worth knowing before
anyone estimates the end of this list.
