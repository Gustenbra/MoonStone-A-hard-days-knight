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

Honest headline: **most of it done**, and what remains is the shell's message system
and mouse. The creatures' own behaviour and both set pieces came off this list when
`ControlTrogg` through `ControlDragon` turned out to read as cleanly as
`ControlKnight` had. The quest came off this
list when `MOON:Valley`, `Henge`, `KnightWonGame` and `WhoLived` turned out to be the
whole of it; that is the third phase running that was written down as design and read
back as translation.

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
      effects (`Effect::Gosub` names the routine and its kind out of a table of all 41;
      `Effect::Sound` carries the sound id the handler would hand `PLAY_SFX`); none is
      faked and none is skipped silently, and the 23 sound routines are transcribed in
      `henge-core/src/sound.rs`.
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
| `FADEPALETTEIN`, `FADEPALETTEOUT`, `FADEOUTDAY` | palette fades | done, recovered: sixteen linear steps, one a frame |
| `ADDCOL`, `COLCON`, `DYNAMIC`, `COLOURCYCLE`, `COLOURGLOW`, `_installcycle`, `_installglow`, `CYCLES`, `GLOWS`, `PALLOC` | animated palette entries | done, recovered: `henge_assets::palette` |
| `COLOURENKNIGHT`, `ColourBackDrop`, `ColourKnight`, `Colour2ndKnight`, `ColourBeast`..`ColourDragon`, `ColourBackdrop`, `BattlePal` | the fight palette: knight, second knight, creature and ground colours written into the backdrop's palette | done, recovered: `henge_core::battle_palette`, below |
| `SCROLL`, `PAN`, `SETSCREENOFFSET`, `AROFFSET` | scrolling the map | **settled: the overworld map does not scroll** |
| `BORD`, `BORDERS`, `SETDEMONBORD` | `bord` (0x5828) is the VGA overscan colour, attribute register 0x11, and nothing to do with movement. `SETDEMONBORD` writes a movement border; `SBORD` (0x4552) and `CheckBorder` (0x40d0) are the two routines that read them | **done**, item 60 and `henge_core::arena` |
| `CLS`, `VBI`, `WAITVSYNC`, `WAITVBS` | clear, vblank sync | done, recovered: the wait is at image `0x5a24` and it is the engine's tick, 70.0863 Hz |
| `CONVERTSCREEN` | planar to linear conversion | done at bake time |

- [x] **Palette fades in and out, as scene transitions.** Both are sixteen steps at one a
      frame and linear. `FADEPALETTEIN` builds a step of `target * 16` per DAC byte and
      accumulates it into a 16.8 fixed-point channel, showing the high byte, so step *k*
      of sixteen shows *k* sixteenths; `FADEPALETTEOUT` reads the DAC back through port
      0x3c7 and subtracts its way to black. Every screen change fades in here, and the
      two screens that are dismissed rather than walked away from fade out: the
      between-days screen, which is `FADEOUTDAY` and which `NextWHICH` reaches through
      `WaitFIRE`, and a message chain
- [x] **Colour cycling, and which indices cycle per scene.** `ADDCOL` queues `COLCON` on
      the frame list and points `PALLOC` at the live palette, which is 32 twelve-bit
      words. `COLCON` walks six `CYCLES` slots of six bytes (first, last, direction,
      period, counter) and six `GLOWS` slots of twelve (index, target, period, counter,
      the colour it came from, repeat count), and reloads the DAC only if either moved.
      A cycle rotates its span by one every period frames, one way with `LOOPY` and the
      other with `POS`. A glow walks one index towards a target one step a channel with
      `DYNAMIC` and swaps target with origin on arrival, so it breathes.
      **The game installs exactly two of them from a screen**: `MapEffects` gives the
      overworld a glow on entry 31 towards `0x0ff` every frame and a cycle over entries
      21 to 23 every twelfth frame, whose handle it calls `RiverHANDLE`, so those three
      are the water; and `ChooseKnight` gives the select screen a glow on entry 15
      towards `0x088`, which on that screen is the highlight frame round the chosen
      knight and nothing else, because the screen is cleared to entry 0 and cel 1 of
      `SEL.CEL` is the only thing on it drawn in 15. A third, `MudmenGlowOn`, is `COLOURGLOW(0x0e, 0x100, 2, 0)` and
      hangs off `InitCombat` rather than off a screen. All three are built
- [x] **The fight palette, `BattlePal`, and it is how the knights get their colours.**
      The original does not recolour a knight by substituting pixels, and henge no longer
      does either. Every fighter is painted against fixed indices and the bout writes
      colours into them. `ColourBackDrop` (image 0x460f) copies the backdrop picture's
      thirty two words from `DS:0x80bb`, where the picture loader (0x875e) left them, into
      `BattlePal` (`DS:0x7a80`), then dispatches on the code each `InitKnightvs*` passes
      in `ax` with `si` at entry 9: beast 0, mudmen 2, demon 4, second knight 6, dragon
      0xa, trogg with axe or hammer 0xc, trogg with spear 0x10, ratmen 0x12, balok 0x18,
      troll 0x20. The creature routines write their block from 9: seven words for most,
      six for the troll (0x4762), twenty three for the demon (0x4781, copied from
      `BlueDemon` at `DS:0x7992`), and the dragon (0x47b1) adds `fc0 f80 c50` at 29 to
      31; the trogg (0x46c0) is the one that looks at the landscape code, with one block
      on the wastes, one on the moors and one everywhere else. A second knight
      (`Colour2ndKnight`, 0x4691) gets `ColourKnight` at 9 to 11, and is drawn from
      `HE1.OB`..`HE3.OB`, the knight painted in 9 to 11 instead of 6 to 8, which
      `InitKnightvsKnight`'s loader (0x89cd) puts in the creature table. Then
      `ColourMainKnight` (0x47e4): `ColourKnight` (0x480e) at entries 6 to 8, `00a 007
      004` blue, `f80 c50 a30` gold, `8c6 593 251` emerald, `f22 b22 700` red, `206 103
      001` on the unguarded last branch that the computer's knights take; `ColourBackdrop`
      (0x4879), which the demon skips, and which writes `PlainsCOLOUR`, `ForestCOLOUR`,
      `SwampCOLOUR` or `WasteCOLOUR` (thirteen words at `DS:0x78d6`, `0x78f0`, `0x7924`,
      `0x790a`) to 16 to 28, and `ffd 998 776 443` to 1 to 4 on the swamp and the wastes;
      entry 0 black; entry 15 `c00` unless the dragon's. Every backdrop in the release was
      saved with its own ground table already at 16 to 28, so those writes change nothing
      on the original's pictures, and the baker checks that they agree. All of it is
      `data/battle-palette.json`, applied by `henge_core::battle_palette::compose` in
      that order, and the knight is blitted in his own indices with no table between
- [x] **`KnightGlowOn` and `KnightGlowColours`**, which flash a knight down to ten health
      in his own colour on entries 6, 7 and 8, and 9 to 11 for a second knight. Wired
      now that those entries are the knight: the combat loop calls it once a frame
      (0x369), and with `KGT` clear and the main knight's health at ten or less it
      installs `COLOURGLOW(6, glow[0], 2, 0)`, `COLOURGLOW(7, glow[1], 1, 0)` and
      `COLOURGLOW(8, glow[2], 1, 0)`, then the same on 9 to 11 every frame for a second
      knight when `COLOURS` says there is one. The glow triples are `00c 009 006`, `fa0
      e70 c50`, `ae8 6b5 473`, `d00 900 500` and `408 305 003`. `KnightGlowOff` writes
      zero into the six handles at the end of the bout. **Which also answers 8.2's old
      open question**: the knight palette is recovered after all
- [x] Map scrolling: confirmed, and there is none. `MAP.CMP` is one 320x200 picture,
      `_MAP:SHOW` passes the token's position straight to the blitter with nothing
      subtracted, and `_MAP:HawkBorders` bounds that position to `0..=310` by `0..=190`,
      which is one screen. `SCROLL`, `PAN`, `SETSCREENOFFSET` and `AROFFSET` carry no
      addresses, so where they are used is not established; `_MAP:ScrollINPUT` reads the
      keyboard despite its name, and `SCROLLX` in `_STATUS` is the status panel's own
      icon cursor
- [x] **Borders, and there is no screen border.** `bord` (image 0x5828) writes attribute
      controller register 0x11, the overscan colour, and is the only thing in the set that
      touches the display. The other two are movement: `CheckBorder` (0x40d0) holds an
      actor inside columns 10 to 320 and depths 30 to 155, and `SBORD` (0x4552) walks the
      arena's own list of impassable rectangles. Both work by clearing bits in a per-actor
      byte of allowed directions rather than by clamping. Item 60

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
| `JOY0`, `JOY1`, `POLL_JOY0`, `BOUNCEBUTTON`, `Fix_JoyStick`, `GetJoyTL`, `GetJoyBR`, `AdjustJoy`, `GetInputDevice`, `JOY_XMIN/XMAX/YMIN/YMAX` | done, recovered: `henge-desktop/src/input.rs` |

- [x] **Gamepad support.** The word is five bits, `0x01` right through `0x10` fire, and
      `JOY0` and `JOY1` build it from the gameport: the one-shots are fired with
      `out 0x201, 0xff`, counted to a cap of `0x400`, and compared against the four
      calibration thresholds. An axis at the cap is a stick that is not there. The
      button is `al & (al >> 1)` over that stick's pair of bits, so either fires.
      `Fix_JoyStick` calibrates in two prompts whose words are recovered verbatim, and
      `AdjustJoy` pulls each threshold an eighth of the measured range inwards.
      `BOUNCEBUTTON` is the debounce and `GetInputDevice` cancels opposite directions.
      Pads come from `gilrs`; on Linux that wants `libudev-dev` at build time
- [x] **The four thresholds the image ships, and the menu route to the calibration.**
      `JOY0` compares against four words at `DS:0x817e`, `0x8180`, `0x8182`, `0x8184`, and
      the image has them initialised: image `0x1a52e` onwards reads **16, 16, 80, 80**.
      That is what the game answers with before anybody calibrates, and henge's
      `Calibration::default` is now those four constants instead of a pair of invented
      corners. They are also `AdjustJoy` of 6 and 90 exactly, which both checks the
      reading and fixes the scale a modern axis is put on: -1 maps to 6 and +1 to 90, so
      calibrating a pad that reports clean extremes gives back the shipped pair. The only
      part left inherent is that a gameport count is a busy-loop count and a pad reports a
      fraction, so *something* has to map one onto the other.
      **And the route in is a menu key:** `OptionKeys` at `0x1282` tests scancode 0x24,
      `J`, before anything else and jumps to `Fix_JoyStick` at `0x128a`, which ends at
      `0x59d7` with a jump back into the options screen. So `J` on the title screen
      calibrates, and F11 is gone. Escape likewise: `OptionKeys` at `0x128d` and
      `StartAgain` at `0x00ba` both test scancode 0x01 and return, so Escape quits from
      the title screen and no longer from everywhere
- [x] **Rebindable controls** `design`, **shipping with the original's keys**. The
      rebinding is ours; the keys are not. The reader at image `0x81ec` is two runs of
      five `KEYPRESSED` calls with an `rcl` after each, so the scancode order is the bit
      order: `0x1c` Enter, `0x48`, `0x50`, `0x4b`, `0x4d` for player one, and `0x0f` Tab,
      `0x11` W, `0x2d` X, `0x1e` A, `0x20` D for player two. Those ten are now
      `Bindings::default`, the arrows-and-space layout that used to ship is gone and so is
      `--original-keys`, which had nothing left to mean. The table is still a table of
      actions to sources serialising to `henge-controls.json`, with `--bind` and
      `--controls-write`. A key the table claims is a control is no longer also a
      developer key, which is why the arena flip moved from Tab to `F2`

## 2.5 Audio `partial`

| Original | What it is | Status |
|---|---|---|
| `LOADSAMPLES`, `SETUPSAMPLES`, `PLAYSAMPLE`, `PLAY_SFX`, `STOP_SAMP`, `SFX`, `SFXTYPE` | sound effects | done, recovered: `PLAY_SFX` at `0x5964` and both of its translate tables |
| `TASKSOUND` | sound triggered from an animation frame | done, recovered: opcode 0x92, handler `0x9b38`, 131 calls, 54 ids, 31 samples, each at the frame its script names |
| `KnightGruntSound`, `AddTrollSND`, `BalokRoarSound` and 20 more | the sound routines the scripts call | done, recovered: all 23 disassembled, three of them silent in the shipped image |
| `LOADMUSIC`, `MusicTable`, `MUSICTYPE`, `int 60h` | music playback | done, recovered: all six tunes |
| `ADDCLICKSND` | UI click | done, recovered: sample 0x0f, `samptab`, `hit3` |

- [x] **`ADDCLICKSND`, and which sample it is.** `_STATUS:AddClickSound` at `0xd508` is
      `mov al, 0x0f` into the sample dispatcher at `0x5964`, and seventeen gadget handlers
      call it: `HotGadget` at `0xca11` and `0xca23`, then `HGCastMagic`, `MagicCast`,
      `HGTakeMagic`, `HGAbility`, `TKAR`, `TKWP`, `TKGP`, `BuyArmour` (three), `BuyWeapon`
      (two), `BuyDagger`, `TTemple` and `SellToTemple`. So it is the sound a gadget makes
      when its action is taken, on the status screen, the shops and the temple. The
      dispatcher translates the number through `samptab` at `DS:0x7e6e` (image `0x1a21e`)
      for a digital card and through `Soundfxtable` at `DS:0x7d02` for the others; entry
      0x0f of `samptab` is 0x15, and the forty-nine-name table at `DS:0x8783` (image
      `0x1ab33`, reached through `SBFileTable` at `DS:0x8651`) has `hit3` twenty-second.
      The same chain at 0x0b gives `swish`, which is what this project already played for a
      swing, so the reading checks out against something chosen independently of it. (0x0b
      is not `KnightGruntSound`'s, as this entry used to say: that routine's ids are 4 to 8
      and it plays none of them. 0x0b is the `TASKSOUND` on the second frame of
      `Knight_SwSwing`.) The click itself is still not wired to anything
