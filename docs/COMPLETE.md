# Everything left to build

> Organised by subsystem. For the same work as one flat checklist in build order,
> see [BUILD_ORDER.md](BUILD_ORDER.md).

An exhaustive plan for a complete Rust reimplementation.

This is built around the symbols left in the original executable's debug info, which is the
closest thing to a definitive list of what the game actually does. Every symbol below is a
real function or variable from the original build. Anything not covered by a symbol is
marked as such, so you can tell recovered fact from design decision.

The plan was originally written against the 334 `PUBLIC` names, which are all that is
visible without unpacking the executable twice. **There are 2,223 symbols with addresses**
(section 1.2), so the coverage below understates what is now recoverable.

**Status key**

| | |
|---|---|
| **done** | built and verified in henge |
| **partial** | exists but incomplete or approximated |
| **todo** | not started, but understood |
| **blocked** | cannot start until something else is decoded |
| **design** | never recovered; has to be invented rather than ported |

Honest headline: **most of it done**, and what remains is the quest, the creatures' own
behaviour, the two set pieces, and the shell's message system and mouse.

---

# 1. Blockers

Two things gated large parts of everything else. Both are now research-complete: what is
left of 1.1 is construction, not investigation.

## 1.1 The animation task VM `done`

The original runs animations as **bytecode on a small virtual machine**, and composes each
character from **several sprite parts per frame**. 39 symbols implement it:

```
INITTASK CLEARTASKS ADDTASK NEWATASK FINDTASK STOPTASK TASKSTANDBY TASKHANDLE
TASKSEQ TASKPERFORM ANIMSEQ REPLACEANIM ENDOFFRAME ENDOFTASK
TASKGOTO TASKJUMP TASKLOOP TASKSKIP TASKGOSUB TASKHOLD TASKTIME
TASKLEFT TASKRIGHT TASKPLACE TASKMOVE TASKFLIP TASK_FLIP TASKSORT
TASKSHADOW TASKSAVE TASKDEAD TASKSOUND TASKCELBUF TASKANIMCLR
TASKTESTEQ TASKTESTNE TASKADDTASK TASKKILLTASK TASKCOLLISION TASKWALKCOLLIDE
CONTROLDONE CONTROLTABLE GROOC
```

**The instruction set and the frame record are recovered.** `docs/TASKVM.md` is the
reference: nineteen commands with their operand widths and meanings, three empty table
slots, the three end-of-frame terminators, the six-byte sprite-part record, the task and
VM state record layouts, and the bank table for every creature. `tools/taskvm.py` reads it
back out of `MAIN.EXE` and disassembles any script.

The scripts themselves are named data in DGROUP: 236 of them, from `Knight_SwWalkR1` to
`Balok_Blink`, all of which parse cleanly. The count was 221 until the bestiary was
built: the mudmen's prefix is `Mudmen`, not `Mudman`, and their fourteen scripts and
`Rat_TreeBrush` had been filtered out of every count.

**Steps**

- [x] **Locate the interpreter.** `PerformCOMMAND` at image offset 0x97fb, loop head
      `PerformLOOP` at 0x97f2, inside `_TASK`'s range 993d-a385. Confirmed by disassembly
      as a byte-at-a-time fetch and dispatch, not by name. The `mov bx, 0x9448` that
      indexes the table matches the `TaskComTable` symbol address exactly
- [x] **Recover the opcode set.** `TaskComTable` is BSS, so the handler addresses exist
      only as immediates in `INITTASK`, and they are link-time offsets needing the same
      correction `symbolmap.py` fits for code symbols. Corrected, all nineteen land exactly
      on a routine entry. Operand widths are read out of each handler's own
      `add word ptr [di+2], n`, not guessed
- [x] **Recover the per-frame part record.** `[u8 bank*4][u8 cel][i8 y][u8 flags][i16 x]`.
      The earlier guess had x and y the wrong way round and read the bank selector as a
      plain index rather than the slot times four. **Verified by compositing and looking**:
      the knight's walk cycle, stance and sword swing all come out as coherent figures, the
      mirrored form stays assembled, and the troll, trogg, ratman and balok composite too
- [x] **The VM.** `henge-core/src/taskvm.rs`: `PerformCOMMAND`, `ENDOFFRAME` and all
      nineteen handlers as a deterministic interpreter, integers only, stepped once a
      tick, returning parts as logical data. `TASKGOSUB` and `TASKSOUND` are emitted
      effects (`Effect::Gosub` names the routine and its kind out of a table of all 37;
      `Effect::Sound` names the sample); none is faked and none is skipped silently.
      Transcribing the end of frame handler found that `ff ff` honours a running
      `TASKLOOP` before ending anything, which `TASKVM.md` now records
- [x] **Exported.** `henge-formats/src/taskvm.rs` reads the table and widths out of the
      image, checks them against the documented set, and parses all 236 scripts into the
      engine's own instruction type; the baker writes `data/scripts.json` and, per
      creature, the four bank tables with cel sizes to `data/banks.json`
- [x] **The knight.** Stance, the four walk frames as a cycle, swing, shoulder hit and
      death, drawn part by part through the bank tables with the `TASKLEFT` mirror term.
      Hit shapes are the `WEAPON` flagged parts of the frame being shown. The hand
      authored frame lists are deleted. Verified against the `tools/taskvm.py` composites
      by eye, and by a traced duel that still resolves

**Still to do here**: the other three attacks (`SwChop`, `SwLunge`, the thrusts) and
the up and down walks, which wait on 46; `TASKSHADOW`, which no knight script uses;
playing `Effect::Sound` and the sound gosubs through `henge-audio`, which still derives
its cues from state changes. `CONTROLTABLE` is uninitialised data and not in the load
image, so which script a state plays is ours and lives in `actors.json`.

**Unlocked**: all eight creatures, shadows, gore, correct sorting.

## 1.2 Symbol name to address mapping `done`

**2,223 symbols, with addresses, recovered by `tools/symbolmap.py`.**

`MAIN.EXE` is packed twice: PKLITE outside, Microsoft EXEPACK inside. The tool runs both
decompression stubs under emulation and writes the true 178,224-byte load image, then
parses the eleven per-module blocks of the TASM symbol table it contains:

```
[u8 reclen][u8 kind][payload][u8 namelen][name]

kind 0x05   u16 offset, u16 segment, u16 type    data      addr = seg*16 + off, DGROUP 0x123b
kind 0x0b   u16 offset, u8 0                     code      addr = image offset
```

