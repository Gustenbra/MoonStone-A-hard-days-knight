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

Honest headline: **about a third done**, and what remains includes the quest, the bestiary
and every economy.

---

# 1. Blockers

Two things gated large parts of everything else. Both are now research-complete: what is
left of 1.1 is construction, not investigation.

## 1.1 The animation task VM `partial`

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

The scripts themselves are named data in DGROUP: 221 of them, from `Knight_SwWalkR1` to
`Balok_Blink`, all of which parse cleanly.

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
- [ ] Write the VM in `henge-core` as a deterministic interpreter, integers only
- [ ] Export every actor's scripts to data, so ours can be authored the same way
- [ ] Replace the hand-authored knight sequences with the recovered ones

**Unlocks**: all eight creatures, correct combat timing, shadows, gore, correct sorting.

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

## 3.1 Core `partial`

| Original | What it is | Status |
|---|---|---|
| `CALCHIT` | resolve a strike | done, positional hit lines |
| `CALCMOVE` | movement and bounds | done |
| `CLEARCOLLISIONS`, `TASKCOLLISION`, `TASKWALKCOLLIDE` | collision | partial |
| `SWORDFLAG` | weapon state | todo |
| `BLOW` | a landed blow | done as `HitEvent` |
| `GORESWITCH`, `BLOODBUFFER` | dismemberment and blood | **todo** |
| `ALLDEAD` | everyone down | done as `Bout::settled` |
| `KNIGHTREFRESH`, `KNIGHTLOC`, `KNIGHTBUFFER`, `ENEMYBUFFER` | actor state | done as `Fighter` |
| `NUM_PLAYERS`, `PLAYER1`-`PLAYER4`, `PLAYERPOINTER` | up to four players | done |
| `GAME_XP` | experience | **todo** |

- [ ] **Attack variety.** We have one attack. The original has several by direction and
      button. Recover from the animation scripts once 1.1 lands
- [ ] **Blocking and parrying.** No defence exists at all
- [ ] Gore and dismemberment. The sprite banks are full of severed parts, currently unused
- [ ] Experience and levelling
- [ ] Weapon state: drawn, sheathed, dropped, thrown

## 3.2 Creatures `blocked`

Loaders exist for each: `LOADKNIGHT`, `LOADHEAD`, `LOADTROLL`, `LOADTROGGAXE`,
`LOADTROGGSPEAR`, `LOADRATMEN`, `LOADMUDMEN`, `LOADDEMON`, `LOADDRAGON`, `LOADBALOK`,
`LOADBEAST`.

Their sprite banks are body parts, not whole poses, so every one needs 1.1. The scripts
and the bank table for each are recovered (`TASKVM.md`); what is left is 1.1's remaining
construction work, the interpreter itself.

- [ ] Troll, trogg with axe, trogg with spear, ratmen, mudmen, demon, dragon, Balok, beast
- [ ] Per-creature behaviour. `design`: no AI logic is named in the symbols beyond the
      collision hooks, so their behaviour has to be observed or invented
- [ ] `SETDEMONBORD`: the demon changes the screen border

## 3.3 The dragon `blocked`

`BATTLEDRAGON`, `DRAGON`, `DRAGONOVER`, `DRAGONFLAG`, `DRAGONDEADFLAG`, `LOADDRAGON`.

A distinct set-piece encounter with its own state machine, not an ordinary bout.

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

- [x] The real terrain table
- [x] `CHECKY`/`CHECKY2`: what makes ground impassable
- [x] Scrolling: settled, and negative
- [ ] The rest of the location graph, which needs `MapIconsTABLE` reconstructed some
      other way, or accepting the places where the artwork puts them

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

## 4.3 Lairs `todo`

`LAIRSTART`, `FINDLAIR`, `FINDCLOSELAIR`, `LAIRTABLE`, `LAIRPOINTER`, `FMEM_LAIR`.

Lairs are fixed dangerous places holding the keys. **None of this exists in henge.**

- [ ] Lair placement, entry, contents, the guardian fight

## 4.4 Time and the moon `partial`

`NEXTMOON`, `MOON`, `MOON_PIV`, `WEEK`, `LOADNEXTDAY`, `NEXTDAYPIC`, `FADEOUTDAY`, `JIFFY`.

We have a day counter. **The moon phase, which the game is named for, does not exist.**

- [ ] Moon phases on a weekly cycle, and the between-days screen
- [ ] What the moon gates. `design` unless recovered

---

# 5. Locations