- [x] **Sounds are carried on the animation frames, and nothing is inferred.** The cue
      layer that watched the fight is gone: `henge-audio/src/cue.rs` announced a swing when
      a fighter's state became `Attack`, a footfall on frames 0 and 4 of a walk, and a hit
      or a death off the bout's `HitEvent`s, choosing one of four samples for each. The
      original does none of that. A scan of the image for every `call`/`jmp` that resolves
      to `PLAY_SFX` (`0x5964`) finds **exactly 26 sites**: the `TASKSOUND` handler at
      `0x9b38`, the 23 sound routines the scripts call through `TASKGOSUB`, `ShakeDiceSnd`
      at `0xb338` and `AddClickSound` at `0xd50a`. There is no footstep routine and no
      swing routine; the knight's walk scripts carry no sound command at all, and his
      swish is `TASKSOUND 0x0b` on the second frame of `Knight_SwSwing`. The VM now emits
      the id, `Bout::step_with` collects it as `bout.sounds`, and the desktop plays it:
      `--sounds` prints which sample fires on which tick and which script asked
- [x] **An id is not a sample number.** `PLAY_SFX` `xlatb`s the byte through one of two
      364-byte tables, DS:`0x7e6e` (image `0x1a21e`) for a sampled card and
      DS:`0x7d02` (`Soundfxtable`, image `0x1a0b2`) for an FM one, and hands the result to
      `PLAYSAMPLE` (`0x932a`) or to `int 0x61`. Nothing gates either path: no distance, no
      channel limit, no priority, no check for the same sound already playing. Entries
      `0x00` to `0x6c` are meant and the rest of each table is `0x15` filler, which five of
      the ids the scripts use fall into and play `hit3`
- [x] **44 of the 49 samples are reachable, and the other five are dead in the shipped
      game.** `SBFileTable` (DS:`0x8651`, image `0x1aad1`) names the 49 in `SAMPLES/`
      order, and **entries 8 and 9 are both `cheap2b`**: the string `cheap1b` is not in the
      image at all, though `SAMPLES/CHEAP1B` is on the disk and the baker bakes it. The
      unreachable five are `camel4`, the duplicate `cheap2b`, `grnt3b`, `mud2` and
      `nitland`, and two of them are unreachable because of the two dead routines below
- [x] **Three of the 23 sound routines are silent, and are translated as silent.**
      `KAudio0`/`KAudio1` (`0x5827`) and `HengeThunderSnd` (`0xb419`) are each a bare
      `ret`. `KnightGruntSound`/`KnightStruckSound` (`0x3d5a`) cycles a counter at
      DS:`0x77a2`, builds the grunt id `cnt + 4`, and hits an unconditional `ret`: the
      `jmp PLAY_SFX` at `0x3d73` that would play it is unreachable, nothing branches
      there, and all ten of the scripts' gosubs carry `0x3d66`, the link-time offset of
      `0x3d5a`. So the knight's grunt is computed and never played, which is why `grnt3b`
      is one of the unreachable five. Two more are quirks: `AddRoar` (`0x3d79`) pops its
      table pointer back before loading, so a roar is always entry zero (every trogg roar
      is `camel3b`, which costs `camel4`), and `AddCrushSnd` (`0x5816`) tests `AX` without
      loading it, so it reads the task x the `TASKGOSUB` handler left there and the
      mudman's crush is heard on three columns in four. All of it is in `docs/TASKVM.md`
      and in `crates/henge-core/src/sound.rs`, with addresses
- [ ] The two sound sites outside a fight. Both are recovered, neither is wired:
      `AddClickSound` (`0xd508`, id `0x0f`, `hit3`) is the gadget click and belongs to
      whatever draws the gadget, and `ShakeDiceSnd` (`0xb32f`, id `0x10`, `hedland`) is the
      dice cup, played unless `MUSICTYPE` at DS:`0x8643` is 2, which is the Roland. Both
      ids are in `henge_core::sound` as `CLICK` and `DICE_SHAKE`
- [x] **Music. The driver was traced and the note data came out.** `xTUNEn.BIN` is a
      relocatable x86 driver with the song welded into it, entered through `int 60h` with
      `ah = 0` to start, `1` to tick and `2` to stop, and ticked by the game's own timer
      at 1193182 / 0x5555 Hz. `MusicTable` is eighteen records, six tunes by three sound
      cards: `a` drives an AdLib, `b` the PC speaker, and **`r` an MPU-401, which is
      plain MIDI**. `tools/tunes.py` runs the Roland driver under the same 8086 harness
      that unpacked the executable and reads the note stream off the port. Four of the
      six loop, and the loop point falls out of comparing what is sounding tick for tick.
      Five callers of `LOADMUSIC` say where each plays, and nothing else in the game has
      music at all. **The voices are ours**, because the stream names a Roland's
      instruments and nothing else: wavetables and envelopes by General MIDI family, in
      `henge_audio::music`

## 2.6 The clock `done`

| Original | What it is | Status |
|---|---|---|
| the wait at image `0x5a24` (`WAITVSYNC`, `WAITVBS`) | the frame | done, recovered: it **is** the engine's tick |
| `Install_Timer`, `Remove_Timer`, `Times3`, the handler at `0x5934` | the 8253 timer | done, recovered: audio only |

**The game's frame is one vertical retrace, and nothing else paces it.** The wait is the
unnamed public routine at image `0x5a24`, in the gap between `AdjustJoy` (`0x59f8`) and the
start of `GFX` (`0x5a6e`):

```text
5a24  ba da 03   mov dx, 0x3da        ; VGA input status 1
5a27  90         nop
5a28  ec         in  al, dx
5a29  a8 08      test al, 8
5a2b  75 fb      jne 0x5a28           ; wait while in vertical blank
5a2d  90         nop
5a2e  ec         in  al, dx
5a2f  a8 08      test al, 8
5a31  74 fb      je  0x5a2e           ; then wait for it to start
5a33  c3         ret
```

Eight places call it, and the four that matter are main loops, once a pass: `Combat` at
`0x0354` (the loop is `0x0351` to `0x0374`, eleven calls and a jump back), `MapLOOP` at
`0x0a306`, `ScanKEYS` at `0x0145a` and `FindLandscape` at `0x0afed`. The other four are
`ShakeScreen` (`0x0496b`), `KnightWonGame` (`0x01117`), `FightDemon` (`0x01031`) and the
palette fade-out loop (`0x05bb0`), which is why a fade is sixteen steps at one a frame.

**So the rate is the video mode's refresh rate, and the mode's timing is the BIOS's.** The
image never writes the Miscellaneous Output register at `0x3c2`, and the only CRTC writes
anywhere in it are index 0x0c, the start address, at `0x5a34` and inside `ShakeScreen` at
`0x4965`. Nothing reprograms the vertical total. A 320x200 VGA mode is the 400-line
timing: 25.175 MHz over 800 dots is 31468.75 lines a second, over 449 lines is
**70.0863 frames a second**. That is the figure `henge_core::intro` already quoted for the
story card's 420-retrace wait.

The engine's tick is therefore 14,268,123 ns, in `henge-desktop`'s event loop. **It used to
be sixty**, which ran every recovered cooldown, script duration and fade about fourteen
percent slow, since all of them are counted in frames.

**The 54.62 Hz timer is a different clock and drives only sound.** `Install_Timer` at
`0x584f` programs counter 0 with `out 0x43, 0x36` and a divisor of `0x5555`, which is
1193182 / 21845 = 54.62 Hz, and hooks int 8 to a handler at `0x5934`. The handler does
exactly three things: `mov ah, 1; int 60h` to tick the music driver, the same with `int 61h`
to tick the sound effects unless the card is 3, and `Times3`, a counter reloaded with 3, so
every third tick it chains the original int 8 and DOS keeps its 18.2 Hz time. It touches no
game state. `henge_audio::music` carries that rate inside the score, so the tunes are
unaffected by the game's clock.

## 2.7 Memory and DOS `done, not needed`

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
| `CLEARCOLLISIONS`, `TASKCOLLISION`, `TASKWALKCOLLIDE`, `TaskCol_MainLoop` (0x9f26), `COLCHK` (0x9fcd) | collision | partial. **The weapon pile is recovered**: `COLLIDE.HIT`'s polyline is baked onto each cel and swept exactly as `CHECKL` (0xa0da) and `NOWID1` (0xa0ed) walk it, mirrored by `neg ax; add ax, [WIDTH]` at 0xa0e7. **The strike point is recovered**: `CXx`/`CY` (0xa130, 0xa13f) are the point of the sweep that landed, not the middle of anything, and `TaskCol_MainLoop` writes them into the struck actor's `+0x58`/`+0x5a` (0x9f87). **The body pile is not**: the original tests each weapon point against the body cel's own pixel mask (`CBITLP`, 0xa190) and this tests the swept line against the authored body box |
| `ControlKnight`, `KnightAttack`, `Rjoystick`, `Ljoystick`, `KnightAttSw` | the attack the direction picks | **done**, recovered: `Attack::for_direction` |
| `KnightHitSw`, `KnightDamSw`, `*Hit`, `*Dam`, `KnightSAnim`, `CalcDamage` | blow taken and damage by kind | done, as `hurt_by` and `attacks` on every actor; `CalcDamage`'s strength and sword are item 40 |
| `CheckBlock`, `blockflag`, `KnightBloSw` | blocking | **done**, recovered; `KnightBloSw` is the block table |
| `KnightHitNormal`, `KnightHitKnight`, `TroggHit`, actor `+0x12` | the recovery a landed blow cuts a swing into | done as `State::Recover` |
| `SWORDFLAG` | weapon state, drawn and sheathed | todo |
| `ControlBlackKnight` (0x4b79), `BKnightMove` (0x4bd3), `BKnightAttack` (0x4c13), `BKBlock` (0x4c40), `_evadechop` (0x4cad), `BKAttack` (0x4cc3), `BKnightStruck` (0x4d50), `BlackKnightStruck` (0x4d7f), `BKnightHit` (0x4da8), `BKnightHitKnight` (0x4dc6), `Progression` (DS:0x7c01) | the knight the machine plays | **done**, translated block for block; see 3.2 |
| `KnifeThrow`, `ControlKnife`, `Knife`, `SpeedKnife`, `KnifeDam`, `SetKnightEquipment` | the thrown dagger | **done**, as a `Missile` in the bout |
| `BLOW` | a landed blow | done as `HitEvent`; a stopped one is a `Parry` |
| `GORESWITCH` (DS:0x700), `OSWITCHES`, `GOREOPT` | the gore switch | **done**: the title row toggles it, `Bout::bloodless` carries it |
| `AddBlood`, `Blood1`, `BloodFile`, `BLOODBUFFER` | blood | done: the spray at the strike point on bank table 4, from the three `*Struck` that call it |
| `DeCapFLAG`, `SetDecapFLAG`, `Knight_SwDeCap`, `Knight_SwCollapse`, `MudmenStruck1`, `KnightKnightStruck1`, `TroggChopHead` | the finisher on a fallen knight | done; the plain opponent comes in for it with the gore on |
| `Knight_Explode`, `TrollOHead`, `TroggSpear_Toss`, `DrDropHead`, `DrDropClaws` | the creatures' own finishers | mostly done. The trogg comes in for the head with the gore on, and so does the computer knight (`BKnightAttack+0x18`, 0x4c2b, with no gore test and no cooldown of its own). **`TrollOHead` (0x4397) is built**: a troll's overhead chop that takes the last of a knight's hit points plays `Knight_Explode` rather than laying him down, and only the chop does. **`TroggSpear_Toss` is built**: `TroggHit+12` (0x2f59) has the spear, and only the spear, pick a dead player knight up on the point with the gore on, taking his own task away (0x2f86) so the corpse is drawn inside the toss. `DrDropHead` and `DrDropClaws` are the dragon's and are not; Balok's landing is not |
| `Order`, `DS:0x783a`, `+0x28` | a creature naming its own script and kind rather than pressing a button | **done**, item 37 |
| `ALLDEAD` | everyone down | done as `Bout::settled` |
| `KNIGHTREFRESH`, `KNIGHTLOC`, `KNIGHTBUFFER`, `ENEMYBUFFER` | actor state | done as `Fighter` |
| `NUM_PLAYERS`, `PLAYER1`-`PLAYER4`, `PLAYERPOINTER` | up to four players | done |
| `GAME_XP` | experience inside a bout | **todo** |

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
- [x] **The computer knight (`ControlBlackKnight`).** What stood here was three
      lines: close, swing, cool down twenty. The routine and its eleven blocks are
      translated now, with `Progression` and the day count that indexes it

