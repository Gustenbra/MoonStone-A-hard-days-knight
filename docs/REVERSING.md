# Reverse engineering notes

The game's logic lives in its executable, not in its data files. This is the record of
getting at it, and of where the trail currently stops.

## The executable is packed twice

Nothing could be read until it was unpacked, and the first attempt stopped one layer short.

`MAIN.EXE` is **PKLITE on the outside and Microsoft EXEPACK inside**. The PKLITE stub
decompresses to another packed program, whose own header carries the `RB` signature, a
`dest_len` of 0x2b83 paragraphs and the familiar "Packed file is corrupt" string.

`tools/unpack_pklite.py` peels the outer layer. `tools/symbolmap.py` continues through the
inner one and writes the true 178,224-byte load image. Both work by **running the
executable's own decompression stub under emulation** (Unicorn, 16-bit real mode) rather
than reimplementing either format. The stub is the specification, so executing it cannot
disagree with it.

```sh
pip install unicorn capstone
python3 tools/symbolmap.py MAIN.EXE symbols.json --image main.final.bin
```

The stopping rule is generic and works on other packed DOS binaries: halt when execution
enters memory the program wrote itself, which is what a self-extractor does when it jumps
into its payload. A packer that relocates its own decompressor first trips the same test,
so the first such jump is treated as a stage boundary and the run continues.

## Debug information was left in the build, addresses included

Appended after the image: a line-number table, a module table, and the names, including
the original source file paths.

**The module table decodes cleanly.** Sixteen-byte records of contiguous start and length
pairs, verified by each start plus its length landing exactly on the next start. So every
one of the eleven source modules has a known code range.

**There is a full name-to-address symbol table: 2,223 entries.** Eleven blocks, one per
source module, in link order, 16-byte aligned and separated by zero padding. Each block is
a run of records:

```
[u8 reclen][u8 kind][payload][u8 namelen][name]    reclen counts everything after itself

kind 0x05   payload = u16 offset, u16 segment, u16 type    data
kind 0x0b   payload = u16 offset, u8 0                     near label, module code segment
```

Addresses are link-time and relative to the start of the fully unpacked image. Code
offsets are the image offset directly, with a monotone step correction of seven steps
between 0 and -473 that `symbolmap.py` derives and applies. Data is `seg*16 + off`, with
DGROUP at 0x123b.

The correction, and the table itself, are corroborated rather than assumed. 1,778 of the
2,223 symbols check out independently: code symbols by landing on a branch target that
something actually calls, data symbols by being referenced from code, and the best of them
by content. `CelFile1` points at `KN1.OB`, `BloodFile` at `BLO.CEL`, `map` at `MAP.CMP`,
`Enemy1Name` at `SIR BANNER`. Those cannot be coincidence.

The eleven code ranges come out contiguous and in link order, which is a further check:

```
MOON    00a8-5a04   GFX     5a6e-80dd   _KBD    81d4-834d   _DOS    8450-84ec
_LOADER 86de-95a9   _EMM    96a2-96c1   _TASK   993d-a385   _MAP    a3c9-b1b0
_TAVERN b1cc-b68b   _WIZARD b6fc-bf1d   _STATUS bfbf-d6e1
```

### The earlier conclusion was wrong, and why

This file, `COMPLETE.md` section 1.2 and build-order item 20 previously recorded that no
name-to-address mapping existed, and listed everything that had been ruled out. That was a
real search, honestly reported, and still wrong: **it was searching the EXEPACK'd image.**
The symbol table was sitting in RLE-compressed bytes, which is exactly why it looked like
loose name strings with junk between them. Unpack the second layer and the structure is
plain.

Worth keeping as a note to whoever reverses the next DOS binary: a negative result about
file structure is only as good as your confidence that you are looking at the file.

## The same trick works on the music

The tune files were written off in `FORMATS.md` as "driver blobs with data welded into
executable code rather than a readable format", which is exactly right and turned out
not to matter. There is no format to parse, but there is a driver to run.

Eighteen files ship, which is `MusicTable`'s six tunes by three sound cards, and the
first letter of each name says which card it is for. Two of the three are hard: `a` is
an AdLib, so recovering it would mean an OPL2 emulator, and `b` is the PC speaker.
**The third is an MPU-401, and the bytes it puts on the port are plain MIDI.**

So `tools/tunes.py` loads an `RTUNEn.BIN` at a segment, sets `ds = cs` the way its own
first instructions do, enters it as an interrupt handler with `ah = 0` and then `ah = 1`
once per tick, and hooks the `in` and `out` instructions. The status port is answered
"ready" and the data port "ACK", and everything the driver writes to 0x330 is read back
as MIDI with running status. Note, channel, velocity, start and length, all six tunes.

The tick is not a guess: `Install_Timer` programs the 8253 with mode 3 and a divisor of
0x5555, and the handler's first two instructions are `mov ah, 1; int 60h`, so one call
of the driver is one tick of a 54.62 Hz timer and nothing else.

It is the same argument as the unpacker, one floor down. The driver is the
specification, so executing it cannot disagree with it.

## A trap in the recovered code addresses

The symbol table's code addresses take a seven-step monotone correction, which
`symbolmap.py` derives and applies. That makes each symbol land on the right byte of the
image, but it also means **a relative call that crosses one of those steps decodes to a
target that is off by the difference between the two steps**.

`COLCON` was where this bit. Its two calls into `GFX` decode to `0x5af2` and `0x5b32`,
and both land in the middle of an instruction, which reads at first like the image being
wrong. It is not: `COLCON` sits in a region corrected by -12 and its targets in one
corrected by -85, so both are 73 bytes high, and the real targets are `0x5aa9`, which
converts the palette to DAC bytes, and `0x5ae9`, which uploads them.

The fix when reading a call is to undo the source's own correction and then look for the
region the target lands in. Worth knowing before concluding that a byte is not where the
symbol says it is; every intra-module branch is exact, so the failure only shows up when
following a call between modules, which is exactly when it is least expected.


The 327 `PUBLIC` names (`LOADKNIGHT`, `CALCHIT` and the rest) live in a separate appended
blob and still carry no addresses. They are not needed: the 2,223 that do carry addresses
already cover the code.

## Animations are scripts, not frame lists

The symbol names give the architecture away. There are entries for sequence, goto, hold,
jump, loop, skip, flip, place, sort and collision operations, alongside an end-of-frame
marker and an animation-replace call.

So animations are **small scripts run by a task VM**, with jumps, loops and collision
hooks, and **characters are composed of several sprite parts per frame**. That is why
creature banks contain loose legs, torsos and heads rather than whole poses, and it is
why the combat reads as positional rather than as trading canned attacks.

### The interpreter has been found, and decoded

`PerformCOMMAND` at image offset `0x97fb`, with `PerformLOOP` at `0x97f2` just above it as
the loop head. It is unmistakably a bytecode fetch-and-dispatch loop. `di` is the task
record, `[di+2]` is the script pointer, and each pass reads one byte:

```
0xff          end of frame; the next byte says what happens after it
0xfd          script pointer <- TaskCommand[0x18]  resume after a jump
0xfe          script pointer <- TaskCommand[6]     resume after a loop
bit 0x80 set  call [TaskComTable + (op & 0x7f)]    a command, through a function table
otherwise     a six-byte sprite-part record
```

`TaskComTable` is at DS:0x9448, and the instruction that indexes it is literally
`mov bx, 0x9448` at `0x9832`. The symbol table and the code agree to the byte, which is as
good a confirmation of both as this work gets.

**The full opcode set, the operand widths and the frame record are recovered.** Nineteen
commands, three empty table slots, and a six-byte part record. `docs/TASKVM.md` has the
whole instruction set with what established each part; `tools/taskvm.py` reads it back out
of `MAIN.EXE` and disassembles any script.

### Absolute code addresses in the code are link-time, not image, offsets

`TaskComTable` is BSS, so it is 44 zero bytes in the load image; the handler addresses
exist only as immediates in `INITTASK`, which fills the table at run time. Taken as image
offsets, those immediates land mid-instruction and the table reads as nonsense.