Code offsets take a seven-step monotone correction between 0 and -473, which the tool
derives from branch targets rather than assuming. 1,778 of the 2,223 corroborate
independently: code symbols by landing on a called branch target, data symbols by being
referenced from code, and several by content (`CelFile1` at `KN1.OB`, `BloodFile` at
`BLO.CEL`, `map` at `MAP.CMP`, `Enemy1Name` at `SIR BANNER`). The eleven code ranges come
out contiguous and in link order.

### This section previously said the opposite, and was wrong

It recorded that no address table existed, with a table of the appended region fully
accounted for and a list of everything ruled out. That search was real and honestly
reported. It was also run **against the EXEPACK'd image**, where the symbol table sits
inside RLE-compressed bytes and reads as loose name strings with junk between them. The
error is kept in the record rather than deleted: a negative result about file structure is
only as good as your confidence that you are looking at the file. `REVERSING.md` has the
full account.

The 327 `PUBLIC` names live in a separate appended blob and still carry no addresses. They
are not needed; the 2,223 that do carry addresses already cover the code.

---

# 2. Platform and engine

## 2.1 File formats `done`

| Original | Ours |
|---|---|
| `DECODE`, `LOADFILE`, `OBJLOADC`, `OBJLOADV`, `DATVAC` | `henge-formats` |
| `LOADSCREEN`, `UNPACKSCREEN` | `Piv` |
| `LOADCOL`, `COLFIL` | `Collide` |

All 386 files decode. See `FORMATS.md`.

## 2.2 Graphics `partial`

| Original | What it is | Status |
|---|---|---|
| `SPRITE` | the sprite blitter | done |
| `FLIPX`, `MAKE_FLIP_T` | mirroring, via a precomputed flip table | done (no table; we mirror directly) |
| `SHOWTILE`, `LOADTILEV`, `GET_TILE_SCREEN`, `TILEDATA`, `TILEFLAG` | tile drawing | done |
| `COPYBACKDROP`, `COPYTOLOGIC`, `SHOWLOGIC`, `_PHYSIC`, `_LOGIC` | double buffering | done implicitly |
| `LOADPALETTE`, `SETCOLOR`, `CHANGE_DAC`, `IFF_PAL`, `RGB_PAL` | palette loading | done |
| `FADEPALETTEIN`, `FADEPALETTEOUT`, `FADEOUTDAY` | palette fades | **todo** |
| `COLOURCYCLE`, `COLOURGLOW` | animated palette entries: water, fire, torches | **todo** |
| `COLOURENKNIGHT` | per-player knight recolour | done (by hue substitution) |
| `SCROLL`, `PAN`, `SETSCREENOFFSET`, `AROFFSET` | scrolling the map | **settled: the overworld map does not scroll** |
| `BORD`, `BORDERS`, `SETDEMONBORD` | screen border, and a special one for the demon | **todo** |
| `CLS`, `VBI`, `WAITVSYNC`, `WAITVBS` | clear, vblank sync | done via the frame loop |
| `CONVERTSCREEN` | planar to linear conversion | done at bake time |

- [ ] Palette fades in and out, as scene transitions
- [ ] Colour cycling. Named entries animate on a timer; find which indices cycle per scene
- [x] Map scrolling: confirmed, and there is none. `MAP.CMP` is one 320x200 picture,
      `_MAP:SHOW` passes the token's position straight to the blitter with nothing
      subtracted, and `_MAP:HawkBorders` bounds that position to `0..=310` by `0..=190`,
      which is one screen. `SCROLL`, `PAN`, `SETSCREENOFFSET` and `AROFFSET` carry no
      addresses, so where they are used is not established; `_MAP:ScrollINPUT` reads the
      keyboard despite its name, and `SCROLLX` in `_STATUS` is the status panel's own
      icon cursor
- [ ] Screen borders

## 2.3 Text `done`

`TEXTPRINT`, `TEXT`, `TEXTCONVERT`, `FONTBUFFER`. Glyph order read off the artwork.

The tail of each bank has now been read the same way. Both fonts run on past the letters
with `#`, `$`, `%`, a blank, an apostrophe and then one more that differs: a diagonal
stroke in `SMALL.FON` and a horizontal bar in `BOLD.F`. The small font's slash is what
lets a status panel print health the way the original does, as `have/most`.

**`BOLD.F` is not all font.** Its last three frames are the title screen: a 305 by 54
`Moonstone / A Hard Days Knight` wordmark and two credit lines. See section 8.1.

- [ ] Recover the real lookup table once 1.2 lands, and check ours against it

## 2.4 Input `partial`

| Original | Status |
|---|---|
| `INSTALLKBD`, `REMOVEKBD`, `KBDHANDLER`, `READKEY`, `KEYPRESSED`, `WAITKEY`, `CLEARKEYS` | done |
| `POLL_JOY0`, `READJOY`, `BOUNCEBUTTON`, `JOY_XMIN/XMAX/YMIN/YMAX` | **todo**: gamepads, with calibration and debounce |

- [ ] Gamepad support. Four players on one keyboard is cramped; the original expected sticks
- [ ] Rebindable controls `design`

## 2.5 Audio `partial`

| Original | What it is | Status |
|---|---|---|
| `LOADSAMPLES`, `SETUPSAMPLES`, `PLAYSAMPLE`, `PLAY_SFX`, `STOP_SAMP`, `SFX`, `SFXTYPE` | sound effects | done, 4 cues of 49 samples |
| `TASKSOUND` | sound triggered from an animation frame | data recovered: opcode 0x92, 124 calls, 49 distinct samples. Needs 25 |
| `LOADMUSIC`, `MUSIC`, `MUSICTYPE` | music playback | **blocked** |
| `ADDCLICKSND` | UI click | todo |

- [ ] Wire the remaining 45 samples to events
- [ ] Sounds carried on animation frames rather than inferred from state changes. The
      sample number and the exact frame are in the recovered scripts
- [ ] **Music.** The tune files begin with x86 machine code: they are driver blobs with data
      welded into executable code, not a readable format. Either trace the driver to recover
      note data, or commission new music `design`

## 2.6 Memory and DOS `done, not needed`

`ALLOCATE`, `DEALLOCATE`, `REALLOCATE`, `INITMEM`, `FREEMEM`, `EMM_*` (10 symbols),
`_OPENF`, `_CLOSEF`, `_READF`, `CHECKDOSERROR`, `PSP`, `FINDDISK`, `PROTECTION`,
`CHECK_EMM`, `DELAY`, `_WRCHAR`, `_WRSTR`, `_WRCARD`, `_WRLN`.

Expanded memory paging, disk swapping and the manual copy protection are artefacts of 1991
hardware. Nothing to port. Listed so the symbol list is complete.