## 3.2 Creatures `done`

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
| actor record `+0x38`, `+0x3c`, `+0x52`, `+0x54`, `+0x56`, `+0x35`, `+0x18` | health, maximum, approach, back-off, plane, kind, bank table | done, and the tracker reads all three ranges |
| `TroggWALKR`/`U`/`D`, `TrollWALKR`, `MudmenWALK`, `BKnightWALKR`/`U`/`D`, `BeastChargeOffsets` | pixels moved per walk frame | read; speeds set from them |
| `MonsterTrack`, `CheckZAxis`, `CheckXAxis`, `FaceKnight`, `MonsterWalk`, `NextWalk` | the tracker: close to `+0x52`, retreat inside `+0x54`, same plane within `+0x56` | **done**, transcribed, as `monster::track` |
| `FaceKnight` (0x3cf3), the controller tail (0x2d52, `mov dh, [di+8]`), `TASKHANDLE` (0x9741, `mov [di+0x14], dh`), `ADDTASK` (0x968a), `perdone` (0x99d5), `TASK_FLIP` (0x9a6d), `TASKLEFT` (0x9864), `MoveBACK` (0x5783), `ControlKnight` (0x3f77, 0x3f86) | which way a fighter faces, how it reaches the task, and how the blit mirrors | **done**, each link translated with its listing; `BUILD_ORDER.md` item 37 walks the chain. A creature is turned by `FaceKnight` and by nothing it does while walking |
| `ControlTrogg` (0x2ddf) to `TroggChop` (0x2eff), `TroggStruck+3` (0x2f1c), `TroggHit+8` (0x2f55), `DeCapFLAG` (DS:0x7841) | the trogg, whole | **done**, translated block for block: ten frames of stance after a blow and the next on the eleventh (`sub; jmp` at 0x2eb1, not the demon's `sub; jne`), the count forgotten when struck and restarted at ten when a blow lands, the flag raised on the decision to finish |
| `KnightGotStruck` (0x4267), `DemonStruck1` (0x4360) into `DemonSlap` (0x437f), `ClawStruck1+26` (0x43ed), `BalokStruck1+7` (0x427d) | a blow turning whoever takes it: the table at DS:0x7843 is indexed by the *striker's* kind, and only the demon's slap (kinds 0x10 and 2) and the dragon's claw write `+8` | **done**, as `monster::struck_facing` and `Bout::turn_struck`. The balok's copy of the demon's three instructions is dead code: the `jne` at 0x427b reads the flags `add bx, ax` left at 0x4272 and `0x7843 + kind` is never zero, so a balok's slap does not turn the knight |
| `RatmanHit` (0x34f0, the `cmp al, [di+8]` at 0x351c), `FlipKnight` (0x3d13) | a ratman's claw on someone facing the same way spins him round, task (`xor byte [si+0x14], 2`) and record (0x3d31) together. The leap (`+0x48 & 1`), the tail (`+0x48 & 8`) and a blow on its own kind (`+0x35 == 0x12`) never reach it | **done**, as `monster::ratman_flips` and `Bout::ratman_hit` |
| `TASKWALKCOLLIDE` (0x9e06) | a fighter may not step into another actor's body, tested only in the direction he faces (`xdir = facing & 3`, 0x9e58) | **not built**; `Bout::separate` pushes overlapping fighters apart instead, which is ours |
| `SetKnightCombat` (0x2962, the facing at 0x297d), `InitNewMO` (0x27ee), `TroggTABLE` and the other spawn tables | the knight at x 250, y 0, z 100, facing 3; creatures from eight-byte `[x][y][z][facing]` records, off screen and facing in | **done**. The records are in the readable span (`TroggTABLE` is (-50, 0, 100, 1), (360, 0, 150, 3), (340, 0, 50, 3), (-80, 0, 120, 1)) and they are baked on each actor as `ActorDef::seats`. Only x and facing survive the arrival: `AddPlayer` (0x2989) falls into `AddKnight`, whose `mov [di+6], ax` at 0x29b6 is unconditional, so every `z` in every table is dead, `SetKnightCombat`'s `0x64` included. The second knight's record is x 30 facing 1, written identically by `InitPractice` (0x200a) and `InitKnightvsKnight` (0x206b). `FaceKnight` still turns a creature on its first frame, which is the original doing that too |
| `CONTROLTABLE`, `ControlTrogg`, `ControlTroll`, `ControlRatmen`, `ControlMudmen`, `ControlBalok`, `ControlBeast`, `ControlDemon`, `ControlDragon`, `ControlClaw` | the controller each kind runs | **done**, as `monster::Controller`, named on each actor |
| `TroggStart`, `TroggAttacks`, `TroggChop`, `TroggSwing`, `TrollAttack`, `TrollBunt`, `ControlRatCollide`, `MudmenReach`, `MudmenIBury`, `MudmenAppear`, `MudmenEntangle`, `MudmenChoke`, `BeastCharge`, `SetBEASTZ`, `SetBeastTimer`, `BalokJump`, `DemonAttack` | per-creature behaviour | **done**, item 37 |
| `ControlBlackKnight` (0x4b79) and everything under it: `BKnightMove` (0x4bd3), `BKnightAttack` (0x4c13), `BKBlock` (0x4c40), `_evadechop` (0x4cad), `BKAttack` (0x4cc3), `K0$`..`K5$` (0x4ce5..0x4d4d), `BKnightStruck` (0x4d50), `BlackKnightStruck` (0x4d7f), `BKnightHit` (0x4da8), `BKnightHitNormal` (0x4dbd), `BKnightHitKnight` (0x4dc6) | the computer knight, `CONTROLTABLE` slot 8 (`InitGameStart+241`, 0x1cfe) against `ControlKnight`'s slot 6 | **done**, translated block for block. It closes on the tracker, blocks a swing (kind 8) and ducks a chop or a lunge (kind 0xe) inside a hundred and twenty, and otherwise picks by range: the swing inside ninety, the overhead chop inside ninety five, the lunge inside a hundred and the thrown dagger past it, never repeating the kind `ATT` holds, and closing when the belt is empty. It gives ground on a roll under the day's figure, comes in for a fallen knight's head inside ninety with `DeCapFLAG` clear, gets its evade's one use back the moment it stops a blow (`and byte [di+0x48], 0x7f`, 0x4d78), and follows a chop or a lunge through rather than recovering (0x4dcc, `0xffff`) |
| `Progression` (DS:0x7c01, twenty bytes `14 0a 08 07 06 05...`), the day count at DS:0x5b1 | how much nerve the computer knight has, and what changes it | **done**. `InitGameStart+60` (0x1c49) writes zero and `EncounterFini+35` (0x1167) adds one every fourth encounter, beside `MoonCount`; `BKBlock+5` (0x4c45) reads it masked to seven and `BKAttack+5` (0x4cc8) reads it unmasked. So the knight hesitates one time in five on the first day, one in ten on the second, and one in twenty from the sixth on, and past the table's twenty bytes `BKAttack` reads `demonbodge`, which is zero, and it never hesitates at all. Kept as `Bout::progression` and `monster::PROGRESSION` |
| `[0x8979]`, `[0x897b]`, `PracticeCombat5` (0x00fe, 0x0104), `MOON:Combat+115` (0x03c4), `InitKnightvsKnight+9` (0x2063) | which two knights a duel fields | **done**, and it is exactly two. The arena browser used to field three or four, the extras wearing the second knight's colours; `ControlBlackKnight`'s own opponent pick (0x4b8c..0x4ba3) has room for two and no more, so the browser fields two now |
| `_WIZARD:RND`, `GETPERCENT` | the shift register `TroggAttacks` rolls against | **done**, as `monster::rnd`, off a seed the bout carries |
| actor record `+0x0a`, `+0x0b`, `+0x48`, `+0x49`, `+0x4a` | a controller's walk frame, timer, flags and cooldown | **done**, as `monster::Brain` on the fighter, in the fingerprint |
| `TroggTABLE` (DS:0x97a, 4), `BeastTABLE` (0x99c, 3), `RatmanTABLE` (0x9b6, 5), `MudmanTABLE` (0x9e0, 5), `BalokTABLE` (0xa5c, 1) | the spawn tables `InitNewMO` reads position and facing from | **done**: eight-byte records `[i16 x][i16 y][i16 z][u8 facing]`, `InitNewMO+0xc..+0x23` (0x27fa..0x2811) copying them into `+2`, `+4`, `+6`, `+8`, baked as `ActorDef::seats` and used for the opening layout. Which record the first arrival takes is not always zero: `SetMonsterCombat` (0x27e4) walks from the front for troggs, beasts, ratmen and Balok, while the troll (0x26f1 into `InitTrogg`, 0x225b, on the trogg's own table) and the mudmen (`InitMudmen`, 0x2655) `xor [SIDE], 1` against the zero `SetUpDKL` left and so start at record **1**. The demon's and the dragon's are written into the record in code, 0x278d and 0x2476. `DemonTABLE` and `WhirlTABLE` are not tables at all, they are single words `InitKnightvsDemon` stores the two records' addresses in |
| `TotalMonsters`, `MaxMonsters`, `NumberInCombat`, `SIDE`, `INITMO`, `INITANIM`, `AdjustLevel`, `lev_adjust`, `KLTAB`, `SetMonsterCombat`, `InitNewMO`, `CountTheDead` | how many come, how many at once, from which side, and when the next one walks in | **done**, item 38, as `henge_core::wave` |
| `SETDEMONBORD` | **not a screen border**: one record written over the arena's own border list, 0 to 309 across and 10 to 99 deep, which is the ground the demon may be fought on. `InitKnightvsDemon` calls it after the arena is loaded and `GENERATELANDSCAPE` calls it before, so only the first of the two survives | **done**, as `ActorDef::border` and `Bout::apply_actor_borders` |
| `Demon_Evolve`, `AddDemonWhirl`, `FlipDemonWhirl`, `StopDemonWhirl`, `KnightOFF`, `KnightON`, `DemonOFollowT`, `DemonOWhipFollow`, `DemonUFollowT`, `DemonUWhipFollow` | the demon's entrance, its whirl, and the whip's four phases | **done**, item 33 |

- [x] Troll, trogg with axe (and hammer), trogg with spear, ratmen, mudmen, beast, Balok
- [x] Demon and dragon, whole: see 3.3 and items 33 and 36
- [x] **Per-creature behaviour (item 37).** `design` was the word here two revisions
      ago and `read in outline` one; it is neither. Every controller reads, and
      `monster.rs` is a transcription of them, with the ranges and the counts the
      code holds. A test drives all nine at eight distances and no two answer alike
- [x] `SETDEMONBORD`, and the correction that it was never a decoration
- [ ] What is left of a creature's own repertoire: the ratman's leap into a tree and
      onto the knight's head (`RatmanInitLeap`, `RatHangKnight`, `RatmanOnHead`,
      `RatmanGouge`), Balok's grab and its landing on a knight (`ControlBalokGrab`,
      `ControlBalokBite`, `ControlBalokCrush`, `Knight_Explode`), and the beast's
      `Beast_BackToss` and `ChestToss`
- [x] **Waves (item 38).** All of it reads and all of it is transcribed in
      `henge_core::wave`. Four words of BSS carry it: `TotalMonsters` (DS:0x96a),
      `MaxMonsters` (0x96c), `NumberInCombat` (0x96e) and `SIDE` (0x970), with `INITMO`
      (0x972) the routine that puts one more in and `INITANIM` (0x974) the creature's own
      `Set*Tables`.

      **How many at once** is `MaxMonsters`, which every `InitKnightvs*` writes as 1
      except `InitKnightvsRatmen` (0x2337), which writes 2. `SetMonsterCombat` (0x27e4) is
      `cx = [MaxMonsters]` passes of `InitNewMO`, the seat pointer walking eight bytes a
      pass, so that is exactly what the fight opens with. `InitKnightvsMudmen` (0x264c) and
      `InitKnightvsTroll` (0x26f4) call `INITMO` once instead, so their opening is chosen
      by `SIDE` like every later arrival.

      **How many altogether** is `TotalMonsters`: 3 for the three troggs and the beast, 2
      for the ratmen, Balok and the mudmen, 1 for the troll, the demon and the dragon, plus
      one more when four people are playing (`cmp [0x91e], 4`, and 0x91e is the count
      `Adjplayers` clamps to 1..4). It is not the head count but what is still owed:
      `CountDone` sends one for every death that leaves it above zero, so a fight opens
      with `max` and shows `max + total - 1` creatures altogether.

      **When the next one walks in** is `CountTheDead` (0x213), which every one of the
      bestiary's eighteen death scripts calls through `TASKGOSUB`. It takes one off
      `NumberInCombat` and one off `TotalMonsters`; if the player is down (`[si+0x38]`
      at or below nothing) or nothing is owed and nothing is standing, it is `StopCombat`
      and thirty five more frames (`mov byte [0x8987], 0x23`); otherwise `CountDone`
      (0x243) calls `INITMO` until the screen holds `MaxMonsters` again.

      **From which side** is `SIDE`. `InitTrogg` (0x225b), `InitBeast` (0x22d2),
      `InitRatmen` (0x239e) and `InitMudmen` (0x2655) all open `xor word [SIDE], 1` and
      step the seat pointer eight bytes on when the result is not zero, so the seats run
      1, 0, 1, 0 off the 0 `SetUpDKL` (0x292e) leaves. `InitBalok` (0x25cf) has no such
      flip and every Balok comes in at the same seat. The depth is `AddCNT` as ever.

      **What `AdjustLevel` (0x2824) does**, in its own order: `MaxMonsters` up one if
      strength (`+0x2e`) is above three; `TotalMonsters` up one at thirty experience
      (`+0x3c`), `MaxMonsters` up one at sixty, `TotalMonsters` up one at ninety, writing
      `RegenerateFLAG` (0xa4a) 1, 2 and 3 as it goes; then, in a lair, the lair record's
      own `+4` written straight over `TotalMonsters` (0x287e); then three ceilings, 1 for
      Balok and the mudmen (0x288a, 0x2898) and 2 for the troll (0x289e). Last, the level:
      `CalcDamage` with the swing's kind in `+0x28`, plus experience over four, halved,
      less six, floored at nothing and held at fifteen, halved again for nought to seven.
      That indexes the creature's row of `lev_adjust` (DS:0xa0a, eight rows of eight signed
      bytes) found by looking `INITANIM` up in `KLTAB` (DS:0xa4c, eight words, in the order
      `SetBalokTables`, `SetRatmenTables`, `SetTroggAxeTables`, `SetTroggHammerTables`,
      `SetTroggSpTables`, `SetUpMudmenTables`, `SetTrollTable`, `SetBeastTables`), and the
      entry is **subtracted** from `TotalMonsters`, and only if the result is positive:

      ```
      balok         2  2  1  1  0 -1 -1 -2
      ratmen        5  4  3  2  0 -1 -2 -4
      trogg axe     5  4  2  0  0 -1 -3 -4
      trogg hammer  5  4  2  0  0 -1 -3 -4
      trogg spear   3  3  2  2  0 -1 -2 -3
      mudmen        3  2  1  0  0 -1 -1 -2
      troll         3  2  1  0  0  0  0 -1
      beast         2  1  0  0 -1 -1 -2 -3
      ```

      So a knight fresh off the select screen raids a lair of fourteen troggs and meets
      nine of them, and a knight who swings for twenty with ninety experience meets more
      than the table says. The rows and the counts are carried on each creature's own
      definition, so a pack decides; nothing in the engine matches on an id

## 3.3 The dragon `done, but for the map flight`

`BATTLEDRAGON`, `DRAGON`, `DRAGONOVER`, `DRAGONFLAG`, `DRAGONDEADFLAG`, `LOADDRAGON`.

A distinct set-piece encounter with its own state machine, not an ordinary bout, and
all of it reads. `InitKnightvsDragon` (0x2438) places the head at x 80 and z 100 and
then builds two more actors through `FindTABLE`, `Claw1TABLE` and `Claw2TABLE`, at
x 5 and ten rows either side of the head; `DragonMoveClaw1` keeps them there.

