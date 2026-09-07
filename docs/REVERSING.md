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

## Consequence for the port

The bestiary is unblocked. All eight creatures are composed the same way, and their
scripts are recovered along with the knight's.

The animation sequences currently in this project are **authored, not recovered**: read
off the sprite banks frame by frame, with timings chosen by feel. Replacing them with the
real scripts is build-order item 27, and now only needs the VM written in Rust.
