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
not a wall. Mask 1 and mask 2 are half speed on different rhythms, mask 3 a quarter. Laid
over the artwork the 3s are the mountain spine and the escarpments, the 1s the deep forest
and the 2s the marsh and the broken waste.

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

### Part of DGROUP is not in the load image, and `MapIconsTABLE` is in it

`MapIconsTABLE` itself cannot be read. The first **2,906 bytes** of DGROUP in the unpacked
image are a byte-for-byte duplicate of the region 0x5398 higher, which is real animation
script data; the duplicate ends exactly where `MOON:LairFile`, the first initialised datum,
begins. So that span is uninitialised storage the packer does not carry and the emulated
unpack left stale, not the program's data.

Everything the symbol table places below DS:0b5a is therefore unreadable in this image:
`MapIconsTABLE`, `LairLocation`, `LairType`, `ForestLairs`, and MOON's scratch variables.
Everything at or above it reads correctly, which is how `LairFile` gives all 24 lair
layouts (`fol1.t` through `gll6.t`, six per family) and how the terrain grids at 0x1e7da
and 0x1ebc2 are trustworthy.

Worth knowing before trusting any address in that range: a symbol having an address does
not mean the bytes at that address are the program's.

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
The routine that reads it tests its argument against 4 first and keeps `FO2.CMP` when it
matches. Every `.T` placement's first byte is 3, 4 or 0xfe, so the natural reading is that
the byte is that argument. **Checked by compositing and looking**: drawing `GL1.T` with
everything from `FO2` gives a black tangle where the canopy should be, everything from
`FO1` gives blue-black blobs, and 4 from `FO2` with the rest from `FO1` gives a tree with a
trunk, a canopy and a stump. The same test on `FO3.T` puts 0xfe with 3 rather than with 4.

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
next question, because if it did then its fitted code correction would be an artefact and
its "first 2,906 bytes of DGROUP are a stale duplicate" would be the same misreading, which
would make `MapIconsTABLE` readable. It is not so, on two independent tests. Running the
same expander over `main.final.bin` finds no real stream: what it matches would make the
file *shorter*, which a run-length expansion cannot do. And every one of the 411 data
symbols that names a string has that string sitting at exactly the address the symbol
table gives, in the unexpanded image, with none wrong. The image is already the program.

So the two executables genuinely differ: `INTR.EXE`'s image needed expanding and
`MAIN.EXE`'s did not, and `MAIN.EXE`'s seven-step code correction stands, corroborated as
it was by all nineteen `TaskComTable` handlers landing on entry points with it and none
without it. `MapIconsTABLE` stays unreadable.

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

## Consequence for the port

The bestiary is unblocked. All eight creatures are composed the same way, and their
scripts are recovered along with the knight's.

The animation sequences currently in this project are **authored, not recovered**: read
off the sprite banks frame by frame, with timings chosen by feel. Replacing them with the
real scripts is build-order item 27, and now only needs the VM written in Rust.