| Original | What it is | Status |
|---|---|---|
| `ControlDragon`, `DragonMove`, `DragonMoveLow`, `DragonHeadMove` | the head lifts inside 140 and lowers outside it, over the 13 and 9 frames `[di+0x4a]` counts, walking the `DragonWal` rows at +0x10 and +0x20 | **done** |
| `DragonFLAGS` | 0x10 a head move running, 0x20 the head is up, 0x40 breathing, 0x80 the knight has landed a blow | **done**, as `monster::flag` |
| `Dragon_LiftHead1`, `Dragon_LowerHead1` | swap the stance itself by writing `Dragon_HighStance` or `Dragon_Stance` into `+0x10` | **done**, as `Act::Stand` |
| `DragonAttack`, `DragonLowAttack`, `AddDragonFIRE`, `Dragon_Fire` | breath high past seventy or once struck, bite closer, breath low with the head down; the fire is a task of its own on `DRAGON5.CEL` | **done** |
| `TrackKnight` | the head inside a corridor 30 to 100 wide, following him in depth, and tracking to two pixels while it breathes | **done** |
| `ControlClaw`, `ClawHit`, `ClawStruck`, `DEAD_CLAWS`, `DrDropClaws` | the claws slap inside x 100, take no damage at all, and die with the dragon | **done** |
| `DragonStruck`, `DragonHit2`, `DragonDam`, `Knight_Burn`, `Knight_SwSlapped` | what a blow does either way, with the rows `InitKnightvsDragon` overwrites for this fight | **done**; `Dragon_BitKnight`, the chewing, is not |
| `Dragon_Flight1`..`8`, `DrBuffer`, `DrAnim`, `_MAP:InitDragon`, `ContinueDragon`, `DragonWander`, `DragonTRACK` | the dragon over the map | bank **recovered**, flight not flown |

**The flight's loader is found, and the record was wrong about which bank it is.**
`_MAP:ContinueDragon` builds `DrBuffer` at DS:`0xccbc` by writing the far pointer at
DS:`0x8975` into all five of its slots, and `DrAnim` at DS:`0xcc22` is sixteen words,
each of the eight flight scripts twice. DS:`0x8975` holds `MI.C`, the map icon bank,
not `DRAGON5.CEL`: the loader at 0x88b5 stores the address the *next* file will be
loaded at before loading it, and the file after `ki.cel` is `mi.c`. Cels 34 to 41 of
`MI.C` composite to a dragon seen from above with its wings beating.
`DRAGON5.CEL` is slot 4 of the creature table and it is the fire. The flight table is
in the pack as the dragon's table 5; `tools/taskvm.py --actor dragon_flight` draws it.

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
classifier that used to stand behind it, which guessed the terrain from the map picture
when a pack had no grid, is **gone**. The bake fails outright without the unpacked image
(`overworld tables: the image vanished during the bake`) and checks the table it lifts
against the four codes before it writes it, so a pack always carries the real grid and a
guess behind it was a second answer to a question that has one.

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
`MOON:StackMessages` gives the nine kinds and their menu lines. **`MOON:MapIconsTABLE` is
recovered too now**: it was in the first 2,906 bytes of DGROUP, which the load image held
as a stale duplicate, and that span reads (`REVERSING.md`). It is nine six-byte records
and a terminator, and `henge-bake` reads it:

```
0x15 (18, 11)   0x16 (286, 11)   0x17 (0, 187)   0x18 (303, 192)   the four villages
0x19 (82, 28)   Highwood         0x1a (277, 143) Waterdeep
0x1b (88, 155)  Stonehenge       0x1c (152, 97)  Valley of the Gods
0x1e (217, 11)  Math the Wizard
```

Each pair is the icon's top-left corner, which is what `CheckGROOC` measures the box from,
so it is exactly what a place wants. The two towns cross-check: `KnightGoesToTown`'s
(94, 47) and (297, 157) are the spots a knight is *sent* to, and both land inside the
recovered boxes.

**What the hand-siting got wrong is worth recording.** The tower was one pixel out. The
two towns were nine. But the ruin in the southern woods this project had called the
hermit's is Stonehenge, and the ring in the middle of it all this project had called
Stonehenge is the Valley of the Gods: both were on the right artwork under the wrong name,
85 and 74 pixels from where they belong. All five are read out of the table now. Every one
of the recovered menu lines is used as its own gadget's label.

**The hermit is gone.** There was a second healer in the southern woods, sited by hand and
selling a flask and a draught this project invented; the original has no such place, no
such man and no such goods, and all of it has been removed. Nothing on the map is sited by
us any more: every box comes out of `MapIconsTABLE` or `LairLocation`, and the baker has a
test that says so.

**The four villages are built.** `CheckGROOC` at image 0x732 gates each of frames 0x15 to
0x18 on `[di+0x20]`, the knight's own colour index, so village 0x15 is knight 0's and the
other three cannot see it: the entry never reaches the paper. What is in one is
`ForestVillage` at 0x112a, which `MooresVillage` and `WasteVillage` are two more names for
and which `TakingMoon` sends all four frames to (0xc99 to 0xcb6):

```
0112a  mov  si, [JOYSTICK1+6]      ; whose turn it is
0112e  cmp  byte [si+0x31], 3      ; his life points
01132  jge  EncounterDone
01134  add  byte [si+0x31], 1
01138  mov  ax, 9 / call ColourStatus   ; and straight back out to the map
```

One life point, three is the ceiling, nothing is paid and no day passes. No backdrop is
loaded anywhere on that path, so a village is a line on the paper and not a screen, and
henge opens it where the traveller stands.

- [x] The real terrain table
- [x] `CHECKY`/`CHECKY2`: what makes ground impassable
- [x] Scrolling: settled, and negative
- [x] The whole location graph, out of `MapIconsTABLE` and `LairLocation`: the two towns,
      Stonehenge, the Valley of the Gods, Math's tower and the twenty four lairs, every
      one of them at the original's own coordinates. Nothing is ours
- [x] The four home villages, one per knight, `MapIconsTABLE` frames 0x15 to 0x18 at
      (18, 11), (286, 11), (0, 187) and (303, 192), gated on `[di+0x20]` by `CheckGROOC`
      (0x732) and worth one life point up to three (`ForestVillage`, 0x112a). The
      between-days screen's `Visit your home village to restore lost lives` is now true
- [ ] Pillaging a dead rival's grave (`kngrave`, `StackMessages[0x21 - 0x15]`,
      `Pillage knight's grave`). **Recovered and not built, because there is nobody to
      pillage.** It is not a place: `MOON:CheckEncounterDone` (0x798) walks the other three
      knight records at DS:0x6c9e, overlaps their tokens with yours, and pushes kind 1
      (`Battle with `) for a living one and kind 0x21 for one whose life points are gone
      (`cmp byte [si+0x31], 0 / jg`, 0x7c7); `_MAP:DisplayOtherKnights` (0xa22c) draws
      `MI.C` frame 0x21 over him instead of his own colour for the same reason, so 0x21 is
      a gravestone. Choosing it goes to 0x3b7, the same entry a challenge does, which sees
      the zero at `[di+0x31]` and jumps straight to `Knight1Won` and `TakeALL` (0xbc5),
      the field-by-field transfer of everything he was carrying. henge has one knight to a
      run and no rival tokens on the map, so there is no grave to stand on; building one
      would mean inventing the rival

## 4.2 Arena generation `done`

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

**The moors is settled, and it is not a fifth family.** `_LOADER`'s public list runs
`LOADREGION`, `GENERATELANDSCAPE`, `GENERATEMOORES`, `GENERATEFOREST`, `GENERATESWAMP`,
`GENERATEWASTE`, `LOADTILEV`: one dispatcher and four families, and they are in
landscape code order. `GENERATELANDSCAPE` at image 0x8cb3 reads the code out of
`[0x694e]` and calls through a four-word table; the entries are link addresses like
every other stored code address, and corrected they are 0x8cd3, 0x8d02, 0x8d31 and
0x8d60. The first loads `GLB1.CMP` and reads `PlainTable[PLAINCOUNT]`, which is
`GL1.t`..`GL8.t`. `MapType` holds only 0, 2, 4 and 6, so there is no fifth code and no
missing `MO*` file: the moors is code 0, the ground this project files under the `GL`
prefix its own layouts carry, and it renders. The name is worth keeping in the record
because the artwork is unmistakably moorland, a golden field under an open sky, and
nothing else in the game looks like it.

- [x] The selection tables, and the rotation
- [x] Which sheet a placement draws from
- [x] The moors: code 0, `GLB1.CMP` over `GL1.t`..`GL8.t`, and no fifth family
- [x] A layout with no scenery is still a layout. `SWL2.T` has bounds like its
      neighbours and an empty placement list on purpose, and the baker was dropping
      it along with the `F09`/`SW9` stubs, which left the fourteenth lair with no
      ground to be fought on. The stubs are caught by their bounds instead

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

**All four per-lair tables are recovered now.** `ForestLairs`, `LairLocation` and
`LairType` were inside the 2,906 bytes of DGROUP the load image held as a stale duplicate;
that span reads (`REVERSING.md`) and `henge-bake` reads it. The initialiser's copy loop at
image 0x1ea0 is what gives each of them its shape, field by field:

```
mov ax, [di] ; add di, 2 ; mov [si+0x02], ax   ForestLairs   CombatTable byte offset
mov ax, [di] ; add di, 2 ; mov [si+0x04], ax   ForestLairs   TotalMonsters
mov ax, [bx] ; add bx, 2 ; mov [si+0x0a], ax   LairLocation  x
mov ax, [bx] ; add bx, 2 ; mov [si+0x0c], ax   LairLocation  y
mov ax, [bp] ; add bp, 2 ; mov [si+0x0e], ax   LairType      landscape
mov ax, [si] ; add si, 2 ; mov [si+0x10], ax   LairFile      arena layout
```

So `ForestLairs` is 24 pairs of words, `LairLocation` 24 pairs and `LairType` 24 single
words, which is the 96, 96 and 48 bytes the symbol table gives. Which guardian each slot
is comes from `InitGameStart` at image 0x1d44, seventeen `mov word ptr [si + n], imm` with
`si` on `CombatTable`; the parallel run just above it at 0x1ced fills the controller table
with `ControlBeast`, `ControlMudmen`, `ControlDemon` and the rest in the same order, which
is what pins each slot's meaning.

What comes out: the forest holds ten to fourteen ratmen or axe troggs, the waste three to
five Baloks and ten hammer troggs, the marsh five or six mudmen and six or seven trolls,
and the glades four to six spear troggs, eight or nine beasts and thirteen axe troggs.
`LairType` is six 2s, six 6s, six 4s and six 0s, the forest, waste, marsh, glade order
`LairFile` already gave, arriving a second time from a second table; and twenty three of
the twenty four `LairLocation` pairs land on a `MapType` cell of that same code. The odd
one, lair 15, is a cell into the treeline and is still fought in the marsh, because
`InitLair` hands `ColourBackdrop` the record's landscape and never asks the map.

The head count is `TotalMonsters`, which the original feeds in in waves, and **the waves
are built** (see 3.2). `AdjustLevel` (0x2824) writes the lair record's own `+4` over
`TotalMonsters` at 0x287e and then takes the creature's `lev_adjust` row off it;
`SetMonsterCombat` (0x27e4) stands `MaxMonsters` up, which is one for every creature but
the ratmen; and `CountTheDead` (0x213) sends the next one in on the frame the last one's
death script reaches its `TASKGOSUB`. So a lair of fourteen is fourteen fights one after
another, and the number goes into the pack unrounded because nothing has to hold it all at
once any more.

What this replaced: twenty four hand-sited lairs, a median of 51 pixels and as much as 129
from where the original puts them, and twenty four invented guardians of which five
happened to be right.

- [x] Lair placement, entry, contents, the guardian fight
- [ ] `LairGEM`, which is the one thing a gem flight is for in the original: looking into
      a lair from the air, and being put back where you started on the way out. Here a
      gem flight ends on fire instead
- [ ] `_STATUS:DisplayLair` (0xc67f), the panel page that draws the floor as icons. The
      routine is transcribed in `status::display_lair` -- three cels of a hoard at
      (0x3a, 0x2e), (0x4c, 0x21) and (0x3a, 0x3c) plus `StatsOffset` 0x96, then
      `DisplayGold`, `DisplayMSword` and `DisplayMagic` over the lair's own record -- but
      nothing opens the panel on a lair yet, so a lair still says what is on its floor in
      words

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
hands out a moonstone, which is 7's. It does now.

**What is ours**: the eight bytes of `MOON:Moons`, which are in the stale part of DGROUP,
so the cycle is five pictures over eight steps waning and waxing back. **Nothing else.** A
day number and one of the fourteen `_LOADER:WaitMES` hints used to be drawn under the
heading; the fourteen belong to `WAITMESSAGE`, which is a different screen shown while a
disk loads, and the routine at 0x8e5b draws one chain and one cel and nothing besides.
Both are gone.

**And the screen waits.** `_MAP:NextWHICH` at 0xa454 is three calls in a row: the screen at
0x8e5b, then `WaitFIRE` at 0x8251, which is `call 0x81ec; test bx, 0x10; je` until fire is
down and the same again until it is up, and then the fade out at 0x5b65, which is
`FADEOUTDAY`. It used to dismiss itself after 150 ticks. It waits now, and goes out over
the same sixteen linear steps every other fade uses.

- [x] Moon phases on a weekly cycle, and the between-days screen
- [x] What the moon gates
- [x] `FADEOUTDAY`, the fade between the map and the screen, which is item 75's

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
token like the knights' own. `MapIconsTABLE` is sixty bytes, and reading it confirms the
shape: nine records of three words, one for each of the fixed places above, and a
terminator. Its coordinates are in 4.1.

**The four villages are built** (see 4.1); pillaging a dead rival's grave is recovered and
not built, because with one knight to a run there is no rival token to stand on.

### The paper, in full, and what it replaces `done`

Walking onto a place used to open it there and then, on an edge, and that was an invention.
**The original never opens anything by walking.** The whole of it, with addresses:

```text
0x06b5  the walker         clears five eight-byte slots at DS:043c (mov bx, 0x43c; mov cx, 5),
                           walks MapIconsTABLE three words a pass, calls CheckGROOC on each
                           and pushes [x][y][kind] for every overlap
0x0798  CheckEncounterDone the other three knights: kind 1 alive, 0x21 dead
0x085a  CheckLairEncounter the 24 lairs, kind 2, and it blits MI.C frame 0x1f at the one
                           you are on before it pushes
0xa2d3  FOLLOW             calls the walker once a frame, so the stack is rebuilt from
                           nothing every frame and remembers nothing
0xa3c6  ScrollINPUT        mov ax, [JOYS]; test ax, 0x10; je  -> call DisplayStack
0xae27  DisplayStack       counts the live slots: 0 returns 0 (NoEncounter), 1 falls into
                           StackDecision, more draws CreatePaper and waits
0xae58  the key loop       call 0x8149 until 2 <= ax <= 0x0a, then (ax - 2) * 8 is the slot;
                           an empty slot is refused and it asks again
0xae9f  StackDecision      kind 1 or 0x21 -> 0x101de, kind 2 -> 0x1039b, else -> 0x10aa6
0xaed4  CreatePaper        the panel, the heading and the numbered lines
0xaf5e  OrderOpt           kind 2 -> knlair, kind 1 -> knkn plus [di+0x4c], else
                           StackMessages[kind - 0x15]
0xaf99  InitPaper          PaperX = 0x32, PaperY = 0x64, blit MI.C cel 0x20 there
```