They are **link-time offsets**, and need the same correction `symbolmap.py` already fits
for code symbols: a coordinate space up to 473 bytes longer than the shipped image,
changing in seven steps. Apply it and all nineteen entries land exactly on a routine entry
point; skip it and none of them do. The same correction turns every `TASKGOSUB` operand
into the exact entry of a named routine, and makes cross-region `call rel16` targets
resolve to symbols instead of to junk.

So the correction is a real property of the binary rather than an artefact of the fit, and
it applies to absolute code addresses baked into the code as well as to the debug info.
Anything that follows an absolute code address in this image has to apply it. Data
addresses do not need it: `seg*16 + off` with DGROUP at 0x123b lands on the right bytes.

### Animations are scripts in the data segment, named

The scripts are ordinary data in DGROUP, and the symbol table names 236 of them:
`Knight_SwSwing`, `Knight_SwWalkR1`, `Troll_Walk1`, `Balok_Blink`, `Dragon_Shadow`. Each
is a list of frames; each frame is a list of sprite parts and ends with `ff`.

Every one of the 236 parses end to end with no unknown opcode and terminates on `ff ff`.
(221 was the count until the fourteen `Mudmen_*` scripts and `Rat_TreeBrush` were
found; the prefix had been assumed to be `Mudman`, after `MudmanTABLE`.)
Every `TASKGOTO` and `TASKDEAD` target is the first byte of another named script, every
`TASKGOSUB` target the exact entry of a named routine: `KnightGruntSound`, `DrDropHead`,
`KnifeThrow`, `SetDecapFLAG`.

### The frame record, and how the earlier attempt was wrong

```
 0   u8   bank selector, low five bits, equal to the slot number times four
 1   u8   cel index within that bank
 2   i8   y offset
 3   u8   flags
 4   i16  x offset
```

The earlier guess, `[kind][image][x][flags][i16 y]`, had **x and y the wrong way round**:
the byte is y and the word is x. It also read `kind` as a plain bank number when it is the
slot times four, because the bank table holds far pointers four bytes apart and the
selector is used as a raw byte offset into it. Those two errors together are why
compositing produced a heap rather than a figure, and no amount of staring at the bytes
was going to separate them from a wrong guess about the record length. Reading the code
that consumes the record settled all of it in one pass.

### It was checked by compositing and looking

Which was the whole point. Walking `Knight_SwWalkOn` through the decoded opcodes and
drawing its parts out of `KN1.OB`..`KN5.OB` gives a coherent eight-frame walk cycle.
`Knight_SwSwing` gives a sword swing with its blade arc. Setting the facing to 3 mirrors
the figure correctly, parts still assembled, which is what confirms the `-(x + cel_width)`
term in the mirrored placement. The same code against the creature bank tables gives a
troll with its club, a trogg with its axe, a ratman, and a three-bank balok whose second
frame opens its jaws.

## The overworld is two byte grids and a rectangle

The map's rules turned out to be small and complete, and all of them are in `_MAP`.

### The terrain table

`_MAP:MapType` is **40 columns by 26 rows of bytes**, at image offset 125,890, one byte to
each 8x8 block of the 320x200 `MAP.CMP` picture. `_MAP:FindLandscape` reads one out and
stores it in the variable `MOON:ColourBackdrop` switches on, which is what gives the four
codes their meanings:

```
0  plain / glade    GL1..GL8.T over GLB1.CMP
2  forest           FO1..FO8.T over FOB1.CMP
4  swamp            SW1..SW8.T over SWB1.CMP
6  waste            WA1..WA8.T over WAB1.CMP
```

The index comes from `_MAP:CalcKnGrid` and `_MAP:GetIndex`. It reads the map token's width
and height out of the `MI.C` bank header, which are 8 and 10, and computes

```
grid_x = (x + width/2)  >> 3
grid_y = (y + height)   >> 3
index  = grid_y * 40 + grid_x
```

where `x, y` are `knight[0x5c]` and `knight[0x5e]`, the token's **top-left corner** on the
map. So the ground under a traveller is the ground under the middle of his feet. With `y`
bounded at 190 the row index reaches 25, which is why the table has a twenty-sixth row for
a picture only twenty-five rows tall.

**Checked by looking.** Drawing the grid's own boundaries over the map artwork puts them on
the treeline, the edge of the marsh and the mountain ridge, region for region. Fitting the
table against the picture at one row up and one row down also makes the within-region
colour variance worse, and reading two rows either way runs into bytes that are not one of
the four codes at all, which pins both the start and the length.

### Hard going, and nothing impassable

`_MAP:MapSLOW` is a second grid at image offset 124,890 on the same index, holding a
two-bit mask. `_MAP:CheckSLOW`, in full:

```
SlowFLAG = 0
if either map effect flag is set: return      ; the pair beside GemXY, one of
                                              ; which GEMEncounter tests
mask = MapSLOW[index]
if mask == 0: return
SlowDELAY += 1
if (SlowDELAY & mask) == 0: return
SlowFLAG = 1
```

and `_MAP:MapMovement` increments the step counter, sees `SlowFLAG`, and throws the
direction away. **So a refused step still costs the day**; hard ground is a tax on time,
not a wall. Mask 1 and mask 2 are half speed on different rhythms, mask 3 a quarter. Read
against `MapType`, all but eight forest cells are 1; the wastes are 0, 2 and 3 in roughly
equal measure, the 3s along the spine and the escarpments; the swamp is mostly open (89 of
220) with patches of all three; and the map's own top row is 3 and its side columns 2
whatever the ground. A step into `HawkBorders`' edge is counted by `MapMovement` too, since
the count comes before `FOLLOW` calls it.

The only hard limit is a rectangle. `_MAP:HawkBorders` clears whichever direction bit would
take the token out of `0..=310` by `0..=190`, using four literal comparisons against 0,
0x136 and 0xbe. That is one screen, which is what settles the scrolling question: `MAP.CMP`
is a single 320x200 image, `_MAP:SHOW` hands the position to the blitter with nothing
subtracted from it, and there is nowhere for the view to go. `_MAP:ScrollINPUT` reads the
keyboard despite its name.

### A place is a box, and two of them have coordinates

`MOON:CheckGROOC` decides whether you have arrived somewhere by overlapping two rectangles,
one axis at a time, through a helper at image 0x9f0d that counts the passes; two passes is
inside. Both rectangles come out of the `MI.C` bank by `MOON:GetWIDTH`: the place's icon,
and the traveller's own 8x10 token. There is no radius and no centre anywhere in it.

What it walks is `MOON:MapIconsTABLE`, records of `[u16 icon][u16 x][u16 y]` ending on a
negative icon, and what it pushes is a stack of `[x][y][kind]` at DS:043c that
`_MAP:CreatePaper` turns into the menu. The kind is the icon's frame number, and
`MOON:StackMessages` maps 0x15..0x22 onto "Enter Village", the two cities, Stonehenge, the
Valley of the Gods, Math the Wizard and a dead knight's grave. The four village kinds are
each gated on the knight's own index, so **a village belongs to one knight and only he can
enter it**.

`_MAP:KnightGoesToTown` carries the two cities as literals: (94, 47) for Highwood and
(297, 157) for Waterdeep, compared as grid cells (12, 7) and (37, 20). Those cells are
exactly what the index formula above makes of those pixels, which is a check on both.

### Part of DGROUP was not in the load image, and the unpack was the reason

**This is fixed, and the fix is the more interesting half of the story.**

What was seen first was real: the first **2,906 bytes** of DGROUP in the unpacked image
were a byte-for-byte duplicate of the region 0x5398 higher, which is real animation script
data, and the duplicate ended exactly where `MOON:LairFile`, the first initialised datum,
begins. Everything the symbol table placed below DS:0b5a therefore read as nonsense:
`SelectPAL`, `CCOL`, `CRText`, `MapIconsTABLE`, `LairLocation`, `LairType`, `ForestLairs`,
`Moons`, `XPlevels`, `ARR`, `BNAME`..`RNAME`. That was recorded here as uninitialised
storage the packer does not carry.

It is not uninitialised. **The emulated EXEPACK stub stops before it has finished.** Hook
every write the stub makes and the whole span from image 0 to 0x12f0a is never written at
all; the last thing the stub does before jumping to the program is a `rep movsb` of 21,400
bytes that leaves the destination pointer at 0x12f0a. Everything below that keeps what the
packed file had there, which is the *source* of that same copy, and a source copied 0x5398
upwards is exactly why the span reads as a duplicate of the region 0x5398 higher. The
duplication is a symptom of the truncation, not a property of the program.

