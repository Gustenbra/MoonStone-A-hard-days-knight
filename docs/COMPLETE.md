# Everything left to build

An exhaustive plan for a complete Rust reimplementation.

This is built around the **334 symbols left in the original executable's debug info**, which
is the closest thing to a definitive list of what the game actually does. Every symbol
below is a real function or variable from the original build. Anything not covered by a
symbol is marked as such, so you can tell recovered fact from design decision.

**Status key**

| | |
|---|---|
| **done** | built and verified in henge |
| **partial** | exists but incomplete or approximated |
| **todo** | not started, but understood |
| **blocked** | cannot start until something else is decoded |
| **design** | never recovered; has to be invented rather than ported |

Honest headline: **roughly a quarter done**, and the remaining three quarters includes the
quest, the bestiary and every economy.

---

# 1. Blockers

Two things gate large parts of everything else. Nothing in sections 4, 6 or 7 can finish
until these do.

## 1.1 The animation task VM `blocked`

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

Read as a design, that is: a per-actor coroutine with a program counter, a call stack
(`GOSUB`), conditional branches (`TESTEQ`/`TESTNE`), loops, frame timing (`TIME`, `HOLD`),
movement (`MOVE`, `LEFT`, `RIGHT`), sprite placement (`PLACE`), mirroring (`FLIP`), depth
sorting (`SORT`), shadows, sound triggers, spawning and killing other tasks, and collision
hooks. It is a small language, and the whole game's feel lives in it.

**Steps**

- [ ] Locate the interpreter in the unpacked image. `_TASK.ASM`'s code range is already
      known from the module table, which narrows the search enormously
- [ ] Recover the opcode set: one byte or word per operation, with operand widths
- [ ] Recover the per-frame part record. Data is around `0x0e000`; the shape looks like a
      header, six-byte records, terminator, but the field meanings are wrong somewhere.
      **Test by compositing and looking**: if it does not produce a coherent figure, it is
      not decoded, regardless of how plausible the table looks
- [ ] Write the VM in `henge-core` as a deterministic interpreter, integers only
- [ ] Export every actor's scripts to data, so ours can be authored the same way
- [ ] Replace the hand-authored knight sequences with the recovered ones

**Unlocks**: all eight creatures, correct combat timing, shadows, gore, correct sorting.

## 1.2 Symbol name to address mapping `blocked`

The debug info holds 334 names and a decoded module table, but the name-to-address map is
not recovered. With it, every function below can be found at an exact offset and read.

- [ ] Reverse the debug info's symbol record format (Borland style; line table and module
      table already decoded)
- [ ] Emit a symbol map, and feed it to a disassembler

**Unlocks**: makes almost everything below cheaper, 1.1 most of all.

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
| `SCROLL`, `PAN`, `SETSCREENOFFSET`, `AROFFSET` | scrolling the map | **todo** |
| `BORD`, `BORDERS`, `SETDEMONBORD` | screen border, and a special one for the demon | **todo** |
| `CLS`, `VBI`, `WAITVSYNC`, `WAITVBS` | clear, vblank sync | done via the frame loop |
| `CONVERTSCREEN` | planar to linear conversion | done at bake time |

- [ ] Palette fades in and out, as scene transitions
- [ ] Colour cycling. Named entries animate on a timer; find which indices cycle per scene
- [ ] Map scrolling. The map is larger than the screen in the original, or pans; confirm
- [ ] Screen borders

## 2.3 Text `done`

`TEXTPRINT`, `TEXT`, `TEXTCONVERT`, `FONTBUFFER`. Glyph order read off the artwork.

- [ ] Recover the real lookup table once 1.2 lands, and check ours against it
- [ ] The three unidentified ornament glyphs at the end of each font bank

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
| `TASKSOUND` | sound triggered from an animation frame | blocked on 1.1 |
| `LOADMUSIC`, `MUSIC`, `MUSICTYPE` | music playback | **blocked** |
| `ADDCLICKSND` | UI click | todo |

- [ ] Wire the remaining 45 samples to events
- [ ] Sounds carried on animation frames rather than inferred from state changes
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