`MI.C` frame 0x20 is **174 by 51**, the only frame in the bank that size, and it is the
paper itself. `CreatePaper` then puts the knight's own token at `PaperX + 5, PaperY + 5` in
the frame his seat names with nothing added, the heading at `+15, +5`, which is his name
with `_MAP:knightopt` (` may ... `) after it, and then steps `PaperY` by fifteen once and
by **six** for each line, drawing every line at `PaperX + 5, PaperY + 5`. Six is the small
face's height, and the bold face is twenty tall, so the step names the font as well.

`StackMessages` at `DS:0xc404` is fourteen pointers for kinds 0x15 to 0x22, and reading them
gives `knvillage` four times, `knhigh`, `knwater`, `knhenge`, `knvalley` twice, `knmath`,
`knmath`, `knmath`, `kngrave`, `knmath`. Each line is baked beside the place it belongs to.

The key loop has **exactly one exit** and it is a number naming something under your feet:
there is no escape key in the original and there is none here. Every entry is a place with
its own way out, so the paper cannot strand anyone.

**One more thing the map draws, which was missed.** `CheckLairEncounter` blits `MI.C` frame
0x1f, nine by five like the lair marker it covers, at the lair the token overlaps, every
frame, before `SHOW` puts the token down. It is drawn entirely in palette index 31, which is
the entry `MapEffects` glows, so the lair you are standing on breathes in the same colour
the token does. So `SHOW`, `DisplayLairs` and `DisplayOtherKnights` are *nearly* all of what
goes on the map picture, and this is the fourth.

**What was removed with it**: a status bar across the bottom of the map, a purse plate in
the top corner, a cutpurse notice beside it, and flat one-colour silhouettes of `MI.C`
frames 0x15 and up, which are one-pixel outlines the original blits nowhere. `MAP.CMP` is
one 320x200 picture and nothing else is on it. What a cutpurse took is said in the box the
original says things in, `OCCURMESSAGE`, since the map has no line to put it on.

| Original | What it is | Status |
|---|---|---|
| `TAVERN`, `TavernOpenScene`, `TavernLoop`, `LeaveTavern`, `SetBET` | the tavern | done: the five painted stakes, and an empty purse turned out at the door |
| `DICE`, `RollDice`, `DiceSort`, `DiceWinner`, `DiceODDS`, `DDICE`, `BET` | a dice game | done, and **recovered, not designed** |
| `HEALER`, `HealDon`, `ExitHealer`, `InitDonation` | restore health | done: the town healer takes a donation and spends it down. There is no other healer; a hermit in the woods who took days was ours and has been removed |
| `TEMPLE`, `TTemple`, `SellToTemple`, `GoldSell` | temple services | done for selling; the moonstone counter is a two-knight trade, see 7 |
| `MYSTIC`, `MysticUpDown`, `MysticAbility`, `MysticJudge`, `DonationTAB` | mystic services | done |
| `STONEHENGE`, `MOON:Henge`, `HengeControl`, `HengeWait` | the stone circle | done, and the winning branch fires now that section 7 hands out a moonstone |
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
      scripts of a hand over the table. The result is drawn; the roll is not animated.
      Still not built
- [ ] `HengeControl` and `HengeLOOP`, the circle's own set piece: `ColourEn4Knight`,
      the thunder, and `Knight_LiftMagic`
- [x] The donation bowl the healer and the mystic both take their fee through.
      `_WIZARD:InitDonation` at image 0xbb34 adds four gadgets, all with `STPL` 0x32, whose
      `[si+0x10]` is 4 at (0x90, 0xa9), 5 at (0xa2, 0xa9), 3 at (0x83, 0xb9) and 2 at
      (0xad, 0xb9); `DonateLoop` at 0xbbe6 reads that word itself and `AddDonation` and
      `SubDonation` move exactly **one coin** between `GOLDP` and `DONATION`, refusing at
      an empty purse and an empty bowl. `OkDonation` writes `GOLDP` back into `[si+0x32]`
      and `ExitDonation` throws the bowl away. `DonationRefresh` at 0xbc9e draws it: cel
      `BAG` at (2, 0xa2) and (0x10e, 0xa2), the four gadget cels 4, 5, 3 and 2 of `mys.cel`
      in their own boxes, `YGOL` `Your Gold` at (2, 0xbe) and `Don` `Donation` right
      aligned at (0x106, 0xbe), with `GOLDP` at (0x14, 0xaf) and `DONATION` at (0x120,
      0xaf). Three fixed amounts stood here and the comment beside them admitted they were
      ours; they are gone
- [x] The two town front doors, which are five gadgets over the painted parchment.
      `MOON:InitHighWood` (0xec9) and `InitWaterDeep` (0xf4a) each clear the table and add
      five 64-wide boxes with `+8` zero, so they say nothing, and `+0xe` 1 to 5;
      `HWLOOP` (0xe35) is `MovePointer`, `CHECKGADGET` and a `cmp es:[si+0xe]` ladder into
      `MERC`, `TAV`, `HEAL`, `HTEM` and `CEXIT`. Highwood x 0x100, y 0x1e, 0x42, 0x6a,
      0x8c, 0xb7, heights 0x10, 0x10, 0x10, 0x1a, 0xc; Waterdeep x 0, y 0x1a, 0x3f, 0x65,
      0x86, 0xb6, heights 0x10, 0x10, 0x10, 0x1f, 0xc. `HIGHWOOD.PIV` already has `Visit /
      Merchant / Tavern / Healer / High Temple / Exit` painted down that strip, so henge's
      own panel and its own words are off it now
- [ ] The mouse gadgets the merchant, the tavern and the temple are really made of, which
      is item 53. The merchant and the temple are `_STATUS` panels, not rooms: see 8.3

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
- [x] The moonstones, which the Valley of the Gods hands out for four keys. Section 7.
      The prices every counter knows (`Buy Moonstone for 20 GP`, `Sell Moonstone for 10
      GP`, `Buy Key for 12 GP`, `Sell Key for 6 GP`) are on the items, and no counter in
      a one-knight run will take them, because that page is a trade between two knights

---

# 7. The quest `done`

**The point of the game, and it was translation, not design.** The plan said the symbols
named the scoring machinery but not the quest logic, so what was left was "mostly
recoverable only by playing the original or by design". That was wrong about the logic and
right about the scoring, and the two halves are worth keeping apart.

**The logic is five routines and it is recovered end to end.**

```text
MOON:Valley          si = [knight+0x44]                 the item record
                     cmp byte ptr [si+0x14], 0xf        all four key bits, or nothing
                     jne -> NoKeysMessage, and done
                     InitKnightvsDemon; InitCombat      the Guardian
                     test KnightDeath, 1
                       lost: sub byte ptr [si+0x31], 2
                       won:  add word ptr [si+0x36], 3
                             mov byte ptr [si+0x14], 0  the keys are spent
                             ValleyEnter
                             al = 1 << (rnd & 3)
                             or byte ptr [di+0x16], al  one moonstone of four

MOON:Henge           ax = tonight's moon; bl = [bx+0x16]
                     bl & 4 && ax == 0x2e -> KnightWonGame
                     bl & 8 && ax == 0x2e -> KnightWonGame
                     bl & 2 && ax == 0x2d -> KnightWonGame
                     bl & 1 && ax == 0x31 -> KnightWonGame
                     else HengeInstruct, and an offering

MOON:KnightWonGame   bp = 1; 0x2e -> 2; 0x2d -> 4; 0x31 -> 3
                     knight 3 -> bp |= 0x10, 0 -> 0x20, 1 -> 0x30, 2 -> 0x40
                     push bp
                     mov si, VICTORY; call 0x8eeb      ; OCCURMESSAGE
                     mov ax, 0x14; call 0x5a24         ; one vertical blank
                     call 0x8251                       ; WaitFIRE
                     mov ax, 1; call 0x8f80
                     pop ax; mov [0x8186], al; jmp 0x8e
0x8e                 int 21h / ah=4Ch with that byte in al

MOON:WhoLived        health <= 0: health = maximum; sub byte ptr [si+0x31], 1
                     called from the end of MOON:Combat's own loop, every fight
0x617                mov si, GameOverMes; call 0x8f17  ; INSTRUCTMESSAGE
                     call 0x8251                       ; WaitFIRE
                     call 0xa554                       ; the map's effects off
                     jmp StartAgain
```

**So a win quits the program with a code in it**, low nibble the moon and high nibble the
knight, and a loss goes back to the title. `INTR.EXE` reads that byte: its ending half
takes the same argument and hands it to `ColourMoonstone` (8.6). `quest::Tally::code` is
the byte.

**And both endings are one message over `MESSAGE.PIV` and nothing else.** 0x8eeb is
`OCCURMESSAGE` and 0x8f17 is `INSTRUCTMESSAGE`, which is why the loss arrives in the red
ramp and the win does not; both `call 0x8e90` first, and that is the `rep movsb` that puts
the message picture back. 0x617 is reached from `_MAP:ScrollINPUT`, where scancode 0x10
quits, and from `_MAP:NextWHICH`, where the last knight with no life points left falls
through to it. 0x8251 is `WaitFIRE`: `test bx, 0x10` until fire is down and then until it
is up again.

`VICTORY` is `SHMES7` at y 75 and `SHMES8` at y 95; `GameOverMes` is `GOmes1` at y 95 and
`promes4`, `Press fire to continue`, at y 180. Every record carries flags 1, the centre
bit. **An ending screen of seven counted lines over `bg8.piv` used to stand here and
there was never anything to port it from**, which the table below already said.

**The words are recovered too, and they were hiding in the wrong place.** The message
records `NoKeysMessage`, `ValleyEnter`, `VICTORY`, `GameOverMes` and `HengeInstruct` are
all at DS offsets inside the 2,906 bytes of DGROUP the load image held as a stale
duplicate, so reading them at their own addresses gave animation script bytes. That span
reads now (`REVERSING.md`), and these records have not been re-read out of it. Their
*lines* are somewhere else entirely: MOON's text pool, at image 0xd6e0, past the end of
every module's code.

```text
0xd6d1  Press fire to continue
0xd724  To be granted a / longer life you must / offer an item of /
        magical nature to Danu
0xd772  You have completed / the quest
0xd78f  bg8.piv
0xd83b  You have proven your skill / and agility against the /
        Guardian.  You have been / granted a Moonstone.
0xd89c  You may only enter your / own home village.
0xd8c6  You must have all four keys / to enter the / Valley of the Gods
0xd908  Player       / GAME OVER
```

That span reads now, so the records no longer have to be paired with their lines by
content: `VICTORY` at `DS:0x671` chains `SHMES7` and `SHMES8`, and `GameOverMes` at
`DS:0x76a` chains `GOmes1` and then `promes4`, which is the same
`Press fire to continue` record `HengeInstruct` and `NextDayMes` end on. `bg8.piv` sits in
the pool between the victory lines and the next message and **nothing in the image refers
to its address**; it is one of the three plates `INTR.EXE`'s ending half uses.
`Player      ` sits nine bytes before `GOmes1` and nothing refers to it either: it is dead
data, and it was once taken for the first line of the game-over chain.

**The Guardian is recovered as well.** `MOON:FightDemon` calls `InitKnightvsDemon`, which
writes 250 into the monster's health, one of it, and finishes on
`mov ax, 4; call ColourBackDrop`: **the Valley of the Gods is fought on marsh.**

**There is no scoring.** `GAMEOVER`, `GAMETABLE`, `TOTALS`, `PPOINT`, `PINDEX`,
`FMEM_POINTS` and `FMEM_COLAREA` are `PUBLIC` names with no addresses; the only symbol in
that whole set that carries one is `GameOverMes`, and it is a two-line message. So there
was no tally page to port, and the seven-line one henge used to show is gone: what is left
is `Tally`, which holds the ending and the seat, and `Tally::code`.

| Original | What it is | Status |
|---|---|---|
| `MOON:Valley`, `FightDemon`, `_MAP:knvalley`, `NoKeysMessage`, `ValleyEnter` | the Valley of the Gods | done, verbatim, on a site of our own choosing |
| `MOON:Henge`, `KnightWonGame`, `VICTORY` | winning | done: `OCCURMESSAGE`, `WaitFIRE`, and the exit byte |
| `MOON:WhoLived`, `_MAP:CheckEncounterDone`, `GameOverMes` | losing | done: five life points, `INSTRUCTMESSAGE`, `WaitFIRE`, then the title |
| `_STATUS:StatCheckKeys` | the keys on the sheet | done, its own cels and x spacing |
| `_STATUS:BuyMoonstone`, `SellMoonstone`, `pu18`, `se18` | the moonstone counter | **not built**: it is a trade between two knights' records, and a run has one knight |
| `GAMEOVER`, `GAMETABLE`, `TOTALS`, `PPOINT`, `PINDEX` | scoring | no addresses anywhere, so there is nothing to build and nothing is built |

- [x] The four keys, one per lair, and the door they open
- [x] The moonstone: the Valley of the Gods pays one of four for four keys, at random
- [x] Win condition and ending: two messages, `WaitFIRE`, and the exit byte
- [x] Scoring and the final tally: there is none in the original, so there is none here
- [x] Losing properly: a life point a death, and the title after the last
- [ ] The Guardian is the demon, and the demon has no set piece: 250 hit points and a
      single slap. Section 3.2's item 33 is what turns the end of the game from a wall
      into a fight
- [ ] `HengeControl` and `HengeLOOP`, the circle's own set piece, which is where the
      winning moment should actually happen. Listed in section 5 too
- [x] Four villages, one per knight: built, see 4.1
- [ ] Pillaging a dead rival's grave, which needs the rival knights on the map that henge
      does not have. Recovered in 4.1 with addresses

---

# 8. Presentation and shell `partly done`