---

# 3. Combat

## 3.1 Core `done, but for the sheathed sword`

| Original | What it is | Status |
|---|---|---|
| `CALCHIT` | resolve a strike | done, positional hit lines |
| `CALCMOVE` | movement and bounds | done |
| `CLEARCOLLISIONS`, `TASKCOLLISION`, `TASKWALKCOLLIDE` | collision | partial; the strike point `CXx`/`CY` is the overlap's middle here |
| `ControlKnight`, `KnightAttack`, `Rjoystick`, `Ljoystick`, `KnightAttSw` | the attack the direction picks | **done**, recovered: `Attack::for_direction` |
| `KnightHitSw`, `KnightDamSw`, `*Hit`, `*Dam`, `KnightSAnim`, `CalcDamage` | blow taken and damage by kind | done, as `hurt_by` and `attacks` on every actor; `CalcDamage`'s strength and sword are item 40 |
| `CheckBlock`, `blockflag`, `KnightBloSw` | blocking | **done**, recovered; `KnightBloSw` is the block table |
| `KnightHitNormal`, `KnightHitKnight`, `TroggHit`, actor `+0x12` | the recovery a landed blow cuts a swing into | done as `State::Recover` |
| `SWORDFLAG` | weapon state, drawn and sheathed | todo |
| `KnifeThrow`, `ControlKnife`, `Knife`, `SpeedKnife`, `KnifeDam`, `SetKnightEquipment` | the thrown dagger | **done**, as a `Missile` in the bout |
| `BLOW` | a landed blow | done as `HitEvent`; a stopped one is a `Parry` |
| `GORESWITCH` (DS:0x700), `OSWITCHES`, `GOREOPT` | the gore switch | **done**: the title row toggles it, `Bout::bloodless` carries it |
| `AddBlood`, `Blood1`, `BloodFile`, `BLOODBUFFER` | blood | done: the spray at the strike point on bank table 4, from the three `*Struck` that call it |
| `DeCapFLAG`, `SetDecapFLAG`, `Knight_SwDeCap`, `Knight_SwCollapse`, `MudmenStruck1`, `KnightKnightStruck1`, `TroggChopHead` | the finisher on a fallen knight | done; the plain opponent comes in for it with the gore on |
| `Knight_Explode`, `TrollOHead`, `TroggSpear_Toss`, `DrDropHead`, `DrDropClaws` | the creatures' own finishers | todo, with 37 and 36 |
| `ALLDEAD` | everyone down | done as `Bout::settled` |
| `KNIGHTREFRESH`, `KNIGHTLOC`, `KNIGHTBUFFER`, `ENEMYBUFFER` | actor state | done as `Fighter` |
| `NUM_PLAYERS`, `PLAYER1`-`PLAYER4`, `PLAYERPOINTER` | up to four players | done |
| `GAME_XP` | experience | **todo** |

The knight's controller was read in full for build order items 46 to 49, and it is
where the pieces that had been taken for design turned out to live. The account of
what each routine does, what was reproduced and what was simplified, is against
those items in `BUILD_ORDER.md`; the tables themselves are in `TASKVM.md`.

- [x] **Attack variety.** Eight, by the direction held with fire, from `Rjoystick` and
      `Ljoystick`; fire alone, which the original leaves at the stance, is the swing here
- [x] **Blocking.** `CheckBlock` as written, one quirk included; a reeling knight
      never blocks, which is a simplification
- [x] Gore and dismemberment: the switch, the gated parts, the blood, the knight's
      decapitation and collapse. The creatures' own finishers wait on their behaviour
- [ ] Experience and levelling
- [ ] Weapon state: the thrown dagger is done; drawn, sheathed and dropped are not

## 3.2 Creatures `done, on the standard states`

Loaders exist for each: `LOADKNIGHT`, `LOADHEAD`, `LOADTROLL`, `LOADTROGGAXE`,
`LOADTROGGSPEAR`, `LOADRATMEN`, `LOADMUDMEN`, `LOADDEMON`, `LOADDRAGON`, `LOADBALOK`,
`LOADBEAST`.

Every creature runs its own scripts on the task VM, through its loader's bank tables,
in the same five states as the knight, and the road fields them by terrain. What each
one plays, and where its numbers came from, is in `BUILD_ORDER.md` items 28 to 36 and
in `TASKVM.md`.

**The controller tables were not lost.** `SetKnightAnims` and `SetMonsterAnims` fill
them at start-up, and the `Set*Tables` routine each `InitKnightvs*` calls writes the
stat block. These are recovered and in the pack:

| Original | What it is | Status |
|---|---|---|
| `TroggWalAxe`, `TrollWal`, `MudmenWal`, `RatmenWal`, `KnightWalSw` ... | walk cycles, right at +0, up at +0x10, down at +0x20 | done, right row; up and down rows wait on 37 |
| `KnightAttSw`, `TroggAttAxe`, `TroggAttHammer` | attack script by attack kind | done, as each actor's `attacks`; which of its own a creature picks when is still 37 |
| `KnightHitSw`, `TroggHitAxe`, `RatmenHit`, `MudmenHit`, `DragonHit`, `BeastHit2` ... | blow-taken script by the attacker's attack kind | done, as `hurt_by`, with each script's own `TASKDEAD` choosing the death |
| `KnightDamSw`, `TroggDamAxe`, `TroggDamHammer`, `RatmenDam`, `TrollDam`, `MudmenDam`, `BalokDam`, `DragonDam` | damage by attack kind | done, on each actor as `damage` |
| actor record `+0x38`, `+0x3c`, `+0x52`, `+0x54`, `+0x56`, `+0x35`, `+0x18` | health, maximum, approach, back-off, plane, kind, bank table | done; approach and back-off carried, not yet read |
| `TroggWALKR`/`U`/`D`, `TrollWALKR`, `MudmenWALK`, `BKnightWALKR`/`U`/`D`, `BeastChargeOffsets` | pixels moved per walk frame | read; speeds set from them |
| `MonsterTrack`, `CheckZAxis`, `CheckXAxis`, `FaceKnight`, `MonsterWalk`, `NextWalk` | the tracker: close to `+0x52`, retreat inside `+0x54`, same plane within `+0x56` | read; the plain opponent stands in |
| `TroggStart`, `TroggAttacks`, `TroggChop`, `TroggSwing`, `TroggStruck`, `BeastMove`, `MudmenSd` ... | per-creature behaviour | **todo**, item 37 |
| `TroggTABLE`, `BeastTABLE`, `RatmanTABLE`, `MudmanTABLE`, `BalokTABLE`, `DemonTABLE` | the spawn tables `InitNewMO` reads position and facing from | not readable: they sit in the first 2,906 bytes of `DGROUP`, which the unpacked image holds as a stale copy of another region |
| `TotalMonsters`, `MaxMonsters`, `AdjustLevel`, `lev_adjust`, `KLTAB` | how many come, in waves, scaled to the knight | read in outline; one at a time is fielded |
| `SETDEMONBORD` | the demon's screen border | **todo** |