**Every one is blocked on 1.1.** Their sprite banks are body parts, not whole poses.

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
| `MAPLOCATE`, `POX`, `POY` | position | done |
| `CHECKENCOUNTERS`, `ENCOUNTERAREA` | ambushes | ours, not theirs |
| `CHECKY`, `CHECKY2` | movement validity | partial |
| `FINDWHICH`, `WHICH`, `DISTAN`, `MAX_DISTAN` | proximity | partial |
| `LANDSCAPE`, `LANDTYPE`, `LANDFILE` | terrain type | ours, by colour |
| `SCROLL`, `PAN` | scrolling | todo |

**Our terrain classification reads pixel colours off the map image. The original uses a
table.** Ours works; it is not theirs.

- [ ] Recover the real terrain table and location graph
- [ ] `CHECKY`/`CHECKY2`: what makes ground impassable

## 4.2 Arena generation `partial`

`GENERATELANDSCAPE`, `GENERATEFOREST`, `GENERATESWAMP`, `GENERATEWASTE`, `GENERATEMOORES`,
plus counts `FORESTCOUNT`, `SWAMPCOUNT`, `WASTECOUNT`, `PLAINCOUNT`, and four tables of six:

```
F1 F2 F3 F4 F5 F6      forest
W1 W2 W3 W4 W5 W6      waste
S1 S2 S3 S4 S5 S6      swamp
P1 P2 P3 P4 P5 P6      plain / moors
```

We load 57 `.T` arenas and pick one at random per family. The original selects through
these tables, and `GENERATEMOORES` implies a **moors/plain family we do not render at all**.

- [ ] Recover the selection tables
- [ ] The moors family: find its sheets and backdrop

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

| Original | What it is | Status |
|---|---|---|
| `TAVERN` | recruit, rumours, drink | menu only |
| `DICE` | a dice game | **todo**, `DICE.CEL` and `DICE.PIV` unused |
| `HEALER` | restore health | done, costs days |
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

# 6. Items, magic and economy `todo`

Almost nothing here exists.

| Original | What it is |
|---|---|
| `GEM`, `INITGEM`, `RESTOREGEM` | a gem item with state |
| `HAWK`, `INITHAWK`, `RESTOREHAWK`, `INITCURSEHAWK` | a hawk, and a cursed variant |
| `HASTE` | a haste effect |
| `PCURSED` | player cursed state |
| `CAST_MAGIC` | casting |
| `DRINKPOTIONHEAL` | potions |
| `BESTOWGOLD`, `BESTOWMAGIC`, `GETABILITY` | acquisition |
| `TAKEFROMKNIGHT` | losing items |

- [ ] Gold, and prices, so the merchant can open
- [ ] Inventory, carrying and losing items
- [ ] Potions
- [ ] Magic: spells, casting, costs
- [ ] Curses
- [ ] Abilities and character stats
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

# 8. Presentation and shell `todo`

| Original | What it is | Status |
|---|---|---|
| `LOADTITLE` | title screen | **todo** |
| `LOADCHOOSE` | character select | **todo** |
| `STATUSDISPLAY`, `V_STATUS`, `STATFLAG`, `_STATUS.ASM` | the status panel | partial |
| `MOVEPOINTER`, `SHOWPOINTER`, `POINTERBUFFER` | mouse pointer | **todo** |
| `ADDGADGET`, `CLEARGADGETS`, `CHECKGADGET`, `GADGET` | clickable UI widgets | **todo** |
| `LOADICONS`, `ICONBUFFER`, `ICONMEMORY` | UI icons | partial |
| `LOADMESSAGE`, `WAITMESSAGE`, `OCCURMESSAGE`, `INSTRUCTMESSAGE`, `MESSAGE`, `MESSAGE_PIV` | message boxes | **todo** |

- [ ] Title screen and attract mode
- [ ] Character select: the four knights, and their differing stats
- [ ] A real status panel
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
| `KI.CEL` UI furniture | 50 | none |
| `SEL.CEL` selection art | 6 | none |
| `DICE.CEL`, `DICE.PIV` | dice game | none |
| `BLO.CEL` blood and gore | 41 | none |
| intro cast banks | 438 | none |
| full-screen scenes | 31 | 13 |
| sound samples | 49 | 4 |
| music | 18 files | none |

---

# 10. Ordering

Dependency order, not preference.

**First, because everything waits on it**
1. Symbol map (1.2)
2. Animation task VM (1.1)

**Then, in parallel**
3. Creatures (3.2), which 1.1 unlocks all at once
4. Gold and inventory (6), which opens the merchant and tavern
5. Title and character select (8)

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