| Original | What it is | Status |
|---|---|---|
| `LOADTITLE`, `DoOptions`, `Selection`, `Adjplayers`, `TitleMes` | title screen and its option list | done |
| `LOADCHOOSE`, `ChooseKnight`, `ChooseRefresh`, `FindChosen`, `ChooseFIRE`, `Chosen`, `choose_knight` | character select | done |
| `TypeName`, `NameDone`, `GNAME`, `BNAME`, `ENAME`, `RNAME` | typing your own name over the knight's | **todo** |
| `STATUSDISPLAY`, `V_STATUS`, `STATFLAG`, `DisplayKnight`, `SetUpStatus` | the status panel | done |
| `DisplayMagic` (0xc38e), `StatCheckKeys` (0xc44e), `StatPlaceScroll` (0xc633) | the magic, the keys and the moonstones on every knight's own sheet | done; `DisplayKnight` falls through into the first of them |
| `DisplayLair` (0xc67f), `DisplayDragon` (0xc6ea), `DisplayMerchant` (0xc7ad), `DisplayMSword` (0xc70c), `DisplayGold` (0xc743), `DisplayAquire` (0xc8ce) | the panel's other pages | built as routines and laid out by `status::lay_out`; **no screen reaches them yet**, because they want a second party and henge has one traveller |
| `MovePointer`, `SHOWPOINTER`, `POINTERBUFFER` | mouse pointer | done |
| `ADDGADGET`, `CLEARGADGETS`, `CHECKGADGET`, `GadgetSlot`, `AddIconGadget`, `HotGadget`, `AddClickSound` | clickable UI widgets | done |
| `LOADICONS`, `ICONBUFFER`, `ICONMEMORY` | UI icons | done for the panel's own |
| `LOADMESSAGE`, `WAITMESSAGE`, `OCCURMESSAGE`, `INSTRUCTMESSAGE`, `MESSAGE`, `MesFILE` | message boxes | done |
| `TextASCII`, `TextPTop`, `TextP`, `TextPDone`, `TextLen`, `CheckBOLD` | the text engine the boxes draw through | done |
| `INTR.EXE`: `picfile1..8`, `panfile1..3`, `intro.sti`, `co.sti` | the intro sequence | **done**; `co.sti` and three plates belong to the ending |
| nothing: the original has no save | save and load | **deleted from the game**; the serialisation is the test harness's, see 8.7 |

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

**What the original drew all this over is `CH.PIV`**, and it was in `MAIN.EXE` all along.
`_LOADER:MoonPic` is the string `CH.PIV` and `MoonFont` is `BOLD.F`; the routine at image
`0x87c3` loads both, keeps a copy of the picture in the segment at `DS:0x88fb`, blits cel
0x49 at (5, 20), cel 0x4a at (22, 181) and cel 0x4b at (110, 190), and walks `TitleMes`
over it (`created by` at y 90, `Rob Anderson` at 105, `Loading...` at 150). `DoOptions`
calls `0x890c`, which loads `Sel.cel` and restores that picture through `0x8e3f`, and
`DisplaySelect` blits cel 0x49 again at (5, 10). The copy at `DS:0x88fb` is taken at
`0x8839`, before the three blits, so what comes back is the bare picture: the option
screen is the night sky, the wordmark ten pixels higher, and the rows. The two credit
lines are on the loading title only, and henge no longer draws them on the option screen,
where `Select Knight` at its own y of 170 had been running into the copyright line at 181.
The earlier note that the picture must be in `INTR.EXE` was wrong.

`CH.PIV` also reserves the bold face's five entries, so the wordmark and the option rows
are blitted in their own indices.

**The rows are recovered now, words and coordinates both.** `DisplaySelect` takes the
arrow's `y` off `MOON:ARR` at `DS:0x706`, which holds 85, 110, 148 and 168, and hands
`MOON:OPT1a` to the message walker at image `0x7a86`. That is a chain of six ten-byte
records, `{text, x, y, flags, next}`:

| record | string | x | y | flags |
|---|---|---|---|---|
| `OPT1a` | `Sel1` `Players` | 86 | 83 | 2 |
| `OPT1b` | `Sel2` `Gore` | 86 | 108 | 2 |
| `OPT1f` | `Sel5` `Practice` | 0 | 150 | 3 |
| `OPT1g` | `Sel6` `Select Knight` | 0 | 170 | 3 |
| `OPT1h` | `NPLAYER` `1          ` | 214 | 83 | 2 |
| `GOREOPT` | `TEXTON` `On` | 214 | 108 | 2 |

Flag bit 0 centres the line between `TextLeftBorder` 0 and `TextRightBorder` 320, which is
what the bottom two rows use and why their `x` is zero; bit 2 right-aligns and nothing on
this screen sets it; bit 3 is the bold face's own three-pixel kern, set by `CheckBOLD`
rather than by the record. Bit 1, which the four left-hand records carry, is read nowhere
in the walker. Before it walks the chain `DisplaySelect` prints the player count into
`NPLAYER`'s buffer through the decimal routine at `0x7d7f` and points `GOREOPT`'s first
word at `TEXTOFF` or, if the gore word at `DS:0x700` is zero, at `TEXTON`; that word is
zero in the image, so gore starts on.

What stood here before was `Players N`, `Gore on`, `Practice combat` and `Moon quest` on
an even eighteen-pixel step from y 100. All four wordings and the whole layout were ours.

**Attract mode** cycles the other ten plates, which is ours; those write the five caption
entries first, the way `INTR.EXE`'s `0x0cfb` does, because no other plate reserves them.

## 8.2 Character select `done`

**There is no backdrop.** `ChooseRefresh` opens with `mov ax, 0xf02; out dx, ax` to the
sequencer and `rep stosw` of zero over `0x2000` words, which is every plane of every pixel
set to palette entry 0. The screen is four portraits on black and nothing else. `CH.PIV`
was drawn behind it here for a long time and that was this project's own addition, taken
from the title screen next door; most of what was wrong with this screen followed from it.

`SEL.CEL` is the art: frame 0 an arrow, frame 1 a hollow frame, frames 2 to 5 the four
knights. `ChooseRefresh` runs `bp` from 0 to 3, takes x from `MOON:CCOL` (12, 88, 164,
240), passes `cx = 0x50` for y and cel `bp + 2`, and calls the ordinary cel blit at
`0x5dc8` **with no colour substitution of any kind**. A knight already taken is not drawn,
which is `ChooseRefresh` only drawing the bits still set in `choose_knight`; the highlight
steps over the taken ones and stops at the ends rather than wrapping, which is
`ChooseLoop`; and after a choice it drops to the lowest still free, which is `FindChosen`.

**`SelectPAL` is recovered.** It is 32 Amiga words at `DS:0x892`, `CCOL` is the four words
immediately after it at `DS:0x8d2`, and both were unreadable until the unpacker was found
to stop before the EXEPACK stream ends; `docs/REVERSING.md` has that story. It is the one
palette in the game that lives in the executable rather than in a picture, which is what
you would expect of the one screen with no picture on it. The baker reads it out of the
image as `palette.select` and refuses anything that is not 32 valid `0x0RGB` words.

What is in it explains the artwork: greys `fff`, `aaa`, `666`, `333` at 1 to 4, which all
four portraits share; the bold face's five entries at 5 and 9 to 12, the same reservation
`CH.PIV` and `MESSAGE.PIV` make; greens at 6 to 8; a teal `066` at 15; browns at 16 to 20;
then blues at 24 to 26, `f90` at 27, `0d00`/`0a00`/`0700` at 29 to 31. Cel 2 indexes the
blues, cel 3 the golds, cel 4 the greens and cel 5 the reds, so the four come out blue,
gold, emerald and red **from the palette alone**, agreeing with `KnightGlowColours` which
was read out of a different part of the image. That agreement is what says these are the
right sixty four bytes.

The recolour that used to stand in for the palette is gone. Building four ramps from the
knight shades and drawing each portrait through a substitution into its own was colouring
artwork that is already coloured, and it is why the portraits read as saturated smears.

**The chosen knight glows, and nothing else does.** Cel 1 is 64 by 76 and every pixel of
it is transparent or index 15; no portrait touches 15, and the cleared screen is entry 0.
So entry 15 on this screen is that frame alone, `ChooseKnight`'s
`COLOURGLOW(0x0f, 0x088, 1, 0)` walks it from `SelectPAL`'s `066` to `088` and back
forever, and what a player sees is the frame round the knight they are on breathing. The
glow was recovered once and taken out again because on the `CH.PIV` backdrop entry 15 is
the sky and glowing it repainted the whole screen. The backdrop was the mistake.

The heading is `MOON:CRText`, one ten-byte record out of the same recovered span: `Select
a Knight`, flags 1, which is `TextPTop`'s centre bit, at y 5.

**Nothing on this screen is ours any more.** A line saying whose turn it is, a name under
each portrait, a stat line along the bottom and `Player N` written where a taken knight was
all stood here, and `ChooseRefresh` draws none of them. It clears the screen, walks
`CRText`, blits the portraits whose bits are still set, blits the frame on the chosen one,
and then:

```text
0x1697  cmp word ptr [TypeFLAG], 0
0x169c  je  0x16ad
0x169e  mov si, word ptr [NAMEy]
0x16a2  mov ax, 0x32          ; x
0x16a5  mov bx, 0x32          ; y
0x16a8  xor cx, cx            ; flags: left aligned
0x16aa  call 0x7a70
```

0x7a70 builds a one-record chain at `DS:0x7ff2` out of `si`, `ax`, `bx` and `cx` and falls
into the walker at 0x7a86, so that is **the name being typed, at (50, 50), in the bold face
the heading is set in**. That is the whole routine; there is nothing else on the screen.

### Typing your own name, which is the rest of that screen

`ChooseFIRE` at 0x16be does not finish a seat's turn. Each of its four arms writes the
knight's buffer into `NAMEy` (0x530, 0x51a, 0x546, 0x55c for knights 0 to 3), calls
`TypeName` at 0x13f0, and only then clears the knight's bit in `choose_knight` and writes
`[di+0x20]`. So the knight being named is still free, still drawn and still framed while
the typing runs, and `ChooseKnight`'s loop steps `choose_player` on only afterwards.

`TypeName` is small enough to quote whole:

```text
0x13f9  TypeFLAG = 1
0x13ff  ScanREF  = ChooseRefresh          ; link offset 0x164d
0x1405  CURSOR   = 0x5c
0x140b  SPACE    = index of the first space or NUL in [NAMEy]
ScanKEYS  0x142e  fire (bx & 0x10) -> NameDone
                  scancode 0x1c, Enter -> NameDone
                  scancode 0x0e -> BACKSPACE
                  ASCIIKEY; zero -> redraw and round again
                  SPACE >= 0xd -> beep, drop the key
                  else [NAMEy + SPACE] = the character; SPACE += 1
TypeREF   0x147a  Cursor; call [ScanREF]; round again
Cursor    0x14ac  [NAMEy + SPACE] = CURSOR
BACKSPACE 0x1485  [NAMEy + SPACE] = ' '; SPACE -= 1, floored at 0;
                  [NAMEy + SPACE] = ' '
NameDone  0x14bb  [NAMEy + SPACE] = 0; TypeFLAG = 0; AddClickSound
```

So the field is **thirteen characters**, the caret is `CURSOR` stamped into the buffer at
`SPACE` before every redraw, and the name is truncated at the caret when Enter or fire ends
it: backspacing the default away and pressing Enter really does leave a knight with no name.
`CURSOR` is 0x5c, which `TextASCII` sends to glyph 71 along with `'/'`, so it draws as the
stroke.

`ASCIIKEY` at 0x14e8 is `mov bx, ASCIIT; xlatb`, and `ASCIIT` at 0x14f2 is the scancode to
character table: uppercase throughout, no shift, and **the space bar maps to 0x5f**, the
underscore. That is the other half of why the default names carry one.

`ScanREF` holding `ChooseRefresh` is what makes the whole select screen redraw on every
keystroke, which is why the name can be drawn by the same routine that draws everything
else on it.

**A second triple has since turned up, and it is the armour rather than the glow.**
Item 75 read `ColourKnight`, which writes three 12-bit words into `BattlePal+12`, and
`BattlePal` is the arena palette, so **those three words are palette entries 6, 7 and 8**:
`0x00a`, `0x007`, `0x004` for the blue knight, `0xf80`, `0xc50`, `0xa30` for gold,
`0x8c6`, `0x593`, `0x251` for emerald, `0xf22`, `0xb22`, `0x700` for red, and
`0x206`, `0x103`, `0x001` for a fifth case. So **the original recolours a knight by
rewriting three palette entries, not by substituting pixels**, and `KnightGlowColours`'
brighter triple is what those same three entries pulse towards when he is nearly dead.
That is now how henge does it, for the knights and for the creatures alike; the hue
substitution that stood in for it is gone. Section 2.2 has the whole of `BattlePal`.

**The four knights do not differ in stats.** `InitKnights` gives each one a name, a colour,
one corner of the map at (10, 10), (300, 5), (26, 180) or (300, 185), and the same stat
block as the other three. `henge` keeps the four as data so they *can* differ; what ships
is what the original had.

**The names are recovered, and they were wrong here.** They are `BNAME`, `GNAME`, `ENAME`
and `RNAME` at image 0x128ca, 0x128e0, 0x128f6 and 0x1290c, twenty-one byte buffers padded
with spaces because `TypeName` lets a player type over them:

| index | ramp | symbol | string | corner |
|---|---|---|---|---|
| 0 | blue | `BNAME` | `SIR_GODBER` | (10, 10) |
| 1 | gold | `GNAME` | `SIR_RICHARD` | (300, 5) |
| 2 | emerald | `ENAME` | `SIR_JEFFREY` | (26, 180) |
| 3 | red | `RNAME` | `SIR_EDWARD` | (300, 185) |

The pairing is not an assumption. `InitKnights` branches on the knight's colour index at
`+0x20` and writes the name pointer and the corner together on each of its four arms, and
`ChooseFIRE` branches on the chosen portrait and writes `NAMEy` and `+0x20` together on
each of its four arms, and the two agree. The initial is the *colour's*: B blue, G gold,
E emerald, R red, which is why `SIR_GODBER` is the B.

**The underscore is a space.** `TextASCII` at `DS:0x8006` turns a character into a glyph
by `char - 0x20`, and `'_'` and `' '` both land on glyph 69, the blank; the font has no
underscore in it. The original stores one because `TypeName` finds where typing starts by
scanning the buffer for the first *space*, so the underscore keeps the whole default name
editable while the screen still reads `SIR GODBER`.