- [x] Troll, trogg with axe (and hammer), trogg with spear, ratmen, mudmen, beast, Balok
- [x] Demon and dragon on the standard states; see 3.3 and item 33 for what is not
- [ ] Per-creature behaviour (item 37). `design` was the word here before; it is
      less than that now. The tracker and each creature's attack choice are named
      routines in `MOON` and have been read in outline, so building them is
      translation with some judgement, not invention
- [ ] `SETDEMONBORD`: the demon changes the screen border
- [ ] Waves: `TotalMonsters` and `MaxMonsters`, three troggs one after another, two
      ratmen at once, `AdjustLevel` adding more for a stronger knight

## 3.3 The dragon `partial`

`BATTLEDRAGON`, `DRAGON`, `DRAGONOVER`, `DRAGONFLAG`, `DRAGONDEADFLAG`, `LOADDRAGON`.

A distinct set-piece encounter with its own state machine, not an ordinary bout. What
is built is the dragon as an ordinary fighter: it stands, bites, is hurt and dies on
its own scripts, with the hit points `SetUpDragonTables` gives it. What is not built,
and has not been guessed at: `DragonFLAGS` and the states it encodes; the head
lifting and lowering through `Dragon_LiftHead1`..`5` and `LowerHead1`..`5`; the fire
(`Dragon_LowBreath`, `HighBreath`, `Dragon_Fire`, `TrackKnight`, `AddDragonFIRE`); the
two claws, which `InitKnightvsDragon` sets up as actors of their own with fifty hit
points each (`Claw1TABLE`, `Claw2TABLE`, `DEAD_CLAWS`); `Knight_Burn` and
`Knight_BurnDeath`, which that routine installs as the knight's blow-taken scripts;
and `Dragon_Flight1`..`8`, its flight over the map on `DRAGON5.CEL`.

---

# 4. The overworld

## 4.1 Travel `partial`

| Original | What it is | Status |
|---|---|---|
| `LOADMAP`, `MAKEMAP`, `MAP_CMP`, `MAP_CHANDLE` | the map | done |
| `MAPLOCATE`, `POX`, `POY` | position | done, and the token's top-left as the original stores it |
| `CHECKENCOUNTERS`, `ENCOUNTERAREA` | ambushes | ours, not theirs |
| `CHECKY`, `CHECKY2` | movement validity | done as `MapSLOW` and `HawkBorders` |
| `FINDWHICH`, `WHICH`, `DISTAN`, `MAX_DISTAN` | proximity | partial |
| `LANDSCAPE`, `LANDTYPE`, `LANDFILE` | terrain type | **done, from the real table** |
| `SCROLL`, `PAN` | scrolling | **settled: the map does not scroll** |