Finish the stream and it all comes back. Microsoft EXEPACK is read backwards from just
below the `RB` header, past the 0xff padding: a command byte, a 16-bit count, and for a
fill one more byte; `0xb0`/`0xb1` fill, `0xb2`/`0xb3` copy, and bit 0 set is the last
command. Run to the end it terminates with source and destination pointers equal at
0x1544, which is the packer's own way of saying everything below is already in place.
`tools/symbolmap.py:finish_exepack` does exactly that, in Python, over the same memory the
stub was handed.

Three things say the result is right rather than merely different. It agrees with the
emulated stub byte for byte over all 100,646 bytes the stub *did* write. `CCOL`, the
select screen's four x positions, comes out 12, 88, 164, 240: four 64-pixel portraits on a
320-pixel screen with a step of 76. And `SelectPAL` comes out as 32 words every one of
which is a valid Amiga `0x0RGB`, whose entries the four portraits index into as blue,
gold, emerald and red, matching `KnightGlowColours` from a completely different part of
the image.

**Only DGROUP is taken from the finished stream.** The rest of the unwritten span is past
the end of the last code module, and the seven-step code correction fitted below is
precisely this same displacement, so correcting the code as well would move every code
address quoted in these documents by up to 473 bytes. That is a change of its own and is
not made here.

So the note that used to stand here, *a symbol having an address does not mean the bytes
at that address are the program's*, was right about this image and wrong about the cause,
and the better lesson is the older one two sections up: **a negative result about file
structure is only as good as your confidence that you are looking at the whole file.**
Twice now the answer has been that an unpacking stopped early.

What came back, and what has been read out of it: `SelectPAL` and `CCOL` are used by the
select screen; `CRText` is its heading, `Select a Knight`, centred at y 5; `ARR` holds the
title's four option rows at y 85, 110, 148 and 168; `Moons` is `45 47 46 48 49 48 46 47`;
`XPlevels` is `3 2 1 1`; `ForestLairs`, `LairLocation`, `LairType` and `MapIconsTABLE`
are the whole overworld, read out and baked (see below); and `BNAME`, `GNAME`, `ENAME`
and `RNAME` are `SIR_GODBER`, `SIR_RICHARD`, `SIR_JEFFREY` and `SIR_EDWARD`. Each of those
belongs to the subsystem that owns it.

### The overworld came back with it

Four tables in that span say where everything on the map stands, and the reading of each
is the code's rather than the bytes'. The lair initialiser's copy loop at image 0x1ea0
runs twenty four times over eighteen-byte records with `di` on `ForestLairs`, `bx` on
`LairLocation`, `bp` on `LairType` and `si` on `LairFile`, and each source is stepped by
the loop itself:

```
mov ax, [di] ; add di, 2 ; mov [si+0x02], ax   which CombatTable entry the guardian is
mov ax, [di] ; add di, 2 ; mov [si+0x04], ax   TotalMonsters
mov ax, [bx] ; add bx, 2 ; mov [si+0x0a], ax   x
mov ax, [bx] ; add bx, 2 ; mov [si+0x0c], ax   y
mov ax, [bp] ; add bp, 2 ; mov [si+0x0e], ax   the landscape ColourBackdrop is given
mov ax, [si] ; add si, 2 ; mov [si+0x10], ax   the arena layout
```

So `ForestLairs` is 24 pairs of words, `LairLocation` 24 pairs and `LairType` 24 single
words: 96, 96 and 48 bytes, which is what the symbol table gives for their sizes, and
nothing about the layout had to be guessed from the shape of the data.

`MapIconsTABLE` is read the same way, by `MOON:CheckGROOC` at image 0x70b: three words a
pass, stop on a negative first word, hand the three to the overlap test as icon frame, x
and y. Sixty bytes is nine records and the terminator, and the nine are the four villages,
Highwood, Waterdeep, Stonehenge, the Valley of the Gods and Math's tower.

**Three things say the result is the map and not a coincidence.** `LairType` comes out six
2s, six 6s, six 4s and six 0s, the forest, waste, marsh and glade order `LairFile` already
gave, arriving a second time from a second table. Twenty three of the twenty four
`LairLocation` pairs land on a `MapType` cell whose code is that lair's own `LairType`,
which is a third table agreeing; the odd one, lair 15, is a cell into the treeline and is
still fought in the marsh, since `InitLair` hands `ColourBackdrop` the record's landscape
and never asks the map. And drawn on the map picture, Highwood's box covers the castle,
Waterdeep's the walled town in the marsh, Stonehenge's the stone circle in the southern
woods and the wizard's the lone dark tower in the northern waste, to the pixel.

The last of those is what corrected two mistakes this project had made and could not have
caught any other way. It had put its healer on the ruin in the southern woods and its
Stonehenge on the ring in the middle of it all. The ruin *is* Stonehenge; the ring is the
Valley of the Gods. Both were sited on real artwork under the wrong name, which is a
failure mode worth naming: **artwork will confirm that a place exists and will not tell
you what it is called.**

### The text records in that span, and what they settle

`MOON:OPT1a` and `MOON:CRText` are message records, the ten-byte kind the walker at image
`0x7a86` follows: `{text, x, y, flags, next}`. The layout is not a guess about the shape.
The walker reads `mov dx, [bx]` for the string, `[bx+2]` into `TextX` and `[bx+4]` into
`TextY`, tests `[bx+6]` for bit 0 (centre between `TextLeftBorder` and `TextRightBorder`,
which are 0 and 320) and bit 2 (right-align), and takes `[bx+8]` as the next record or
stops on zero. Every chain in the span walks to a terminator and every string pointer in
every one of them lands on a NUL-terminated line that reads as English, which is the check
that the layout is right.

The single-line entry point at `0x7a70` settles it outright. It builds one record at
`DS:0x7ff2` out of its arguments and falls straight into the walker:

```
0x7a70  mov di, 0x7ff2
0x7a73  mov [di], si          ; text
0x7a75  mov [di+2], ax        ; x
0x7a78  mov [di+4], bx        ; y
0x7a7b  mov [di+6], cx        ; flags
0x7a7e  mov word [di+8], 0    ; next
0x7a83  mov si, 0x7ff2
```

So the field order is not inferred from the data at all; it is written out one register at
a time. `ChooseRefresh` is one of its callers: `mov si, [NAMEy]; mov ax, 0x32;
mov bx, 0x32; xor cx, cx`, the chosen knight's name at (50, 50) with no flags, drawn only
while `TypeFLAG` is set, which is while `TypeName` has the caret in it.

Two things this settles that were previously paired by content:

* the title's option list is `Players`/`Gore` at x 86 with their values at x 214, and
  `Practice`/`Select Knight` centred, at y 83, 108, 150 and 170. All six strings are here
  too: `Sel1`, `Sel2`, `Sel5`, `Sel6`, `NPLAYER` and `TEXTON`/`TEXTOFF`
* `GameOverMes` is **two** records, `GOmes1` `GAME OVER` centred at y 95 and then
  `Press fire to continue` at y 180. The string `Player      ` sits nine bytes before
  `GOmes1` and `henge` had paired the two, on the reasonable guess that the original
  printed a player number into it. It does not: nothing anywhere in the image refers to
  that string's address, and `GameOverMes` starts at `GOmes1`. `NoKeysMessage` and
  `ValleyEnter` both end on the same `Press fire to continue` record, which `VICTORY`
  does not

### The glyph map, and why the knights have underscores in their names

`TextASCII` at `DS:0x8006` is 96 bytes indexed by `char - 0x20`, and both `TextP` and
`TextLen` go through it. It has 72 glyphs: A-Z at 0, a-z at 26, 0-9 at 52, then `!`, an
unused 63, `.`, `,`, `#`, `$`, `%`, the blank at 69, `'` and `/`. Everything it has no
glyph for, `'_'` included, maps to 69, the blank. So `SIR_RICHARD` draws as `SIR RICHARD`.

The underscore is there on purpose. `TypeName` starts the caret by scanning the name
buffer for the first *space*, so a name written with spaces would only be editable from
`SIR` onwards; written with underscores the whole eleven characters are, and the screen
still shows a space.

### The arena tables