`SIR BANNER`, `SIR DWAIN`, `SIR BALAIN` and `SIR GUNTHER`, which this project called the
four knights until now, are `Enemy1Name`..`Enemy4Name` at image 0x18cd2. `InitGameStart`
writes them into the four knight records before anybody chooses, with colour index 4, the
dark purple, and the corners (15, 100), (300, 100), (160, 20) and (160, 180). A seat a
person takes is overwritten by `ChooseFIRE` and `InitKnights`; a seat nobody takes keeps
the enemy name and the purple. **So they are the computer knights' names**, and henge has
no computer knights to give them to yet: nothing uses them, which is the right amount of
use for them, and the four in `knights.json` are now the player names.

## 8.3 The status panel `done`

**There is no in-fight readout, and `Combat` is the proof.** The fight loop, `MOON:Combat`
at image 0x351, is ten calls long and every one of them is accounted for: 0x96e1 sets the
frame's target off the BIOS tick at `0:046c`, 0x5a24 waits for vertical retrace on port
0x3da, 0x4988 shakes the screen and runs `VBLQUE`, 0x5a66 copies two hundred rows of the
back buffer to the page, 0x9702 runs the ten `TaskTable` slots through `PerformCOMMAND`,
0x975b places them and their shadows, 0x5a3e flips the page by writing CRTC register 0x0c,
0x9f1d tests collisions, 0x8f8 is `KnightGlowOn` and 0x96f1 waits for the tick. Not one of
them draws a glyph or a rectangle, and the only thing `VBLQUE` ever holds is `ADDCOL`'s
palette upload at 0x4a3f (`ADDCOL`, 0x49b8, is its only filler). `DisplayKnight` (0xc0c2)
has three callers, `ReDisplay+72` and the two arms of `_displayknight`, all of them in
`_STATUS`; `ReDisplay` itself is called only from `ResetStatus`, `HotGadget` and the buying
routines, so nothing on this screen is reachable from a bout. A sweep of all 2,223 symbols
for energy, health, bar, hud, strip, gauge, meter, score, life, pip and vital returns one
name, `_MAP:HealLife`, the healer. The knight's numbers are this screen and nowhere else.

henge used to draw a plate per fighter across the deepest thirty two rows of every arena,
from row 168 down. That was ours and it is gone, and what it was covering was ground:
`Find3QuarterBORD` (0x29f7) stands an arrival with his feet on row 184 of a layout whose
tree line is at 119, and `CheckBorder` lets him walk them to 198.

**The screen is ivied stone arches and it has its own palette.**
`_STATUS:DisplayPillars` clears the screen to index 0 and falls into `StatusSetup`, which
walks eight-byte records `[cel][x][y][mirror]` until the first word goes negative, adding
`StatsOffset` to every x. `TradingData` (`DS:0xf37c`, seven records) has no terminator of
its own, so walking it walks `SingleData`'s eight as well and stops on that one's `ff ff`.
Fifteen cels of `KI.CEL`: cel 26 is a 25 by 117 pillar, and cels 34, 35 and 36 a column, a
corner and an arch head, each once and once mirrored. `ABorders` is a third table of the
same shape and it is the row labels, cels 38, 39, 40 at x 41 and 41, 42, 43 at x 89 on rows
35, 42 and 49, reading `STR :`, `CON :`, `END :`, `XP :`, `GOLD :` and `HIT :`.

**How many arches there are is the status type, and the plain sheet has one.**
`OffsetValues` is `{9, 3}`, and for those two `DisplayPillars` sets `StatsOffset` to `0x4a`
*and* starts the walk at `SingleData`, so it draws two pillars and one arch, shifted 74 to
the right: pillars at x 74 and 222 with the knight centred between them. Type 9 is the
plain character sheet and type 3 is the temple. Every other type takes both tables at
offset 0, which is three pillars at 0, 148 and 296 and two arches, one party in each; the
right hand one is then drawn at `StatsOffset` `0x96`.

henge drew the two-arch furniture on the plain sheet, which left an empty arch on the right
of every character sheet, and put a menu of its own in it: eleven rows of nine pixels
windowed on a cursor, with `Increase` gadgets and a line per carried item. Both are gone.
There is no menu on this screen in the original and there is nowhere for one to be.

**What replaces it is `SetUpStatus` (0xcf45) and `SetUpID` (0xd2b9)**, and they are not a
menu either. `SetUpStatus` fills seven parallel arrays of twenty seven string pointers,
`Inc`, `Use`, `Take`, `Purchase`, `Offer`, `Sell` and `Identify` at `DS:0xedc6` through
`DS:0xef0a`, one slot per thing a knight can have; `SetUpID` and `Paper2` (0xd3ca) pick one
array for each side of the screen on the status type and copy its pointers into `Response1`
(`DS:0xef40`) and `Response2` (`DS:0xf15c`) as ten-byte text records with x 0, y 3 and
flags 3. Every icon on the screen is registered as a gadget the size of its own cel by
`AddIconGadget`, whose text record is `RESP + (STID >> 1) * 10`, so `STID` is twice the
slot; `GadgetHit` draws that record, which puts the line **centred across the top of the
screen in the bold face**. Fire runs `HotGadget`, which decodes `STRP`'s low nibble: 5
cast, 1 take magic, 3 raise an ability, 0xa buy, and everything else through
`test ax, 0x20` to `TakeGold`. The permission is in the flag word: `or ax, 0x10` on the
left, `or ax, 0x20` on the right, both skipped for `Identify`, and `xor [di+6], 0x40` on an
ability row the experience can pay for. All of that is in `henge_core::status`.

**`DisplayKnight` falls through into `DisplayMagic`**, at 0xc38e, after
`push word ptr [si + 0x44]; pop word ptr [Address]`. So the potions, the gems, the rings,
the talismans, the five scrolls, the keys and the moonstones are part of every knight's own
sheet and were missing here entirely. The record they come out of is
`+0x00` potions, `+0x02` gems, `+0x04` the magic sword, `+0x06` rings, `+0x08` talismans,
`+0x0a` to `+0x12` the five scrolls in slot order (haste, hawk, aquisition, wyrm,
protection), `+0x14` the four keys as bits and `+0x16` the moonstones as bits.
`StatCheckKeys` (0xc44e) puts the keys on row **0x6f** at x 0x4c, 0x5e, 0x70 and 0x82 and
the moonstones all at x 0x67 on the same row; `StatPlaceScroll` (0xc633) lays the scrolls
along row 0xb7 from x 0x1e in steps of 0x19, and the step is inside the routine, so a
scroll you do not hold leaves no gap.

The palette is `STAPAL` (twenty eight words) plus `STAKNP` (four), and `ColourStatus`
overwrites the first two of `STAKNP` on `knight[0x20]`: `03f/028` blue, `fb0/b60` gold,
`4c3/160` emerald, `f00/800` red. Loaded, every cel on the screen draws in its own
colours, which is what makes the ivy green and the labels gold.

**A correction, and then a correction to it:** the life point figure is cel
`StatACEL + 0x11`. `ReDisplay` sets `StatACEL` to 0 before it draws the knight whose sheet
it is and `_displayknight` sets it to 1 for the second knight in the other arch, so on a
plain character sheet the cel is **17**, not 18, and 19 when `knight[0x3a]` is set. `KI.CEL`
17 and 18 are a helmed head, 19 and 20 a frog. This project drew 19 and then 18; it draws
17 now.

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

`KI.CEL` supplies the furniture at the cel numbers the fields index directly: frame 17 the
life figure, 21 the dagger, 22 to 25 the four swords, 27 to 30 the four suits of armour,
and 0 to 15 the moonstones, the ring, the potion, the keys, the gem, the talisman and the
five scrolls that `DisplayMagic` draws.
Frames 38 to 42 are five-pixel labels reading `STR :`, `CON :`, `END :`, `XP :` and
`GOLD :`, which is how the three ability rows and the right column are known; they are
anti-aliased across seven palette entries, so a silhouette of one is a smudge and the words
are set in type instead.

Everyone in an arena is a knight, so `henge` fights at the recovered scale and moves the
blow with it: twenty health and a five-point blow settles a bout in the same four swings a
hundred and twenty-five did.

## 8.4 The pointer and the gadgets `done`

**The original's pointer is driven by the stick.** `_STATUS:MovePointer` is nine lines:

```text
PointerFLAG = 1
read the stick
bit 0x01 -> x += 2      bit 0x02 -> x -= 2
bit 0x04 -> y += 2      bit 0x08 -> y -= 2
bit 0x10 -> PointerFLAG = 0        ; fire
clamp x to 0..0x13a, y to 0..0xc2
```

and the routine just below it, `SHOWPOINTER` at image 0xcf31, blits the art at that pair
with nothing subtracted, so the hot spot is the top left corner:

```text
push es
les si, ptr [0x892f]      ; PO.CEL's bank
sub ax, ax                ; cel 0
mov bx, [0xe492]          ; the pointer's x
mov cx, [0xe494]          ; and its y
call 0x5d7f               ; the cel blit every other sprite in the game goes through
pop es
```

`PO.CEL` is the art: one 16 by 18 arrow with a tail. **It is blitted in its own pixels like
any other cel**, with no ink and no substitution: the flat silhouette with an
eight-direction dark halo under it that used to stand here was invented to make a flattened
version of drawn artwork read, and `sprite::draw_mask`, which flattened it, now has no
callers at all.

**And the pointer is only on the screens that call that routine.** Scanning every call in
the image gives six: `MOON:WDLOOP+66` and `MOON:HWLOOP+66`, the two town menus;
`_TAVERN:TavernLoop+57`; `_WIZARD:DonateLoop+52`; `_STATUS:StatLOOP+19`; and
`_STATUS:FiDisplay+10`. `MovePointer` itself is called from `StatLOOP` alone. **The title
and the select screens are not in either list**, and neither has a gadget: `DoOptions` at
0x1241 and `ChooseLoop` at 0x15a0 poll the stick themselves (`test bx, 8` up, `test bx, 4`
down, `test bx, 0x10` fire) with no `CLEARGADGETS`, no `ADDGADGET` and no `CHECKGADGET`
anywhere in either. Both had a box per row here so a mouse could drive them, and both are
gone.

**The gadgets are a flat table of rectangles, not a widget kit.** `GadgetSlot` walks 98
records of twenty bytes for one whose width is zero, so 98 is the size and a zero width is
what empty means; `CLEARGADGETS` zeroes all 2,000 bytes. `AddIconGadget` fills a record:

```text
+4,+6   width and height, byte swapped out of the icon's own cel header at +0xe, +0x10
+8      a pointer to a ten-byte text record, at RESP + STID/2 * 10
+0xa,+c x and y, from STX + StatsOffset and STY
+0xe    the id, STID
+0x10   a payload word, STRP; +0x12 a second, STPL
```

So a gadget's rectangle is the rectangle of the thing drawn in it, and every gadget carries
the line it says. `CHECKGADGET` walks the live slots and asks the two-rectangle overlap
helper at image 0x9f0d, the same one `MOON:CheckGROOC` uses to decide whether a traveller
has arrived somewhere, one axis at a time, two passes is inside. The pointer's own
rectangle is **one pixel square** (`mov bx, ax; inc bx`). The helper is not symmetric, and
reproducing it as written gives `gadget.x <= pointer.x < gadget.x + width`.

`HotGadget` is what fire does with the gadget underneath: id 7 leaves the screen, a gadget
whose text record is `NEXT` moves to the next knight, and everything else is decoded from
the payload word's low nibble (5 cast, 1 take magic, 3 raise an ability, 0xa buy).
`AddClickSound` plays sample 0x0f.

**The payload is the original's now.** It used to be an id that whichever screen registered
the gadget interpreted, on the argument that this project's screens were already lists with
a highlight. They are not any more. A gadget carries `henge_core::status::Payload`, which is
`STRP` at `+0x10` and `STPL` at `+0x12`; `STRP`'s low nibble is the operation `HotGadget`
dispatches on and its high nibble is the bit within a field that holds several things, which
is how `StatCheckKeys` gives four keys one operation and four masks (0x11, 0x21, 0x41, 0x81)
and `DisplayMerchant` gives three armours one buy and three (0x1a, 0x2a, 0x4a). `STPL` is
the byte offset in the record the operation works on: 0x2e to 0x30 the abilities, 0x31 the
life points, 0x32 the purse, 0x34 the daggers, 0x40 the weapon, 0x42 the armour, and 0 to
0x16 the magic record's own fields.

`_WIZARD:DonateLoop` has a numbering of its own and reads `[si+0x10]` directly rather than
through `HotGadget`: 2 leave, 3 take it, 4 one coin off, 5 one coin on. That is
`henge_core::status::DonateOp`, and it is a separate enum for that reason.

The screens that lay gadgets out are the two towns, whose five boxes are recovered
(`MOON:InitHighWood` at 0xec9 and `InitWaterDeep` at 0xf4a), the donation bowl and the
character sheet. A place whose options are still a list of this project's own gets boxes
with no payload, which is what `HotGadget` would fall off the end of.

A real mouse moves it too, mapped back through the 4:3 letterbox, because a window with a
mouse in it should behave like one. `--point x,y` places it with no display at all.

**One correction while here.** The map's own arrival list, whose labels are
`_MAP:knhigh`, `knwater`, `knhenge`, `knmath`, `knvalley`, `knvillage`, `knlair`, `knkn`
and `kngrave`, is **not** a gadget screen. `_MAP:CreatePaper` draws a panel at
`PaperX`, `PaperY` = (50, 100) and prefixes each line with a digit starting at `KEYNUM` =
0x31, which is `'1'`. It is a numbered list you answer with the number keys, and no
`ADDGADGET` is anywhere in it. The gadgets all live in `_STATUS`. It is built now: see
5's *The paper, in full*, which also corrects the step between its lines. It is six
pixels and not the fifteen recorded here; fifteen is the single step `add [PaperY], 0xf`
takes at 0xaf0f, before the first line and not between them.

## 8.5 The message system `done`

**One record, one chain walk, three routines.**

```text
+0 u16  the text, a DGROUP offset, NUL terminated
+2 u16  x
+4 u16  y
+6 u16  flags: 1 centre, 4 right, 8 the bold face's kerning
+8 u16  the next record; 0 ends the chain
```

The routine at image 0x7a86 sets `TextTABLE` and falls into `GFX:TextPTop`, which applies
the alignment between `TextLeftBorder` and `TextRightBorder` and draws the line;
`TextPDone` reads `+8` and goes round until it is zero. That is `MESSAGE`, and sixteen
places call it, the dice table and `GadgetHit` among them.

The three kinds are three routines in `_LOADER`. All three blit `MESSAGE.PIV`, set the font
to `BOLD.F`, walk the chain and fade. They differ by one thing each:

| | image | takes | and then |
|---|---|---|---|
| `WAITMESSAGE` | 36524 | nothing | reads `WaitMES[WaitCOUNT]`, steps it, wraps at fourteen |
| `OCCURMESSAGE` | 36587 | a chain | the standard fade |
| `INSTRUCTMESSAGE` | 36631 | a chain | installs its own six-word palette ramp (0x800, 0x600, 0x400, 0, 0x200, 0x100) first, so it arrives in another colour |

**Which door shows which was recovered rather than assigned.** Scanning the code for calls
to each of the three gives every caller: `WAITMESSAGE` from `PracticeCombat5`,
`InitKnightvsDemon`, `SetUpDKL` and `_WIZARD:LoadWizard`; `OCCURMESSAGE` from `Valley`,
`KnightWonGame` and twice from the routine that opens a city; `INSTRUCTMESSAGE` from
`KnightProtection`, `CheckLairClear`, `FightDemon`, `Henge`, `bac` and the stone circle's
`noswap`. So the wizard's tower is the one door on the map that takes one off the wait
pile, the two cities greet you, and the circle instructs you, and that is what `henge` does.

`MESSAGE.PIV` turned out to be the stone circle in silhouette against a purple night sky
with black under it, which is exactly a box to write in.

**The text that survived, and the text that did not.** The fourteen wait chains, the city
welcome (`WelHigh1a`..`e`, one chain with the city's name swapped into its last record by
each caller), `_TAVERN:HengeWait` and `TitleMes` all read cleanly and are quoted verbatim
with their own coordinates. `HengeInstruct`, `GameOverMes`, `NoKeysMessage` and
`SHMES1`..`SHMES8` sit below `DS:0b5a`, inside the 2,906 bytes of DGROUP the unpacked image
held as a stale copy of another region, so **their words were not readable and none of
them is guessed at**. That span reads now (`REVERSING.md`) and these four have not been
re-read out of it.

Every one of the fourteen ends on `Loading...` at y 182 in the bold face, because in the
original the box is up while a disk is read. The table keeps that line and the drawing
leaves it out, because nothing here loads from a disk.

### `TextASCII`, which the project had reconstructed

`GFX:TextASCII` at image 107,446 is 95 bytes indexed by `character - 32`. It is the glyph
map this project read off the artwork instead, and **the reading was right**:

```text
A-Z -> 0..25   a-z -> 26..51   0-9 -> 52..61
!   -> 62      .   -> 64       ,   -> 65
#   -> 66      $   -> 67       %   -> 68
no glyph -> 69      '   -> 70      / and \ -> 71
```

One correction: the bold font's glyph 71 was read as a bar and is the slash, like the small
font's. One gap: **nothing maps to glyph 63**, so the `?` there is the one entry the table
cannot confirm and it stays a reading of the artwork.

The metrics came with it. `TextP` advances by the glyph's own cel width and nothing else,
except that `CheckBOLD` sets the flag word's bit 3 whenever the current font is `BOLD.F`
and `TextP` then takes three pixels off before drawing. So the bold face tracks three
tight, the small face not at all, and the space is glyph 69 like any other character:
fifteen wide in the bold bank, five in the small. Those are now what the pack says, which
is why a recovered line fits the screen it was written for.

## 8.6 The intro `done`

`INTR.EXE` unpacks with `tools/symbolmap.py` unchanged: four modules, **319 symbols**, 262
of them independently corroborated. **The image that tool writes, though, is still
packed**: its tail is Microsoft EXEPACK's run-length stream, so every zero-filled span of
the program is four bytes standing for hundreds. That one fact was the whole blockage, and
it explains both of the corrections the record carried against this executable and could
not account for: the code addresses looked as though they needed a fitted seven-step
correction because the image is short by exactly the fills that precede each step, and the
data addresses looked 14,911 bytes out because by `DGROUP` the fills have accumulated to
that. `henge_formats::introexe::expand` walks the stream, and afterwards **every symbol,
code and data, lands on its own byte with no correction of any kind**: `TextPTop` at
12,589, `PerformCOMMAND` at 16,332, `TextASCII` at 44,634, `MesFILE` at 46,007.

With the image expanded, everything that had been written off came out of it.

**`.STI` is a tile map, and the opening is a vertical pan.** `FindTile` cuts tile *n* out
of a 320x200 sheet at `((n % 10) * 32, (n / 10) * 25)`, the same 32x25 grid ten across a
`CMP` scenery sheet uses, and the routine at `0x0e6f` walks the map ten **big-endian** words
to a row, dividing each by 80 to choose between three loaded sheets. `INTRO.STI`'s 960
bytes are therefore 48 rows: **a 320 by 1200 panorama** out of `bg1a`, `bg1c` and `bg1b`,
which is what the three `panfile` symbols are for. The moon is at the top of it, the
treeline in the middle and a colonnade of trunks at the foot, and the intro moves a 200-tall
window down it from 0 to 1000 on a speed ramp of its own: eleven thresholds at `DS:0x124`
and eleven speeds at `DS:0x13a`, accelerating to six pixels a frame and easing back to one.
`INTRO1.STI` is not a tile map at all; it is byte for byte `F09.T` and `SW9.T`, a 105-byte
stub arena that ships three times.

**The captions have coordinates after all.** They are ordinary ten-byte
`[string][x][y][flags][next]` records, the same chain the message system walks; the flag's
bit 0 centres the line, which is why every x in the intro is zero. So the whole of the
guessing is gone: `MINDSCAPE PRESENTS` at y 20 with `copyright 1992` at 165, and the story
cards at 55, 75, 95, 115, 135 and 175.

**The credits are the loading screens, and the pairing is read rather than guessed.** A
seven-entry table at `DS:0x152` is stepped once per file the opening loads, so what had been
left as six names without headings is simply there: `conversion by` / Images Software Ltd,
`created by` / Rob Anderson, `Programmed by` / Anthony Mack and Nicholas Snape, `Artwork by`
/ Rob Anderson and Dennis Turner, `Music and Sound by` / Audio Visual Magic, `Additional Art
by` / Steve Leney, `Design by` / Rob Anderson and Todd Prescott. `Richard Joseph` and `Kevin
Hoare` are strings no record points at, so they are left out rather than placed.

**The story cards are not drawn over a plate.** `0x36c4` puts `MESSAGE.PIV` up first, the
same stone-circle box the game's own messages go over. The black bands this project used to
cut through the artwork were an invention of the missing coordinates, and there was nothing
to invent.

**The cast animates.** Its scripts are ordinary data in the intro's own `DGROUP`, and the
intro's `INITTASK` fills one handler slot more than the game's, so `TASKGOSUB` is `0x9a`
here and `0x98` there. Sixteen scripts drive the plates, using five of the twenty-one
commands and nothing else, and a `TASKGOTO` whose mode is not 3 arms a jump taken at the
*next* end of frame, which is what makes the scenery hold still while the one script with no
pending jump decides how long a scene lasts. So the scene lengths are the scripts' own tick
counts, not a choice.

**`MINDSCAP` is a PIV.** It has no extension, which is the only reason nothing had baked it:
the loop that turns full-screen images into sheets asks for `.piv`, `.cmp` and `.p`.

**The intro is the first half of `INTR.EXE` and the ending is the second.** The program
reads its command tail at `PSP:0x82` and jumps to a different sequence when it is given
one, and that is the half with `The End`, `And so, the tale of the Moonstone...`, `co.sti`
and the plates `bg5`, `bg7` and `bg8`. So the intro is five plates and a panorama, not
eleven, and the other three are deliberately not in it. `CO.STI` decodes as the ending's
own pan: 48 rows again, with the bottom eight a whole screen and everything above it one
repeated tile, so the camera rises off `bg7` into empty sky.

**And the argument is `MAIN.EXE`'s exit byte, which is now proved rather than assumed.**
The entry reads it as

```text
0x000c  mov ax, es:[0x82]      ; the two characters of the command tail
0x0010  sub ax, 0x3131         ; both are digits
0x0013  cmp al, 3; ja          ; and both are '1' to '4'
0x0017  cmp ah, 3; ja
0x001c  add ax, 0x101
0x001f  mov [0x12d1], ax
0x0063  cmp [0x12d1], 0; je 0x6d      ; no tail: the intro
0x006a  jmp 0xfa                      ; a tail: the ending
```

so `al` is the first digit and `ah` the second, each 1 to 4. Two routines read that word.
The one at 0x3b23, called from `ColourKnight`, branches on **`al`** and patches three
twelve-bit words into a plate's palette; the one at 0x3b9d, called five times from
`ColourMoonstone` and once from `ColourKnight`, branches on **`ah`** and patches four:

```text
ah = 1  0xe00 0x900 0x600 0x300     red
ah = 2  0x05d 0x028 0x016 0x003     blue
ah = 3  0xfa0 0xb40 0x930 0x710     gold
ah = 4  0x0c5 0x082 0x061 0x040     green
```

`MOON:KnightWonGame` puts the knight in the **high** nibble as 3 -> 1, 0 -> 2, 1 -> 3,
2 -> 4, and seats 3, 0, 1 and 2 are `RNAME` red, `BNAME` blue, `GNAME` gold and `ENAME`
emerald. **Four for four**: the second digit is the high nibble and it recolours the
winning knight in the ending, so `Tally::code` has somewhere to go after all. The first
digit is the low nibble, which is the moon, and it colours the stone.

The sequence itself is at 0xfa: the `CEREMONY` card at `DS:0x13b9`, then the plate loads at
0x3a00, then four scene routines at 0x547, 0x628, 0x6db and 0x778, then `The End` at
`DS:0x0002` held for 0x64 retraces. `And so, the tale of the Moonstone...` at `DS:0x1459`
is inside the last of the four. **Not built**: it wants `co.sti` baked as a second
panorama, the eight cast scripts those four routines spawn, and a mode of its own.

**Built:** the logo, the wordmark and the publisher's card, the seven credit screens, the
pan, and the plates in the order the scene routines hand them to the blitter, with the cast
running on the intro's own scripts and every caption at its own y, then the story card over
`MESSAGE.PIV`. Skippable with fire, and the title behind it.

**Ours, and marked so in `henge_core::intro`:** how long the logo and each credit screen is
held, because in the original each is up for exactly as long as the next file takes to come
off a floppy; the rounding of the intro's 9.1033 frames a second onto this engine's 70.0863
ticks, which is 7.70 and so eight;
and the dark ring drawn round a caption, which stands in for the glyph shading this engine's
silhouette text throws away. The ending's own sequence is recovered above but not built: a
win shows `VICTORY`, waits for fire and goes to the title, which is as far as it can go
while there is nothing to hand the exit byte to.

## 8.7 There is no save, and the serialisation is the test harness's `done`

**The original has none.** `MOON.CFG` is a sound-card profile, and among the 2,223 symbols
there is no slot, no file and no routine; the only `*Save*` hit is `SaveTYPE`, a task VM
opcode. A game of Moonstone is finished or abandoned in one sitting.

So **the save is gone from the game**. F5 and F9 are gone, there is no menu item, and
nothing on any screen reaches it. What is kept, and kept explicitly labelled, is the
serialisation as a **test harness feature reachable only from the command line**: `--save
<path>` says where, `--load` reads one at start, and `S` and `L` in an `--input` script
write and read one mid-run. That was worth keeping because a headless test can then pose a
run at day nine with a particular purse and a particular traveller's seed instead of
walking the whole way there every time.

It also earns its place as the determinism check. `henge-core` is deterministic by
construction and both generators carry their seeds inside the state, so a round trip
through text that comes back bit for bit and then goes on identically for five hundred more
steps is a real test of that, and that test is what the module is for:

```text
magic        "henge-harness"
format       an integer, bumped whenever a stored field changes meaning
run          the whole Run: purse, pack, knight, lairs, moon, seeds
travel       where on the map, what day, and the traveller's own seed
players      the title's settings
gore
wait_count   which of the fourteen the Gods say next
fingerprint  Run::state_hash mixed with Overworld::state_hash
```

**Three refusals, told apart on purpose, none of which loads half a run:** not a snapshot
(wrong magic), one this build cannot read (wrong format), and one whose contents do not
match its fingerprint. **No path of any kind is written into one**, and a test asserts it.
A snapshot is taken on the map and nowhere else, because inside a bout it would have to
carry every fighter's script pointer and the knives in the air.

The module is `henge_core::harness`, named so that nobody later mistakes it for a game
feature. Reading and writing the file is `henge-desktop`'s, because core does no I/O.

- [x] Title screen and attract mode
- [x] Character select: the four knights, and what they are
- [x] A real status panel, which is a screen of its own and the only place the knight's
      numbers are drawn. Nothing is drawn over a bout; see 8.3
- [x] Typing your own name over the knight's: `TypeName`, `ASCIIT`, `CURSOR` and the
      thirteen-character field. The panel's other pages are still not built
- [x] Mouse pointer and clickable widgets
- [x] The message system: three distinct kinds (wait, occurrence, instruction)
- [x] The intro sequence. `INTR.EXE`'s image expanded, the `.STI` tile map decoded, the
      pan, the cast, the credits and the captions' own coordinates all recovered
- [x] Save and load **deleted**: the original has none, and the serialisation is now the
      test harness's alone

---

# 9. Assets not yet used

| Asset | Frames | Used |
|---|---|---|
| creature sprite banks | 8 sets | all 8, through the task VM |
| `MI.C` map icons | 47 | 1 of 47 |
| `KI.CEL` UI furniture | 50 | 11: the status panel's labels, pips, swords and armour |
| `SEL.CEL` selection art | 6 | all 6 |
| `PO.CEL` the pointer | 1 | the one, and it is the pointer |
| `DICE.CEL`, `DICE.PIV` | dice game | none |
| `BLO.CEL` blood and gore | 41 | none |
| intro cast banks | 438 | five of the nine, on the intro's own scripts; the other four are the ending's |
| full-screen scenes | 32 | 27: five plates and a 320x1200 panorama carry the intro, the other plates the title and attract mode, `MESSAGE.PIV` is the message box, and `MINDSCAP` is the publisher's logo |
| sound samples | 49 | 4 |
| music | 18 files | all six tunes, from the six Roland drivers |

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
- ~~Music (2.5), gamepads (2.4), colour cycling and fades (2.2)~~ **all three done**;
  scrolling was settled and there is none

**Honest note on effort.** Steps 1 and 2 are research: they could take a day or a month,
and no amount of planning makes that predictable. Everything after them is construction and
estimable. The quest is neither, because it was never recovered, so it has to be designed
and playtested rather than ported.