**The terrain table is recovered.** `_MAP:MapType` is 40x26 bytes, one per 8x8 block of the
320x200 map picture, holding 0, 2, 4 or 6 for plain, forest, swamp and waste;
`_MAP:FindLandscape` reads it and `MOON:ColourBackdrop` branches on those four codes.
`_MAP:CalcKnGrid` builds the index from the traveller's own token as `((x+4)>>3,
(y+10)>>3)`, which is why the grid needs a twenty-sixth row it never draws. The colour
classifier is still in `henge-core` as a fallback for a pack baked without the unpacked
executable, and is marked as such.

**`CHECKY`/`CHECKY2` are answered, and the answer is that nothing is impassable.**
`_MAP:MapSLOW` is a second grid on the same index holding a two-bit mask, and
`_MAP:CheckSLOW` refuses a step when `SlowDELAY & mask` is not zero. The step is still
charged to the day before it is thrown away, so hard ground costs time rather than
blocking. Forest and marsh are half speed, the mountain spine a quarter, and the only hard
edge is the rectangle `_MAP:HawkBorders` clamps the token into.

**The location graph is half recovered.** `_MAP:KnightGoesToTown` carries Highwood at map
(94, 47) and Waterdeep at (297, 157), and cross-checks them against grid cells (12, 7) and
(37, 20), which is exactly what the recovered index formula makes of those pixels.
`MOON:CheckGROOC` decides you have arrived by overlapping the place's `MI.C` icon
rectangle with the traveller's own 8x10 one, so a place is a box and not a radius, and
`MOON:StackMessages` gives the nine kinds and their menu lines. What is **not** recovered
is `MOON:MapIconsTABLE` itself: it is uninitialised data, the first 2,906 bytes of DGROUP
in the load image are a stale duplicate, and the seven places that are not towns therefore
have no recovered coordinates. See `REVERSING.md`.

**So the places that are not towns were put where the artwork puts them**, and that
decision is now taken: the healer keeps the ruin in the southern woods, the stones are the
ring the map draws in the middle of it all, and Math's tower is the lone dark tower
standing in the northern waste, which is the only building on the picture nothing else
claims. The twenty four lairs are sited by the recovered terrain grid rather than by eye;
see 4.3. Every one of the recovered menu lines is used as its own gadget's label.

- [x] The real terrain table
- [x] `CHECKY`/`CHECKY2`: what makes ground impassable
- [x] Scrolling: settled, and negative
- [x] The rest of the location graph, as far as it can be: the two recovered towns, the
      four other fixed places on their own landmarks, and the lairs on their own ground
- [ ] The four home villages, one per knight, which `StackMessages` names four times over
      (`Enter Village`, and the entry is skipped unless the knight's index matches). Four
      villages is a mechanic henge does not have at all, and the between-days screen tells
      you to `Visit your home village to restore lost lives`
- [ ] The Valley of the Gods (`knvalley`), which is what four keys are for, and pillaging
      a dead rival's grave (`kngrave`). Both are phase 7's

## 4.2 Arena generation `done, except the moors`

`GENERATELANDSCAPE`, `GENERATEFOREST`, `GENERATESWAMP`, `GENERATEWASTE`, `GENERATEMOORES`,
plus counts `FORESTCOUNT`, `SWAMPCOUNT`, `WASTECOUNT`, `PLAINCOUNT`.

**The tables are four of eight, not four of six**, and the plan's guess at `F1..F6` was
short by two. `_LOADER` holds `PlainTable`, `ForestTable`, `SwampTable` and `WasteTable`,
sixteen bytes each, every entry a pointer to a filename:

```
GL1.t .. GL8.t      plain      counter at DS:8995
FO1.t .. FO8.t      forest     counter at DS:8997
SW1.t .. SW8.t      swamp      counter at DS:8991
WA1.t .. WA8.t      waste      counter at DS:8993
```

**The choice is a rotation, not a roll.** Each family's loader reads `Table[counter]`,
loads it, then does `inc counter` and `and counter, 7`. The four counters are the four
`*COUNT` publics. The six lair layouts per family (`FOL1..6` and the rest) are in
`MOON:LairFile`, not in these tables, so they never come up on the road.

`TileTable`, four words indexed by the landscape code, says which sheet the scenery is cut
from: `FO1.CMP` for both plain and forest, `SW1.CMP` for swamp, `WA1.CMP` for waste. A
placement whose selector byte is 4 draws from `FO2.CMP` instead, whatever the family, which
is what the routine's `cmp ax, 4` does before it consults the table. Every `.T` placement
in the game carries 3, 4 or 0xfe there.

- [x] The selection tables, and the rotation
- [x] Which sheet a placement draws from
- [ ] The moors family. There are four tables and four counters, not five, and no `MO*`
      file exists in the game data. In the symbol table `MooresVillage`, `ForestVillage`
      and `WasteVillage` are three names for one address, 0x112a, so "moores" is at least
      sometimes another word for ground the game already has. Whether `GENERATEMOORES` is a
      fifth family or a second name for the plain one is not established either way, and
      that is what item 59 has to settle

## 4.3 Lairs `done`

`LAIRSTART`, `FINDLAIR`, `FINDCLOSELAIR`, `LAIRTABLE`, `LAIRPOINTER`, `FMEM_LAIR`, and
with addresses: `MOON:InitLair`, `LairFill`, `GoldLair`, `MagicLair`, `GoldMagicLair`,
`LairWon`, `LairGEM`, `CheckLairClear`, `CheckLairEncounter`, `MOON:LairRND`,
`MOON:LairFile`, `fmem_LairMagic`, `_MAP:DisplayLairs`, `TrackLair`, `CloseLair`,
`LairFLAG`, `NoLairsFLAG`, `_STATUS:DisplayLair`.

**Twenty four lairs, six to a family, and the quest begins in them.** The table is 24
records of 18 bytes at DS:0002, built by the initialiser at image 0x1e00:

```
+0x00  the lair's own item record, 24 bytes of counts at fmem_LairMagic
+0x02  which entry of CombatTable sets up the guardian fight
+0x04  how many of them
+0x06  gold
+0x08  set to 1 the first time the guardian is beaten
+0x0a  x on the map, 0xffff once the lair is stripped bare
+0x0c  y
+0x0e  the landscape code
+0x10  the arena layout
```

**The keys.** The initialiser calls `NUM16` four times, which is `rnd & 7` rolled again
while it exceeds five, and writes 8, 4, 2 and 1 into byte `+0x14` of the chosen lair's
item record, stepping 0x6c (six records) on between each. One key per family, in a lair
chosen uniformly at the start of the quest, and `MOON:Valley` wants all four bits.

**The contents.** `LairFill` rolls 0..=100 against `MOON:LairRND`, four records of a
threshold and a kind: 50 gold, 70 magic, 90 both, 100 both again, because the
dispatcher's last comparison is dead code and falls through to the same call. Gold is
the wizard's own gift routine called with `dx` set, ten to thirty one; magic is his
bestowal called twice, so a lair with magic holds two items and nothing is ever empty.

**`MOON:LairFile` is recovered**, which the plan did not expect: it sits four bytes past
the end of the stale duplicate, and is 24 pointers to `fol1.t`..`fol6.t`, `wal1.t`..`wal6.t`,
`swl1.t`..`swl6.t`, `gll1.t`..`gll6.t`. That is the arena layout of each lair *and* the
order the keys are planted in, which is `moon::Key::ALL`.

**Arriving and leaving.** `CheckLairEncounter` overlaps the traveller's 8x10 token with
`MI.C` frame 0x1f, exactly the way a town is decided, and puts `Enter Lair` on the map's
list; `DisplayLairs` blits frame 0x14 at every lair whose x is not negative, so lairs are
on the map from the start; `CheckLairClear` writes 0xffff over the coordinates when the
gold is zero *and* all 24 item counts are, so a lair you have beaten but could not empty
is still there to go back to; and `LairWon` marks it and adds one to the knight's
experience the first time only.

**What is not recovered**: `ForestLairs` (guardian and count), `LairLocation` and
`LairType` are all inside the 2,906 bytes of DGROUP that the load image carries as a stale
duplicate. What a guardian *can* be is recovered, because `InitGameStart` fills
`CombatTable` with the thirteen `InitKnightvs*` routines. Where each lair stands is a
search over the real `MapType` grid for a cell whose whole neighbourhood is that family's
ground, clear of every other place and of the four starting corners, spread by
farthest-point sampling; which guardian each holds is the family's own road creatures for
the first three and then the beast and Balok, the two the road never produces. Both are
marked as ours in the baker.

- [x] Lair placement, entry, contents, the guardian fight
- [ ] `LairGEM`, which is the one thing a gem flight is for in the original: looking into
      a lair from the air, and being put back where you started on the way out. Here a
      gem flight ends on fire instead
- [ ] `_STATUS:DisplayLair`, the panel page that draws the floor as icons. Ours says it
      in words instead

## 4.4 Time and the moon `done`

`NEXTMOON`, `MOON`, `MOON_PIV`, `WEEK`, `LOADNEXTDAY`, `NEXTDAYPIC`, `FADEOUTDAY`, `JIFFY`,
and with addresses: `MOON:EncounterFini`, `AdjustTIME`, `Moons`, `MoonCount`,
`_MAP:NextWHICH`, `_LOADER:NextDayMes`, `NDM`, `WaitMES`, `WaitCOUNT`, `MoonPic`.

**The calendar is recovered whole.** `EncounterFini` is called by `_MAP:NextWHICH` once
every fourth turn, that is once each time all four knights have moved:

```
[0x898b] += 1                     ; days since the moon last moved
if [0x898b] <= 3: AdjustTIME      ; four days to a phase
[0x5b1] += 1
[0x898b] = 0
MoonCount = (MoonCount + 1) & 7   ; eight steps to the cycle
GiveBK()
[0x8989] = Moons[MoonCount]       ; tonight's moon, as a cel number
AdjustTIME()
```

So **four days move the moon one step and eight steps close the cycle**: thirty two days.
`InitGameStart` writes 0x2d before anything else runs, so a quest opens on the full moon.
`AdjustTIME` is the rest of a turned day: the wizard's grudge down by ten unless it is the
0xff of a knight he has never met, a day off a toad, and a quarter of whatever health is
missing back, never less than a point.

**The screen** is the routine at 0x8e5b: `NextDayMes` (`NDM`, `Next Day`) at y 95 over
`CH.PIV`, and cel `[0x8989]` of `KI.CEL` blitted at (119, 12). Cels 0x2d to 0x31 are five
57x44 moons, full to sliver, shadow on the right of each.

**What the moon gates**, all recovered: `SetRatmenTables` reads the phase before every
ratman fight and writes five hit points and a slash of one, or seven and three under 0x2d,
or twelve and five under 0x31; `CalcDamage` doubles a knight's blow while he carries the
moonstone whose night it is; `MOON:Henge` ends the game for a knight standing in the
circle with it. The first is live. The other two are built and cannot fire until something
hands out a moonstone, which is 7's.

**What is ours**: the eight bytes of `MOON:Moons`, which are in the stale part of DGROUP,
so the cycle is five pictures over eight steps waning and waxing back; and showing one of
the fourteen `_LOADER:WaitMES` hints on the between-days screen, which the original shows
while a disk loads and henge has no disk to load.

- [x] Moon phases on a weekly cycle, and the between-days screen
- [x] What the moon gates
- [ ] `FADEOUTDAY`, the fade between the map and the screen, which is item 75's

---

# 5. Locations `done`

Each is a named routine, and **all of them now run**. Two of the eleven modules,
`_TAVERN` and `_WIZARD`, turned out to be nothing but these: the dice table, the stone
circle, the wizard's bell, the town healer and the mystic. The temple is one routine in
`_STATUS`. What was in the plan as five `todo` lines was mostly translation.

**What the map offers, recovered.** `MOON:CheckGROOC` walks `MapIconsTABLE`, overlaps each
entry's `MI.C` icon rectangle with the traveller's 8x10 token, and pushes what it hits onto
a five-entry stack at DS:043c of `[x][y][kind]`. `_MAP:OrderOpt` turns a kind into a menu
line, through `MOON:StackMessages` for the fixed places:

```
0x15 0x16 0x17 0x18   Enter Village                one per knight, and only his own:
                                                   the entry is skipped unless the
                                                   knight's index matches