Four tables of eight in `_LOADER`, each entry a pointer to a filename, with a counter
beside it:

```
PlainTable   GL1.t..GL8.t     counter DS:8995
ForestTable  FO1.t..FO8.t     counter DS:8997
SwampTable   SW1.t..SW8.t     counter DS:8991
WasteTable   WA1.t..WA8.t     counter DS:8993
```

The loader for a family reads `Table[counter]`, loads it, then `inc counter` and
`and counter, 7`. **A rotation, not a roll.** The four counters are the `PLAINCOUNT`,
`FORESTCOUNT`, `SWAMPCOUNT` and `WASTECOUNT` publics.

Beside them, `TileTable` is four words indexed by the same landscape code and gives the
scenery sheet: `FO1.CMP` for plain *and* forest, `SW1.CMP` for swamp, `WA1.CMP` for waste.
`SetTileSheet` (image `0x8dec`) tests its argument against 4 first and keeps `FO2.CMP`
when it matches, and the argument is the placement's first byte, filtered by `Sholoop`
(`0x7ca9`) on its way in: **0xff ends the list, 0xfe is skipped without drawing anything,
3 goes through to the table, and every other value is forced to 4.** That was read out of
the walk itself rather than guessed from what looks coherent, and it corrects two things
this project had wrong. 0xfe does not draw from the family's sheet, it does not draw at
all, and the 168 records that carry it are half the furniture of the lair arenas. And the
six records that carry 1 draw from `FO2`, not from the family's sheet.

**The walk is the whole of the scenery, and it is not sorted.** `Sholoop` steps the
records six bytes at a time in file order, and `Dump_Tile` stamps each cell into the
compose page at `0xac00`, which the fight loop copies back over the screen every frame
before the actors go on top. Nothing about scenery is per-frame and nothing about it
sorts against a fighter. `PlaceTile` clamps rather than clips, and `docs/FORMATS.md` has
the two clamps and the missing right-edge test.

### Nothing walkable is a rectangle, and the tree line is a border list

This is the arena's own half of the same idea the overworld solves with `MapSLOW`, and
this project had it inside out until it was read properly. **The `.T` header is a count
and that many eight-byte rectangles, each of them ground a fighter may not stand on**, and
the walkable ground is what is left below them. `docs/FORMATS.md` has the layout and the
four layouts that carry more than one rectangle.

Two routines share the work, and neither of them clamps a coordinate. Both clear bits in a
per-actor byte at `+0x26` which holds **the directions still allowed this frame**: bit 0
right, bit 1 left, bit 2 down, bit 3 up. `ControlKnight` (`0x3ec4`) fills that byte from
`GetInputDevice` at the top of the frame, and if it comes back empty the frame ends there.
Bit 4 is fire.

**`CheckBorder`, image `0x40d0`**, is the global limit and is the same in every arena:

```asm
mov ax, 0x19            ; 25
test byte [si+8], 2     ; facing: 1 right, 3 left, bit 1 is left
je +4
neg ax
add ax, [si+2]          ; ax = anchor x, 25 ahead of himself by his facing
mov bx, 9
add bx, [si+6]          ; bx = anchor depth + 9
cmp ax, 0x0a  / jge  ->  and [si+0x26], 0xfd ; no left    / mov word [si+2], 0x0a
cmp ax, 0x140 / jle  ->  and [si+0x26], 0xfe ; no right   / mov word [si+2], 0x140
cmp bx, 0x9b  / jle  ->  and [si+0x26], 0xfb ; no down
cmp bx, 0x1e  / jge  ->  and [si+0x26], 0xf7 ; no up
```

The x tests **write the coordinate back**, and to the raw limit rather than to where the
probe stopped, so the twenty five pixel lead decides *when* a fighter is stopped and not
*where*: a knight walking left is refused at column 34 and put at 10, and one walking right
is refused at 296 and put at 320. `KnightSLAP`, the knockback, clamps the task's own x to
the same 10 and 320, which is the second witness that those two are meant as resting
columns. So the facing offset does not leave a fighter standing in a different column
depending on which way he faces; both ways he ends on the wall.

**`SBORD`, image `0x4552`**, walks the arena's own list. `es` comes from DS:`0x88ff`, the
segment the `.T` was loaded into, the count is the big-endian word at `es:0`, and the
records follow it. Per record, with `ax` the step's dx and `bx` its dy:

* the actor's body box `[+0x22, +0x24]`, moved by dx, against the record's `left`/`right`
  through the overlap counter at `0x9f0d`. No overlap, next record;
* the body box's lowest row `+0x50` against the band from 30 to the record's `bottom`. If
  it is inside, the sideways step is refused: facing left clears bit 1, facing right clears
  bit 0. Both refusals are guarded by a comparison of the moved box against the unmoved
  one that is true for any step shorter than the actor is wide, so in practice a fighter
  whose feet are inside a rectangle cannot walk sideways at all;
* `bottom` against `[si+6] + dy + 0x2f`. At or below it, bit 3 goes and he may not walk up.

`top` is loaded and then thrown away (`mov bx, es:[di+6]` immediately followed by
`mov bx, ax`), and the constant 30 stands in for it. It is 10 in every shipped record, so
nothing depends on the difference.

The depth both routines test is the **task anchor** rather than the feet. The knight's
standing frame hangs from `-9` to `+45` about that anchor with a shadow to `+52`, so
`+0x2f` lands five rows above his feet and `+9` lands forty three above them. The three
places `AddKnight` stands an arrival, through `FindQuarterBORD`, `FindHalfBORD` and
`Find3QuarterBORD`, are a quarter, a half and three quarters of the way from the deepest
`bottom` down to row 200, less that same `0x2f`. Row 200 is the foot of the screen, so the
original fights over the whole of it and has no status panel along the bottom at all.

**Only the knight is bordered.** `SBORD` has exactly one caller, `ControlKnight`, and
`CheckBorder` has three, all of them the knight's: `ControlKnight` and the two halves of
`KnightSLAP`. `MonsterWalk` (`0x4e8b`) calls neither, and DS:`0x88ff` is read at combat
time by `SBORD` and by nothing else. In the DOS game a troll walks through the tree line.

Henge used to run every fighter through the same gate. That was ours, and it was a bug, and
it is gone. It is worth recording what it cost, because the symptom looked nothing like its
cause.

The spear trogg appeared to freeze. Inside his `back_off` (`SetTroggSpTables` at `0x2220`
gives him approach `0x82`, back off `0x78`) `MonsterTrack`'s `TrackBack` (`0x5763`) hands
him a step *away* from the knight, so a person who walked up to him pushed him backwards.
Once that carried him out of the borders' idea of the arena, our gate refused him every
direction at once, `moved` came back false, and the translation of `0x4eaa` -- reset the
walk cycle, play the stance -- put him in his stance, where he stayed for the rest of the
fight. He was not stuck in any state machine. He was walking into a wall we had built.

`MonsterWalk` (`0x4e8b`) is the whole of a creature's gate and it is four instructions long
in substance: `NextWalk`, `TASKWALKCOLLIDE` at `0x4e9e`, `and [si+0x26], ax`, then
`add [si+2], ax` and `add [si+6], ax` at `0x4eeb` and `0x4ef1`, and out to `0x2d52`. No
border test, and **no clamp on the result**. `ControlBlackKnight` jumps into the same
routine at `0x4c10`, so a computer knight is not bordered either.

Nor is anything clamped after the add, for anybody. `ControlKnight` adds its own step at
`0x3fdc`, `0x3feb`, `0x3ffa` and `0x4009` and falls straight out. `CheckBorder`'s write-back
(`0x40ed`, `0x40fb`) is the whole of the knight's bound and it is taken *before* the add, off
the probe, which is why a knight pushed into the limit comes to rest at exactly `X_LOW` or
`X_HIGH` rather than 25 short of it. Scanning every instruction in the image that writes an
actor's column settles the rest: `mov [si+2], imm`, `add`, `sub` and `mov [si+2], r` between
them give exactly two routines that bound one, `CheckBorder` and `KnightON` (`0x5289`, on
arrival, and it bounds `[si+4]`), against four movement sites that add without a bound --
`MonsterWalk` `0x4eeb`, `DemonMove` `0x5004`, `MudmenMove` `0x53fb`, `MudmenAppear` `0x5492`.
The task VM writes no actor column at all: a scripted lunge moves the *task*, and the
controller owns the anchor. So `run_task`'s clamp was ours too, and it was the visible half
of the freeze -- it pulled a fighter who had walked past the edge back two pixels on every
script frame, which is the twitching the person saw before the stance took over.

