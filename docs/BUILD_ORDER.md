# Build order

Every item, once each, in the order you would actually do it. One flat list.

`COMPLETE.md` is the same work organised by subsystem, with the original's function names
against each part. This file is the checklist.

**77 items. 61 done, 2 partial, 14 remaining.**

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
of everything but the two towns are still ours.** `MOON:MapIconsTABLE`, `LairLocation`
and `LairType` are all inside the 2,906 bytes of DGROUP the load image carries as a stale
duplicate. What is recovered is the *shape* of the answer, and for the lairs the ground:
each one is sited on a cell of its own family in the real `MapType` grid, which is why a
`fol3.t` lair stands under trees.

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

**The point of the game.** Needs lairs (62) and inventory (39), and both are now done:
the four keys are planted one to a family and come out of a lair into the pack. Almost
none of the rest is recoverable from the symbols, so most of it is design and playtesting
rather than porting.

- [ ] 70. The four keys, one per lair. **Half done by 62**: the lair initialiser plants
      one in each family's six, `LairWon` hands it over, and `Run::keys_held` reads them
      back the way `MOON:Valley` tests the four bits of `+0x14`. What is missing is the
      Valley of the Gods itself, which is what four keys are for
- [ ] 71. The moonstone: where it is, what retrieving it takes. Everything that reads
      one is built and waiting: `MOON:Henge` ends the game for a knight in the circle
      with the stone of the night, `CalcDamage` doubles his blow while he carries it, and
      the temple's own `pu18` and `se18` lines buy and sell it. Nothing hands one out
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
knight in a costume, and the ranges it needs are already on every actor. **70** and
**71** are next to free now: the four keys are already hidden, planted and carried, and
the stone circle and `CalcDamage` are both already waiting for a moonstone that nothing
hands out. **54** would give every one of the doors that opened in phase 6 somewhere
better to put its words than a box the renderer picks a colour for.

## What is not portable

Some of the original has no Rust equivalent worth writing: expanded memory paging, disk
swapping, the manual copy protection, DOS file handling. Accounted for in `COMPLETE.md` so
the symbol coverage is checkable, and deliberately absent here.

## What is design rather than translation

Items 37 and 56, and all of phase 7, and which creature waits on which ground, and
where everything but the two towns stands on the map, and which guardian each lair
holds. These were never recovered from the executable, so finishing them means designing
and playtesting, not translating. Worth knowing before anyone estimates the end of this
list.

**64 and 69 were on this list and are not any more.** What the moon gates is spelled out
in `SetRatmenTables`, `CalcDamage` and `Henge`; the dice game's odds are eleven records
in `_TAVERN`. Both were taken for design because the plan was written against the 334
`PUBLIC` names, before the other 1,889 symbols were recovered. Anything still marked
design here is worth a grep before it is invented.
