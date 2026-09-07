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

### The interpreter has been found

`PerformCOMMAND` at image offset `0x97fb`, with `PerformLOOP` at `0x97f2` just above it as
the loop head. It is unmistakably a bytecode fetch-and-dispatch loop. `di` is the task
record, `[di+2]` is the script pointer, and each pass reads one byte:

```
0xff          end
0xfd          script pointer <- [actor+0x18]      return / loop back
0xfe          script pointer <- [actor+6]         restart
bit 0x80 set  call [TaskComTable + (op & 0x7f)]   a command, through a function table
otherwise     op & 0x1f selects a bank from [di+0x18], then a frame record follows
```

`TaskComTable` is at DS:0x9448, and the instruction that indexes it is literally
`mov bx, 0x9448` at `0x9832`. The symbol table and the code agree to the byte, which is as
good a confirmation of both as this work gets.

The frame path reads `[si+1]` into `[di+0x13]`, `[si+2]` as a signed y contribution summed
with `[di+6]` and `[di+8]`, `[si+4]` as x added to `[di+4]`, and tests bit 0x40 of
`[si+3]`; `[di+0x14]` bit 1 mirrors the x term. Related: `TaskCelTable` DS:0x9474,
`TaskTable` DS:0x947e, `TaskPlace` 0x986c, `MoveX` 0x9b5d.

### What is known about the data

Located around `0x0e000` in the unpacked image. The structure is per-frame lists: a
two-byte header, a run of six-byte records, and a two-byte terminator. The records look
like `[kind][image][x][flags][i16 y]`, with one bit of `flags` reading as a horizontal
flip.

Three consecutive groups differing by exactly one record is what an animation cycle looks
like, and that pattern is present.

### Why it is not decoded

**Compositing a group's parts does not produce a coherent figure.** The region is right
and the record shape is roughly right, but the field semantics are wrong somewhere,
most likely in how `kind` selects which bank each part is drawn from.

This is stated plainly because a plausible-sounding table that is quietly incorrect would
be worse than nothing. **This gates the entire bestiary**: until it is decoded, no
creature can animate, and picking a different creature does not help, because they are
all composed the same way.

## Consequence for the port

The animation sequences in this project are **authored, not recovered**: read off the
sprite banks frame by frame, with timings chosen by feel. The player and hero banks
contain complete figures, so they animate; creatures do not.