The seat tables corroborate all of it beyond argument. `TroggTABLE` puts one creature at
column -50 and another at 360, both outside `X_LOW`..`X_HIGH`, and a creature the borders
held could never walk in from either. Note also that `CheckBorder` writes back only the
column: its two depth tests at `0x4100` and `0x410a` clear direction bits and nothing more,
so even the knight's depth is never snapped.

A creature that walks off-screen is not lost. Once the knight is further away than
`approach`, `MonsterTrack` hands the creature a step toward him again, so he walks back in.
That is the original's only restoring force and it is enough.

`bord` (`0x5828`) is not part of any of this: it writes attribute controller register
`0x11`, the overscan colour.

### Every write to an actor's facing, found by scanning rather than by reading around

`+8` is one byte, so every store to it is `C6 /r 08 imm` or `88 /r 08`, and scanning the
whole image for those two shapes gives the complete list. There are twenty one immediate
stores and eight `mov [reg+8], al`, and no other instruction anywhere touches the byte:

```
01da0 InitGameStart+403   02019 InitPractice+43     0207a InitKnightvsKnight+32
02485 InitKnightvsDragon  02573 SetUpDragonTables   025f7 SetBalokTables+34
027ab InitKnightvsDemon   0297d SetKnightCombat+27  02ff8 BeastCharge+18
03009 BeastChargeLeft+11  035fe ControlBalok+101    03604 ControlBalok+107
03c1f TrackKnight+55      03d05 FaceKnight+18       03d0c FaceKnight+25
03f77 ControlKnight+179   03f86 ControlKnight+194   043ed ClawStruck1+26
05480 MudmenAppear+21     0548b MudmenAppear+32     079c9 FlipXL+189 (a blit record)
02811 InitNewMO+35        03d31 FlipKnight+30       04282 BalokStruck1+12
04384 DemonSlap+5         044e5 KnightSLAP+19       099d5 perdone+24
09a85 TASK_FLIP+24        0a607 ContinueDragon+84
```

Three of them were missing from the account of the chain and are now built: `FlipKnight`
(reached only from `RatmanHit+49`), `DemonSlap` and `ClawStruck1+26`. One of them is
unreachable and it is worth saying why, because the listing reads as though it turns the
knight: `BalokStruck1` is

```text
04276  mov word [si+0x28], 4
0427b  jne 04285
0427d  mov al, [si+8]
04280  xor al, 2
04282  mov byte [di+8], al
04285  sub word [di+0x38], 5
```

and the only instruction before the `jne` that sets flags is `add bx, ax` at 0x4272 in
`KnightGotStruck`, where `bx` is `0x7843` and `ax` the striker's kind. That sum is never
zero, so the jump is always taken. A balok's slap does not turn the knight.

`KnightSLAP+19` (`044e5`) sets the slapped knight's `+8` from DS:0x7832, the direction the
slap goes. It is built now, with the rest of the slap: `Bout::knight_slap` off the
`InitSLAP` and `KnightSLAP` gosubs, which used to be named in `taskvm.rs` as `TASKGOSUB`
targets and no more.

### The fight loop draws no readout, and it is short enough to say so exhaustively

The question this settles is whether the original puts anything over an arena. It does
not, and `MOON:Combat` at image `0x351` is the whole of the argument: ten calls, a test of
`DS:0x897d` and a decrement of `DS:0x8987`, and back to the top.

```text
0x96e1  read the BIOS tick at 0:046c, target = tick + 2
0x5a24  wait for vertical retrace on port 0x3da
0x4988  ShakeScreen's tail, then walk VBLQUE calling each entry
0x5a66  copy 200 rows of 0xac00 to the display segment, all four planes
0x9702  walk the ten TaskTable slots through PerformCOMMAND
0x975b  place the parts and the shadows
0x5a3e  advance ClipYOffset by 0x400 and write CRTC register 0x0c: the page flip
0x9f1d  TaskCol, the collision pass over the same ten slots
0x8f8   KnightGlowOn
0x96f1  spin until the BIOS tick reaches the target
```

Three of those targets need the correction from the section above, since `Combat` sits in
the region shifted by 0 and the task and video routines in regions shifted by -473 and
-12. The two things that could have hidden a readout are `VBLQUE` and the task loop.
`VBLQUE` is filled by exactly one routine, `ADDCOL` at `0x49b8`, and the only address it
ever appends is `0x4a3f`, a palette upload. The task loop draws sprite parts and nothing
else. And `DisplayKnight` (`0xc0c2`), which is where the knight's numbers are drawn, has
three callers, `ReDisplay+72` and the two arms of `_displayknight`, all three inside
`_STATUS`, and `ReDisplay`'s own callers are `ResetStatus`, `HotGadget`, `DoneCast` and
the buying routines. Nothing on that screen is reachable from a bout.

The symbol sweep agrees: of the 2,223 names, one matches energy, health, bar, hud, strip,
gauge, meter, score, life, pip or vital, and it is `_MAP:HealLife`.

## `INTR.EXE` unpacks the same way, and it is the same program

The intro is a separate executable and had never been looked at. It is packed identically,
and `tools/symbolmap.py` reads it with no change at all:

```sh
python3 tools/symbolmap.py INTR.EXE intro-symbols.json --image intro.final.bin
```

```text
image 56128 bytes, 4 blocks, 319 symbols
  module0    131 symbols  code 0fe4-35ca
  module1     95 symbols  code 369e-3d3e
  module2     73 symbols  code 3de2-43f9
  module3     20 symbols  code 447e-453a
262/319 symbols independently corroborated
```

**It is built from the same source modules.** Module 2 is `_TASK`: `PerformCOMMAND`,
`PerformLOOP`, `TaskGoto`, `TaskHold`, `TaskGosub`, `TaskCelBuf`, `TaskTestEq`,
`TaskCommandTable`, name for name with `MAIN.EXE`'s. Module 0 is `GFX`, with `TextPTop`,
`TextP`, `TextLen`, `CheckBOLD`, `TextASCII` and, beside them, a tile engine `MAIN.EXE`
also carries: `PlaceTile`, `FindTile`, `CalcOffset`, `ClipTile`, `Dump_Tile`, `TileScreen`,
`TileNum`, `TileX`, `TileY`. Module 1 is `_LOADER` and module 3 the DOS error table. So the
intro is not a separate engine; it is the game's engine with a different program on top.

### The image the tool writes is still packed, and that was the whole blockage

`symbolmap.py` resolves a data symbol as `seg * 16 + offset`, which is right for
`MAIN.EXE`. In `INTR.EXE` it appeared to land 14,911 bytes past where the bytes actually
are, and the code addresses appeared to need the same seven-step monotone correction the
tool fits for `MAIN.EXE`. Both readings were recorded here as facts about the executable.
**Neither is one.** They are two symptoms of the same thing:

> **The `--image` `symbolmap.py` writes for `INTR.EXE` has not been fully unpacked.** Its
> tail is Microsoft EXEPACK's run-length stream, one command every few hundred bytes, and
> every zero-filled span of the program is sitting there as a four-byte `fill` command
> instead of the bytes it stands for.

```text
read backwards from the last command byte:
  b0 / b1   fill:  [u8 value][u16 count][cmd]
  b2 / b3   copy:  [count bytes][u16 count][cmd]
  bit 0 set on the command is the last one executed, and it is the lowest in the file
```

The code region is nearly all `copy` blocks, which is why it disassembled correctly and
why the correction the tool fitted was monotone and stepped: each step is one `fill` the
image is short by. By `DGROUP` the fills have accumulated to 14,911, which is the other
number. Expand the stream and **every symbol, code and data, lands on its own byte with no
correction of any kind**:

```text
TextPTop        12,589      PerformCOMMAND  16,332
LoadScreen      14,156      TaskGosub       17,170
TextASCII       44,634      MesFILE         46,007
```

`henge_formats::introexe::expand` does the walk; it finds the end of the stream rather than
assuming it, because the debug information is appended after it.