0x19                  Enter the city of Highwood
0x1a                  Enter the city of Waterdeep
0x1b                  Enter Stonehenge
0x1c 0x1d             Enter Valley of the Gods
0x1e 0x1f 0x20 0x22   Visit Math the Wizard
0x21                  Pillage knight's grave       another knight, dead
0x02                  Enter Lair                   not from the table; from CloseLair
0x01                  Battle with <name>           another knight, alive
```

The kind is the `MI.C` frame number, and the frames back it up: 0x15 to 0x1c and 0x1e are
the only big place-shaped sprites in the bank, 24x19 down to 7x20, while 0x21 is an 8x10
token like the knights' own. `MapIconsTABLE` has room for ten records of three words, which
is the nine fixed places above plus a terminator.

**Four villages, one per knight, is a mechanic henge does not have at all**, and neither is
the Valley of the Gods, the wizard, or pillaging a dead rival's grave.

| Original | What it is | Status |
|---|---|---|
| `TAVERN`, `TavernOpenScene`, `TavernLoop`, `LeaveTavern`, `SetBET` | the tavern | done: the five painted stakes, and an empty purse turned out at the door |
| `DICE`, `RollDice`, `DiceSort`, `DiceWinner`, `DiceODDS`, `DDICE`, `BET` | a dice game | done, and **recovered, not designed** |
| `HEALER`, `HealDon`, `ExitHealer`, `InitDonation` | restore health | done; the hermit costs days, the town healer takes a donation and spends it down |
| `TEMPLE`, `TTemple`, `SellToTemple`, `GoldSell` | temple services | done for selling; buying a moonstone waits on 7 |
| `MYSTIC`, `MysticUpDown`, `MysticAbility`, `MysticJudge`, `DonationTAB` | mystic services | done |
| `STONEHENGE`, `MOON:Henge`, `HengeControl`, `HengeWait` | the stone circle | done: an offering, and the winning branch waiting on a moonstone |
| `CONTROLWIZARD`, `WIZBESTOW`, `GETABILITY`, `BESTOWGOLD`, `BESTOWMAGIC`, `WIZBestowGold`, `WIZBestowMagic`, `WIZBestowAbility`, `MagicRND` | the wizard grants gold, magic, abilities | done, all four outcomes and the grudge |
| `LOADHIGHWOOD`, `LOADHW`, `LOADWATERDEEP`, `LOADWD`, `LOADCITY`, `LOADREGION` | town loading | done as scenes |

**The dice game, in full.** `RollDice` rolls three bytes with `and ax, 7; cmp ax, 5; jg`
and re-rolls, blits `DICE.CEL` cel `DDICE[n]` at (115, 15), (49, 38) and (75, 88) over
`DICE.PIV`, and then `DiceSort` bubble sorts the three and the sorted throw is compared
two words at a time against `DiceODDS` at DS:d125, eleven records of three faces and a
multiplier. The table is **not** monotone in the face: three of face 0 pays thirty, of
face 1 twenty, of face 5 eighteen, of face 3 sixteen, of face 4 fourteen and of face 2
twelve, and no pair pays at all unless it is a pair of face 0. The purse saturates at a
hundred and fifty (`cmp word ptr [si+0x32], 0x96`), so a big win can pay less than the
arithmetic says, here as there.

**The wizard, in full.** One roll plus the grudge byte at `+0x3b`, against thirty (magic),
seventy (an ability), ninety (ten to thirty one gold) and everything above that (a toad
for three days). A knight he has never met carries 0xff, which rolls one lower and cannot
reach the toad; leaving sets the grudge to seventy whatever he gave, and `AdjustTIME`
takes ten off it a day. `MagicRND` is ten records of a threshold and a slot, with two
refusals: never the Sword of Sharpness twice, because only one exists, and never the same
slot twice running. The fourteen `WizardText` lines are cycled by `WIZGOLD_CNT` and
`WIZMAG_CNT` and are used verbatim.

- [x] Tavern: what it actually offers
- [x] The dice game, and it was in `_TAVERN` all along
- [x] Temple and mystic services
- [x] The wizard: abilities, gold and magic bestowal
- [x] What the stone circle does, and its relationship to the moon
- [ ] The shake before the throw, `DD_ShakeDice` and `DD_ThrowDice`, two animation
      scripts of a hand over the table. The result is drawn; the roll is not animated
- [ ] `HengeControl` and `HengeLOOP`, the circle's own set piece: `ColourEn4Knight`,
      the thunder, and `Knight_LiftMagic`
- [ ] The mouse gadgets all six of these screens are really made of, which is item 53

---

# 6. Items, magic and economy `done`

Gold, prices, a carried pack, the ten magic items, casting, both curses, the hawk and the
gem. What is left here is the quest's own tokens, which are section 7's.

| Original | What it is | Status |
|---|---|---|
| `GEM`, `INITGEM`, `RESTOREGEM` | a gem item with state | done: a flight that returns you to where it began |
| `HAWK`, `INITHAWK`, `RESTOREHAWK`, `INITCURSEHAWK` | a hawk, and a cursed variant | done but for the cursed one: the hawk's flight lands where you put it |
| `HASTE` | a haste effect | done: `DistanceDONE` doubles the day's step budget, `NextWHICH` clears it |
| `PCURSED`, `ControlKnight`'s inverted joystick | player cursed state | done: a backfired scroll of protection, one bout, cleared at the end of `Combat` |
| `CAST_MAGIC`, `MagicCast`, `MagicName`, `MagicPrices` | casting | done, from the character sheet as the original does it from the status screen |
| `DRINKPOTIONHEAL` | potions | done, as an item virtue in the data |
| `BESTOWGOLD`, `BESTOWMAGIC`, `GETABILITY` | acquisition | done: off the fallen, out of a lair, and over the wizard's balcony |
| `TAKEFROMKNIGHT` | losing items | done: spent when used, taken by a cutpurse, and given to the druids |
| `AdjustLevel`, `XPlevels` | levelling | done: three `Increase` gadgets on the sheet, lit as `0xd3a7` lights them |

**The ten magic slots are the seam between the engine and the pack**, and they have to
agree: `_WIZARD:MagicRND` returns a slot, `magic_item` names the id, and the wizard's
gift, a lair's floor and the temple's counter all go through it. A pack that files one of
them under another id simply never has it handed out, so the baker's ids are the engine's
and a test in the baker asserts it.

- [x] Gold, and prices, so the merchant can open
- [x] Inventory, carrying and losing items
- [x] Potions, and the nine other magic slots, each with a virtue in the data
- [x] Abilities and character stats, and what makes them grow: experience against
      `XPlevels`, the mystic's donation, and the wizard's bestowal, all three capped at
      five by `CheckMaxAbility`
- [x] Swords and armour, as items with a price and a number: four blades worth 0, 2, 3 and
      5 damage and four suits worth 0, 10, 20 and 30 health, from `CalcDamage` and the
      derivation routine at 0x28d, at the prices the merchant's own lines carry, and both
      merchants sell them
- [x] Magic: spells, casting, costs
- [x] Curses: the backfired scroll, and the wizard's toad, which costs the three turns
      `NextWHICH` refuses it
- [x] The hawk and the gem
- [ ] Two of the ten are still inert, and honestly so: `TalismanWrym` shifts the dragon's
      fire right once per talisman and floors it at five, and the Scroll of the Wyrm sets
      `WyrmFLAG` so `KnightWyrm` can send the dragon after a rival. Both act on the
      dragon, and the dragon's set piece is 3.3
- [ ] `INITCURSEHAWK`, the hawk that drops you somewhere you did not choose
- [ ] The moonstones, which every counter in the game already knows the price of
      (`pu18`, `se18`) and nothing hands out. Section 7

---

# 7. The quest `design`

**This is the point of the game, and section 4.3 has just laid its first half.**

`GAMEOVER`, `GAMETABLE`, `TOTALS`, `PPOINT`, `PINDEX`, `FMEM_POINTS`, `FMEM_COLAREA`.

The symbols name the scoring and completion machinery but not the quest logic, so what is
left is mostly recoverable only by playing the original or by design.

- [ ] The four keys, one per lair. **Half done**: the lair initialiser plants one in each
      family's six, a raid hands it over, and `MOON:Valley` reads them back as the four
      bits of `+0x14`. What is missing is the Valley of the Gods itself
- [ ] The moonstone itself: where, what retrieving it requires. Everything that reads one
      is built: `MOON:Henge` ends the game for a knight in the circle with the stone of the
      night, `CalcDamage` doubles his blow while he carries it, and `Valley` hands one out
      for the four keys as `1 << (rnd & 3)`. Nothing calls `Valley`
- [ ] Win condition and ending
- [ ] Scoring and the final tally
- [ ] Losing: currently a run just ends

---

# 8. Presentation and shell `partly done`

| Original | What it is | Status |
|---|---|---|
| `LOADTITLE`, `DoOptions`, `Selection`, `Adjplayers`, `TitleMes` | title screen and its option list | done |
| `LOADCHOOSE`, `ChooseKnight`, `ChooseRefresh`, `FindChosen`, `ChooseFIRE`, `Chosen`, `choose_knight` | character select | done |
| `TypeName`, `NameDone`, `GNAME`, `BNAME`, `ENAME`, `RNAME` | typing your own name over the knight's | **todo** |
| `STATUSDISPLAY`, `V_STATUS`, `STATFLAG`, `DisplayKnight`, `SetUpStatus` | the status panel | done |
| `DisplayMagic`, `DisplayLair`, `DisplayDragon`, `DisplayMerchant`, `StatPlaceScroll` | the panel's other pages | **todo** |
| `MOVEPOINTER`, `SHOWPOINTER`, `POINTERBUFFER` | mouse pointer | **todo** |
| `ADDGADGET`, `CLEARGADGETS`, `CHECKGADGET`, `GADGET`, `AddIconGadget`, `HotGadget` | clickable UI widgets | **todo** |
| `LOADICONS`, `ICONBUFFER`, `ICONMEMORY` | UI icons | done for the panel's own |
| `LOADMESSAGE`, `WAITMESSAGE`, `OCCURMESSAGE`, `INSTRUCTMESSAGE`, `MESSAGE`, `MESSAGE_PIV` | message boxes | **todo** |

## 8.1 The title `done`

**The wordmark was in the font bank.** `BOLD.F` has 76 frames and the glyph map only ever
used 66 of them. Frames 73, 74 and 75 are not glyphs: they are the 305 by 54
`Moonstone / A Hard Days Knight` logo, the copyright line and `All rights reserved`. The
project had decoded that bank on its first day and never drawn the last three frames of it.

The option list is `DoOptions` and `Selection`. Four rows in `optmode` order: a player
count that left and right adjust between one and four (`Adjplayers`, which clamps rather
than wrapping), a gore switch (`GOREOPT`, thrown by left, right or fire alike through
`OSWITCHES`), `StartPractice` and `StartMoonQuest`. Up and down clamp at both ends. The
arrow is `SEL.CEL` frame 0 at `ARX` = 50.

What the original drew all this over is still unknown: `LOADTITLE` is a name, `TitleMes`
and its neighbours are text records, and the picture is in `INTR.EXE`, which has never been
examined. Ours goes over an intro plate, and **attract mode** cycles the other ten. Those
eleven screens had been sitting in the pack unused.

## 8.2 Character select `done`

`CH.PIV` is the backdrop and `SEL.CEL` the art: frame 0 an arrow, frame 1 a hollow border,
frames 2 to 5 the four knights, all at y 80 which is `ChooseRefresh`'s own coordinate. A
knight already taken is not drawn, which is `ChooseRefresh` only drawing the bits still set
in `choose_knight`; the highlight steps over the taken ones and stops at the ends rather
than wrapping, which is `ChooseLoop`; and after a choice it drops to the lowest still free,
which is `FindChosen`.

**The colours are recovered; the palette is not.** `CH.PIV` carries only sixteen colours
and the portraits index up to twenty-eight, so the top half comes from `SelectPAL`, whose
bytes did not survive into the unpacked image. `KnightGlowColours` did: three 12-bit shades
per knight, blue, gold, emerald and red in knight order, which is what the initials on
`BNAME`, `GNAME`, `ENAME` and `RNAME` stand for. The top sixteen palette entries are built
as four ramps from those, and each portrait is drawn through a substitution into its own.

**The four knights do not differ in stats.** `InitKnights` gives each one a name, a colour,
one corner of the map at (10, 10), (300, 5), (26, 180) or (300, 185), and the same stat
block as the other three. `henge` keeps the four as data so they *can* differ; what ships is what the original
had. The names are `Enemy1Name`..`Enemy4Name`, `SIR BANNER`, `SIR DWAIN`, `SIR BALAIN` and
`SIR GUNTHER`, which the original hands to its computer knights while a person types their
own over the top. Which name goes with which of the four is an assumption.

## 8.3 The status panel `done`

The knight record, from `DisplayKnight`, `SetKnightEquipment` and `_STATUS`:

```
+0x2e strength   +0x2f constitution  +0x30 endurance  +0x31 life points
+0x32 gold       +0x34 daggers       +0x36 experience
+0x38 health     +0x3c max health    +0x40 weapon     +0x42 armour
+0x44 magic      +0x4c name          +0x5c,+0x5e where on the map
```

and the arithmetic on it:

```
max health = 10 * constitution + armour + 10        (MOON, 0x28d)
stride     =  2 * endurance    + armour +  4        (MOON, 0x2e7)
damage     = swing + strength  + weapon             (CalcDamage, 0x2d67)
```

`SetKnightEquipment` opens every knight at one of each ability, five life points, ten
daggers, ten gold, a long sword and padded armour, so a new knight is worth twenty health
and not the ninety-nine the field is first written with. Chain mail, plate and battle
armour are worth ten, twenty and thirty health; a broad sword, claymore and sword of
sharpness two, three and five damage. Only chain mail and battle armour add to the stride,
which looks like an oversight and is kept because it is what the original does.

`KI.CEL` supplies the furniture at the cel numbers the fields index directly: frame 19 the
life figure, 21 the dagger, 22 to 25 the four swords, 27 to 30 the four suits of armour.
Frames 38 to 42 are five-pixel labels reading `STR :`, `CON :`, `END :`, `XP :` and
`GOLD :`, which is how the three ability rows and the right column are known; they are
anti-aliased across seven palette entries, so a silhouette of one is a smudge and the words
are set in type instead.

Everyone in an arena is a knight, so `henge` fights at the recovered scale and moves the
blow with it: twenty health and a five-point blow settles a bout in the same four swings a
hundred and twenty-five did.

- [x] Title screen and attract mode
- [x] Character select: the four knights, and what they are
- [x] A real status panel
- [ ] Typing your own name over the knight's, and the panel's other pages
- [ ] Mouse pointer and clickable widgets
- [ ] The message system: three distinct kinds (wait, occurrence, instruction)
- [ ] The intro sequence. `INTR.EXE` is a separate program, never examined
- [ ] Save and load `design`, the original has none

---

# 9. Assets not yet used

| Asset | Frames | Used |
|---|---|---|
| creature sprite banks | 8 sets | all 8, through the task VM |
| `MI.C` map icons | 47 | 1 of 47 |
| `KI.CEL` UI furniture | 50 | 11: the status panel's labels, pips, swords and armour |
| `SEL.CEL` selection art | 6 | all 6 |
| `DICE.CEL`, `DICE.PIV` | dice game | none |
| `BLO.CEL` blood and gore | 41 | none |
| intro cast banks | 438 | none |
| full-screen scenes | 31 | 25: the eleven intro plates now carry the title and attract mode |
| sound samples | 49 | 4 |
| music | 18 files | none |

---

# 10. Ordering

Dependency order, not preference.

**First, because everything waits on it**
1. ~~Symbol map (1.2)~~ **done**, 2,223 symbols with addresses
2. Animation task VM (1.1), decoded; what is left is writing the interpreter

**Then, in parallel**
3. ~~Creatures (3.2)~~ **done** on the standard states; their behaviour is next
4. ~~Gold and inventory (6)~~ **done**, which opened the merchant and leaves the
   tavern needing only what it offers
5. ~~Title, character select and the status panel (8.1 to 8.3)~~ **done**, which
   leaves the shell needing only the pointer, the gadgets and the message boxes

5b. ~~Lairs (4.3), the moon (4.4) and every door in section 5~~ **done**, which put the
   quest's four keys on the board and left the moonstones as the only missing token

**Then**
6. The quest (7), which is now one step: the Valley of the Gods, and what it gives
7. ~~Attack variety, blocking, gore (3.1)~~ **done**
8. Per-creature behaviour (3.2), which is the last thing standing between the bestiary
   and fighting like itself
9. The dragon (3.3) and the demon as set pieces

**Whenever**
- Music (2.5), gamepads (2.4), colour cycling and fades (2.2), scrolling

**Honest note on effort.** Steps 1 and 2 are research: they could take a day or a month,
and no amount of planning makes that predictable. Everything after them is construction and
estimable. The quest is neither, because it was never recovered, so it has to be designed
and playtested rather than ported.