Each is a named routine. All are **todo** beyond the menus already drawn.

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
| `TAVERN` | recruit, rumours, drink | menu only, but gold now exists to charge |
| `DICE` | a dice game | **todo**, `DICE.CEL` and `DICE.PIV` unused |
| `HEALER` | restore health | done; the hermit costs days, a town healer days and gold |
| `TEMPLE` | temple services | **todo** |
| `MYSTIC` | mystic services | **todo** |
| `STONEHENGE` | the stone circle | menu only |
| `CONTROLWIZARD`, `WIZBESTOW`, `GETABILITY`, `BESTOWGOLD`, `BESTOWMAGIC` | the wizard grants gold, magic, abilities | **todo** |
| `LOADHIGHWOOD`, `LOADHW`, `LOADWATERDEEP`, `LOADWD`, `LOADCITY`, `LOADREGION` | town loading | done as scenes |

- [ ] Tavern: what it actually offers
- [ ] The dice game, rules unknown `design`
- [ ] Temple and mystic services
- [ ] The wizard: abilities, gold and magic bestowal. `GETABILITY` implies a character
      ability system that does not exist in henge at all
- [ ] What the stone circle does, and its relationship to the moon

---

# 6. Items, magic and economy `partly done`

Gold, prices, a carried pack and potions exist. Magic, curses, the hawk and the gem
do not.

| Original | What it is | Status |
|---|---|---|
| `GEM`, `INITGEM`, `RESTOREGEM` | a gem item with state | **todo** |
| `HAWK`, `INITHAWK`, `RESTOREHAWK`, `INITCURSEHAWK` | a hawk, and a cursed variant | **todo** |
| `HASTE` | a haste effect | **todo** |
| `PCURSED` | player cursed state | **todo** |
| `CAST_MAGIC` | casting | **todo** |
| `DRINKPOTIONHEAL` | potions | done, as an item virtue in the data |
| `BESTOWGOLD`, `BESTOWMAGIC`, `GETABILITY` | acquisition | gold done, as a bounty off the fallen; the wizard's bestowal **todo** |
| `TAKEFROMKNIGHT` | losing items | done: a flask is spent when drunk, and a cutpurse on the road takes coin or goods |

- [x] Gold, and prices, so the merchant can open
- [x] Inventory, carrying and losing items
- [x] Potions, in as much as healing flasks exist. A potion that does anything
      other than mend waits on magic
- [x] Abilities and character stats, in as much as a knight has strength, constitution and
      endurance, they are on the sheet, and the original's own arithmetic turns them into
      health, reach and damage. **What makes them grow does not exist**: `AdjustLevel`
      spends experience on a random one of the three at a threshold `XPlevels` sets per
      player count, and that is build order item 45
- [x] Swords and armour, as items with a price and a number: four blades worth 0, 2, 3 and
      5 damage and four suits worth 0, 10, 20 and 30 health, from `CalcDamage` and the
      derivation routine at 0x28d, at the prices the merchant's own lines carry. Nothing
      sells them yet
- [ ] Magic: spells, casting, costs
- [ ] Curses
- [ ] The hawk and the gem, whatever they turn out to do

---

# 7. The quest `design`

**This is the point of the game and none of it exists.**

`GAMEOVER`, `GAMETABLE`, `TOTALS`, `PPOINT`, `PINDEX`, `FMEM_POINTS`, `FMEM_COLAREA`.

The symbols name the scoring and completion machinery but not the quest logic, so this is
mostly recoverable only by playing the original or by design.

- [ ] The four keys, one per lair
- [ ] The moonstone itself: where, what retrieving it requires
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
| creature sprite banks | 8 sets | **none** |
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
3. Creatures (3.2), which 1.1 unlocks all at once
4. ~~Gold and inventory (6)~~ **done**, which opened the merchant and leaves the
   tavern needing only what it offers
5. ~~Title, character select and the status panel (8.1 to 8.3)~~ **done**, which
   leaves the shell needing only the pointer, the gadgets and the message boxes

**Then**
6. Lairs (4.3) and the moon (4.4)
7. Attack variety, blocking, gore (3.1)
8. The quest (7)
9. The dragon (3.3) and Balok as set pieces

**Whenever**
- Music (2.5), gamepads (2.4), colour cycling and fades (2.2), scrolling

**Honest note on effort.** Steps 1 and 2 are research: they could take a day or a month,
and no amount of planning makes that predictable. Everything after them is construction and
estimable. The quest is neither, because it was never recovered, so it has to be designed
and playtested rather than ported.