**`MAIN.EXE` was then checked, and it does not have the same fault.** This was the obvious
next question, because if it did then its fitted code correction would be an artefact. It
is not so, on two independent tests. Running the same expander over `main.final.bin` finds
no real stream: what it matches would make the file *shorter*, which a run-length expansion
cannot do. And every one of the 411 data symbols that names a string has that string
sitting at exactly the address the symbol table gives, in the unexpanded image, with none
wrong. The image is already the program.

So the two executables genuinely differ: `INTR.EXE`'s image needed expanding and
`MAIN.EXE`'s did not, and `MAIN.EXE`'s seven-step code correction stands, corroborated as
it was by all nineteen `TaskComTable` handlers landing on entry points with it and none
without it.

The question it was really asking, though, was whether the bottom of DGROUP could be
recovered, and the answer to that turned out to be yes by another route entirely: the
EXEPACK stub is not run to the end. See *Part of DGROUP was not in the load image* above.
The correction and the truncation are two faces of one thing: the seven steps of the
correction are the displacement of the copy the stub stopped in the middle of.

Worth keeping beside the earlier note about the symbol table: **a negative result about
file structure is only as good as your confidence that you are looking at the file**, and
that applies to an unpacked image as much as to a packed one.

### What its symbols and its data give, once the image is expanded

**The asset list, by name and in order.** The numbering is a real ordering, not an
artefact: the filenames are not in alphabetical order and the symbols are.

```text
panfile1..3   bg1a.piv  bg1c.piv  bg1b.piv
picfile1..8   bg4.piv  bg5a.piv  bg3.piv  bg2.piv  bg2a.piv  bg5.piv  bg7.piv  bg8.piv
cast          au1.cel  li1.cel  da1.cel  dw1.cel  ha1.cel  ov1.cel  co1.cel  dg1.cel
              klift1.cel
also          bold.f  message.piv  mindscap  blo.cel  be1.c
mapfile1,2    intro.sti  co.sti
```

**`.STI` is a tile map.** `FindTile` cuts tile *n* out of a 320x200 sheet at
`((n % 10) * 32, (n / 10) * 25)`, which is the 32x25 grid ten across that a `CMP` scenery
sheet is cut on, and the walker at `0x0e6f` reads ten **big-endian** words to a row and
divides each by 80 to choose between three loaded sheets. `INTRO.STI`'s 960 bytes are 48
rows: a 320 by 1200 panorama out of `bg1a`, `bg1c` and `bg1b`. `docs/FORMATS.md` has the
format. `INTRO1.STI` is not a tile map: it is byte for byte `F09.T` and `SW9.T`.

**The opening is a vertical pan down that panorama**, 0 to 1000, on a ramp of eleven
thresholds at `DS:0x124` and eleven speeds at `DS:0x13a`: one pixel a frame, up to six,
and back to one. `0x0f0d` and `0x0f57` scroll what is on the screen by whole pixel rows and
stamp two fresh tile rows into the edge they uncover.

**Its captions are ten-byte records**, the same `[string][x][y][flags][next]` chain the
game's message system walks, and bit 0 of the flag word centres the line, which is why
every x in the intro is zero. Nothing points at the *strings*, which is why the earlier
search for them failed; the records point at them.

**The credits are the loading screens.** A seven-entry table at `DS:0x152` is stepped once
per file the opening loads, so the heading-to-name pairing this project had refused to
guess at is simply read off: `Programmed by` with Anthony Mack and Nicholas Snape,
`Music and Sound by` with Audio Visual Magic. `Richard Joseph` and `Kevin Hoare` are
strings no record points at.

**The intro's task VM is the game's with one slot more.** Its `INITTASK` at `0x3be8` fills
`TaskCommandTable` with twenty-one handlers where the game's fills nineteen, inserting
`TaskStop` at `+0x14`, so every opcode above it moves up one: `TASKGOSUB` is `0x9a` here
and `0x98` there. Its scripts use `TASKHOLD`, `TASKGOTO`, `TASKLOOP`, `TASKGOSUB` and the
part record and nothing else, and a `TASKGOTO` whose mode is not 3 arms a jump taken at the
*next* end of frame, which is how a script that finishes on `ff ff` carries on anyway.
That is what holds the intro's scenery still while the one script with no pending jump
decides how long a scene lasts.

**The intro is the first half of the program and the ending is the second.** `0x000c` reads
the command tail at `PSP:0x82`; given one, `0x006a` jumps to a different sequence, and that
is the half with `The End`, `And so, the tale of the Moonstone...`, `co.sti` and the plates
`bg5`, `bg7` and `bg8`. `ColourMoonstone` reads the same argument to colour the stone, so
the argument is which moonstone the game was won with.

## The sprite blitter, and what `blit_mask` really is

`GFX` names twenty-two routines `do_1` through `do_20` after the mask byte each one
serves, and `plane_tab`, thirty-three words in the code segment indexed by that byte,
is how one is chosen. That table is the whole answer to a field this project had been
guessing at.

**`blit_mask` is a bit set, not a bit length.** A frame stores one plane per set bit and
stored plane *i* goes to the mask's *i*th set bit; the bits the mask leaves out are
zeroed. `do_17` says so in six instructions: four `mov`s through `si`, `di`, `bx` and
`bp`, an `xor cl, cl` where bit 3 would be, four `inc`s, and then the same five
`rol`/`rcl` pairs every routine ends with. Eleven of the table's slots hold the address
of a bare `ret` instead, and a frame on one of those masks is not drawn at all.

The project had read the mask as a bit length, which is the same answer for `0x01`,
`0x03`, `0x07`, `0x0f` and `0x1f`. Those five are most of the release, which is why it
survived so long. On the gapped masks it reads one plane too many and takes the head of
the next frame as the top plane, so the sprite comes out in stripes of `index + 16`: the
intro's walking druids in navy, and the arch-druid's lightning in red and white instead
of blue.

**It was checked against the whole release rather than against the one sprite that showed
it.** For every pair of frames adjacent in a bank's packed blob, the gap between their
data offsets must be `popcount(mask) * stride * height`. That holds in all 3,236 cases a
gap can be measured, with no exceptions, across the 386 files.

The same table explains the fonts. `BOLD.F` is mask `0x0f` and every glyph is drawn in
five indices: 5 is an outline that rings the letter and fills its counters, and 9 to 12
are the bright face inside it. `MESSAGE.PIV` carries `000`, `fed`, `dc9`, `b95`, `842` at
exactly those five entries and uses none of them in its own picture, and `INTR.EXE`'s
`0x0cfb` writes the same five words itself after every caption. So the font's colours are
recovered, and a bold glyph has to be blitted with its own indices: flattened to one
colour the ring and the face become the same colour and every letter closes up.

## The glyph map was in the executable after all

`GFX:TextASCII` at image 107,446 in `MAIN.EXE` is 95 bytes indexed by `character - 32`, and
`TextP` reads it with `sub al, 0x20; mov di, TextASCII; add di, ax; mov al, [di]`. That is
the table this project had recorded as not recovered and reconstructed by looking at the
artwork.

**The reconstruction was right.** A-Z at 0..25, a-z at 26..51, 0-9 at 52..61, then `!` 62,
`.` 64, `,` 65, `#` 66, `$` 67, `%` 68, blank 69, `'` 70 and `/` 71. One correction: the
bold bank's glyph 71 had been read as a horizontal bar and is a slash, the same as the
small bank's. One gap: **no character maps to glyph 63**, so the `?` there is the single
entry the table cannot confirm.

The metrics come with it. `TextP` advances by the glyph's own cel width, except that
`CheckBOLD` sets bit 3 of the record's flag word whenever the current font is `BOLD.F` and
`TextP` then does `sub word ptr [textwidth], 3`. So the bold face tracks three pixels tight
and the small face not at all, and a space is glyph 69 drawn like any other character.

## What `TextP` does with a glyph, and what follows from it

`TextP` looks the glyph up, puts its width and height in the blitter's registers and
calls the same routine every other cel in the game goes through. **There is no ink
anywhere in the text path.** A glyph is a sprite, and its colours are its own five
palette entries.

That is checkable against the artwork, and it holds. Of the thirty seven full-screen
pictures the bake produces, exactly three keep 5 and 9 to 12 out of their own painting
and hold the face's ramp there instead:

```text
MESSAGE.PIV   000 / fed dc9 b95 842     every message in the game
CH.PIV        000 / fed dc9 c95 842     the title, the select and the next-day screen
bg8.piv       000 / fee dc9 b95 842     the victory plate
```

Everything else uses those five entries for its picture, which is why a caption over an
intro plate has to write them first, and `INTR.EXE`'s `0x0cfb` is the original doing
exactly that. So a line of text is legible in its own colours on those three plates and
nowhere else, and the three are precisely the plates the game writes on.

## The title screen is `CH.PIV`, and it was never in `INTR.EXE`

This file and `COMPLETE.md` section 8.1 recorded that what the option list was drawn over
was unknown, and that the picture must live in `INTR.EXE`. It does not. It is named twice
over in `MAIN.EXE`'s own loader:

```text
_LOADER:MoonPic    "CH.PIV"        _LOADER:MoonFont   "BOLD.F"
```

The routine at image `0x87c3` loads the font, loads the picture, keeps a copy of it in
the segment at `DS:0x88fb`, and then blits three cels of the bold bank on it:

```text
mov ax, 0x49 ; mov bx, 5   ; mov cx, 0x14   the wordmark at (5, 20)
mov ax, 0x4a ; mov bx, 0x16; mov cx, 0xb5   the copyright line at (22, 181)
mov ax, 0x4b ; mov bx, 0x6e; mov cx, 0xbe   `All rights reserved` at (110, 190)
```

and walks `TitleMes` over it, which is `created by` centred at y 90, `Rob Anderson` at
105 and `Loading...` at 150. `MOON:DoOptions` then calls `0x890c`, which loads `Sel.cel`
and puts the same stored picture back through `0x8e3f`, and `DisplaySelect` blits cel
0x49 again at (5, 10) with the arrow and the option list under it.

The stored picture is bare. The `rep movsw` at `0x8839` that fills `DS:0x88fb` runs
before the three blits at `0x8850`, `0x8860` and `0x8870`, so what `0x890c` puts back has
no wordmark and no credit lines on it, and `DisplaySelect` adds the wordmark and nothing
else. So the option screen is the night sky, the wordmark ten pixels higher than on the
loading title, and the rows; the copyright line and `All rights reserved` are on the
loading title only. henge drew them on the option screen as well for a while, and once
the rows had their own coordinates `Select Knight` at y 170 ran straight into the
copyright line at 181. The wordmark is not centred on either screen.

The four option rows take their `y` from `MOON:ARR` at `DS:0x706`, readable now, and `ARX`
is what `DoOptions` writes into it: `0x32`.

## The status screen is a pair of stone arches, and it has its own palette

`_STATUS:DisplayPillars` clears the screen and falls into `StatusSetup`, which walks
eight-byte records `[cel][x][y][mirror]` and blits each until the first word goes
negative. `TradingData` at `DS:0xf37c` has seven records and **no terminator**, so
walking it walks `SingleData`'s eight as well and stops on that one's `ff ff`;
`OffsetValues` is `{9, 3}` and only those two status types take `SingleData` alone with
the knight's numbers shifted right by `0x4a`. The plain sheet takes both tables, which is
fifteen cels of `KI.CEL`: cel 26 is a 25 by 117 pillar at x 0, 148 and 296, and cels 34,
35 and 36 are a column, a corner and an arch head, each placed once and once mirrored.
Two ivied arches on three pillars, the knight's numbers in the left one.

`ABorders` is a third such table and it is the row labels: cels 38, 39 and 40 at x 41 and
41, 42 and 43 at x 89, on rows 35, 42 and 49. They read `STR :`, `CON :`, `END :`,
`XP :`, `GOLD :` and `HIT :`.

The palette is `STAPAL`, twenty eight twelve-bit words, and `STAKNP`, the four after it,
which together are the screen's thirty two colours; `ColourStatus` overwrites the first
two of `STAKNP` on `knight[0x20]` with `03f/028` blue, `fb0/b60` gold, `4c3/160` emerald
and `f00/800` red. With that loaded every cel on the screen draws in its own colours and
nothing on it has to be flattened.

One reading was wrong and shows: `DisplayKnight` draws the life points as cel
`StatACEL + 0x11`, and `_displayknight` sets `StatACEL` to 1, so the cel is **18**, with
two more added when `knight[0x3a]` says the wizard has made a toad of him. `KI.CEL` 17
and 18 are a helmed head and 19 and 20 are a frog. This project drew 19.

## The fight palette is written, not substituted

`MOON` has a family of `Colour*` routines, and for a long time this project had read one
of them (`ColourKnight`) and left the mechanism it belongs to unbuilt, recolouring the
knights instead by a hue substitution of its own. Reading the family whole settles how a
bout gets its colours, and the answer is that **nothing is ever substituted**: every
fighter's artwork is painted against fixed palette indices, and the bout writes colours
into those indices before it starts.

`ColourBackDrop` (image 0x460f, capital D) is the entry. Its first instruction is
`mov si, 0x80bb; mov di, 0x7a80; mov cx, 0x20; rep movsw`, which copies thirty two words
from `DS:0x80bb` into `BattlePal`. `DS:0x80bb` is where the picture loader at 0x875e
leaves a picture's palette (`mov di, 0x80bb; rep movsb` after the header, then a loop
that byte swaps each word), and the landscape generators at 0x8cd3, 0x8d02, 0x8d31 and
0x8d60 each load their backdrop, `GLB1.CMP`, `FOB1.CMP`, `SWB1.CMP` or `WAB1.CMP`, through
it before the layout. So the base is the backdrop picture's own palette. It then stores
`ax` in `COLOURS`, sets `si = 0x7a92`, which is entry 9, and dispatches: 0 to
`ColourBeast`, 2 `ColourMudmen`, 4 `ColourDemon`, 6 `Colour2ndKnight`, 0xa `ColourDragon`,
0xc and 0x10 `ColourTroggAxe`, 0x12 `ColourRatmen`, 0x18 `ColourBalok`, 0x20
`ColourTroll`, anything else straight to `ColourMainKnight`. The thirteen `InitKnightvs*`
routines each end on `mov ax, <code>; jmp ColourBackDrop`, which is where the codes were
read: both trogg loaders with axe and hammer pass 0xc and the spear passes 0x10.

Every creature routine is a run of `mov word ptr [si + 2k], imm` stores, so the colours are
immediates in the code and were read off it directly. `ColourTroggAxe` (0x46c0) is the
one that tests something first: `[0x694e]`, the landscape code, against 6 and 0, so a
trogg has one block on the wastes, another on the moors and a third everywhere else.
`ColourTroll` stores six words and not seven. `ColourDemon` is `mov di, 0x7992; mov cx,
0x17; xchg si, di; rep movsw`, twenty three words from `BlueDemon` over 9 to 31.
`ColourDragon` stores its seven and then `mov si, 0x7aba` and three more, which is entry
29. `Colour2ndKnight` is `mov di, [0x897b]; call ColourKnight`, the second knight's
record, with `si` still at entry 9.

All of them fall into `ColourMainKnight` (0x47e4): a fade out, `mov di, [0x8979]; mov si,
0x7a8c; call ColourKnight`, the main knight's record and entry 6; `call ColourBackdrop`;
`mov word ptr [0x7a80], 0`; `cmp [COLOURS], 0xa; je; mov word ptr [si + 0x1e], 0xc00`,
which is entry 15 made red for everyone but the dragon; then the fade in.
`ColourBackdrop` (0x4879, lower case d) returns at once when `COLOURS` is 4 and otherwise
branches on `[0x694e]`: 0 and 2 copy `PlainsCOLOUR` or `ForestCOLOUR` to entry 16, 4 and
6 first write `ffd 998 776 443` to entries 1 to 4 and then copy `SwampCOLOUR` or
`WasteCOLOUR`. `ColourBACKDROP` (capitals) is the shared tail, `mov si, 0x7aa0; mov cx,
0xd; xchg di, si; rep movsw`, thirteen words to entry 16. The four tables are in the data
segment and the baker reads them out of the image; they match the entries 16 to 28 of
the four backdrop pictures word for word, which is the corroboration.

**How the second knight is a different colour.** `InitKnightvsKnight` calls a loader at
0x89cd, which loads `HE1.OB`, `HE2.OB` and `HE3.OB` into the creature table in place of
`KN1` to `KN3`, sharing `KN4` and `KN5`. Counting the indices in the baked sheets says
what they are: `KN1`..`KN3` put 13 percent of their pixels on each of 6, 7 and 8 and
none on 9 to 11; `HE1`..`HE3` put 17 percent on each of 9, 10 and 11 and none on 6 to 8.
The same knight, painted in the other three entries. Every creature sheet uses 9 upwards
and none of them uses 6 to 8, and every one of them uses entry 5, which is the black
outline in all four backdrops.

**The call trap bit again here**, and the note above is the fix. `SetUpDKL`'s call reads
as `0x8e80` and `InitKnightvsKnight`'s as `0x8b9a`; taken as image offsets the first is
mid-instruction and the second lands inside the knight loader. Undoing the callers' own
`-12` and applying `-473` gives `GENERATELANDSCAPE` at 0x8cb3 and the second knight's
loader at 0x89cd, both on an entry point.

**What was wrong on screen, exactly.** `FOB1.CMP` was saved with `d00 900 600` at 6 to 8
and the mudmen's `332 ccb b81 851 630 f52` at 9 to 14, so with nothing written a knight
drawn over it is red and a trogg is grey and gold. The hue substitution then moved the
red into the nearest populated hue bucket, which on the forest palette was the browns,
and that was the gold knight the screenshot showed.

**Left alone.** `ColourEn4Knight` (`_TAVERN`, 0xb4b2) writes four words at `si + ax` into
the henge picture's palette, `0x80bb + 0x10`, entries 8 to 11, for the knight at the
stones: `05d 028 016 003` blue, `fa0 b40 930 710` gold, `e00 900 600 300` red, `0c5 082
061 040` emerald. Its red branch tests `[si + 0x20]` where the other three test
`[di + 0x20]`, which with `si` already at entry 8 of the palette is entry 24 of the
picture and not the knight's colour index, so as shipped the red knight probably never
gets his red there. Not built,
not the arena.

## Consequence for the port

The bestiary is unblocked. All eight creatures are composed the same way, and their
scripts are recovered along with the knight's, and the port now runs them: the animation
sequences are the executable's own, not authored, which was build-order item 27.

## How an actor moves, which is the pattern any new creature is built to

Written down because it was got wrong twice, and because it is what any monster added
later has to follow rather than being given an invented speed.

### There is no such thing as a speed per tick

A controller runs **once per displayed frame** and moves its actor **once**, by a number
it looks up. Nothing in the image adds a fraction of a step on a sub-tick, and nothing
holds a "pixels per tick" figure at all. A combat frame is `DELAY` = 6 ticks of the
54.6204 Hz timer (109.849 ms; see the clock section), so an engine that applies a flat
step every tick moves everybody six times too far between decisions — and, worse, makes
every position a creature can stop at a multiple of one stride, which is how the spear
trogg came to step over his own ten-pixel attack window in both directions for ever.

### The four movers, and the table each caller chooses

`MoveL` (0x4dd5), `MoveR` (0x4e09), `MoveU` (0x4e3d) and `MoveD` (0x4e64) are one routine
four times:

```text
04e0e  mov al, byte ptr [si + 0xa]   ; the walk cycle
04e11  shl ax, 1
04e13  shl ax, 1                     ; times four: (x, z) pairs
04e17  add di, ax                    ; di is the table the CALLER loaded
04e19  mov ax, word ptr [di]         ; this frame's x
04e1b  mov word ptr [0x7bf5], ax
04e1e  mov ax, word ptr [di + 2]     ; and this frame's z
04e21  mov word ptr [0x7bf7], ax
```

`MoveL` then negates the x (0x4df5) and `MoveU` the z (0x4e59), so a table is written
rightward and downward and the other two directions are its mirror. `MonsterWalk`
(0x4eeb, 0x4ef1) adds the pair once and is done.

`di` never comes from the actor record. The caller names its table outright, and a scan
of every `mov di/si/bx, imm16` in the image against the six table addresses finds
**twelve** loads and no others:

| caller | table | actors |
|---|---|---|
| `TroggMove` 0x2e37, 0x2e43, 0x2e4f, 0x2e5b | `TroggWALKU`, `TroggWALKD`, `TroggWALKR` | all three troggs |
| `ControlBlackKnight`'s `M0$`..`M3$` 0x4be6, 0x4bf2, 0x4bfe, 0x4c0a | `BKnightWALKU`, `BKnightWALKD`, `BKnightWALKR` | a computer knight |
| `MudmenMoveR`/`MudmenMoveL` 0x5422, 0x5435 | `MudmenWALK` | mudmen |
| `TrollMoveL`/`TrollMoveR` 0x5648, 0x5667 | `TrollWALKR` | the troll |

The person's own knight has the same thing under other names — `K_WalkRValue`,
`K_WalkUpValue`, `K_WalkDownValue` (0x77fe, 0x7808, 0x7810) — because `ControlKnight`
reads them itself, as three rows of plain words, instead of going through the movers.

### The cross-check that proves the layout

`BKnightWALKR` is `(25,0) (3,0) (23,0) (4,0)`; `K_WalkRValue` is `25 3 23 4`.
`BKnightWALKU` is `(0,2) (0,9) (0,2) (0,9)`; `K_WalkUpValue` is `2 9 2 9`.
`BKnightWALKD` is `(0,8) (0,2) (0,9) (0,2)`; `K_WalkDownValue` is `8 2 9 2`.

The two knights walk the same distances on the same frames. If the pair layout, the
stride or the cycle length were read wrong, these would not line up, and
`tables::the_walk_speed_tables_read_as_their_movers_index_them` fails if they stop
lining up.

### How long a table is, which is not a property of the table

`NextWalk` (0x4ef7) does `add byte ptr [si+0xa], al`, `and byte ptr [si+0xa], 7`, and
then **skips any index whose walk *script* row word is zero** (0x4f0f). So the cycle is
as long as the matching row of walk scripts, and the speed table is read at those
indices and no others. The tables are written longer than they are walked —
`TroggWALKR`'s twenty four bytes hold its three entries twice over — so measuring one by
its symbol gap gives a cycle the game never walks. The row is what measures it:
`TroggAxe_WalkR1..3` is three, `TroggAxe_WalkU1..4` is four.

One exception, and it is a real one: **`MudmenMoveR` shifts the cycle once** (0x5420),
not twice, so `MudmenWALK` is two bytes an entry — x alone. Its depth is the flat `±2`
of `MudmenMoveU`/`MudmenMoveD` (0x5447, 0x5451).

### What the rest of the bestiary does instead

Five creatures have no controller step at all and move from their own scripts:

* the **beast** charges, and wraps at the screen edge rather than stopping —
  `BeastCharge` (0x2fec) sets the column to 0x17c when it passes 0x154, `BeastChargeLeft`
  (0x2ffe) sets it to -50 when it passes 0;
* the **ratman** leaps and hangs (`RatmanLeap` 0x31e7 writes `+6` on an arc);
* the **Balok** jumps (`BalokJumping` 0x3714 writes `+2` and `+6` outright);
* the **dragon** flies (`ContinueDragon` 0xa5f5);
* the **demon** is the one that does have a step and no table: `DemonMove` writes it as a
  literal, `mov ax, 5` / `mov ax, 0xfffb` for the column (0x4fe2, 0x4fed) and
  `mov bx, 0xfffb` / `mov bx, 5` for the depth (0x4ff6, 0x5001), and adds them at 0x5004
  and 0x500a. Five pixels a frame, both ways.

### So: adding a creature

1. Give it walk **script** rows first. Their length is its walk cycle.
2. Give it a walk-speed table with one `[x, z]` entry per row entry, in
   `ActorDef::walk_speed`. Where the original has one, read it at bake time from the
   symbol its mover names (`tables::WALK_SPEED_TABLES`) rather than typing the numbers.
3. Where an axis has no table, put the controller's own literal in `speed_x`/`speed_y`
   — **per displayed frame**, not per tick — and cite the instruction that writes it.
4. Leave both empty for anything that moves from its scripts.

Uneven entries are not decoration. They are what lets a creature come to rest at
distances a single stride would skip, and several of the range bands they have to land
in are narrower than one stride.

