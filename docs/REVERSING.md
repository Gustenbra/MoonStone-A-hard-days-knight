# Reverse engineering notes

The game's logic lives in its executable, not in its data files. This is the record of
getting at it, and of where the trail currently stops.

## The executable is PKLITE-compressed

Nothing could be read until it was unpacked.

`tools/unpack_pklite.py` does this by **running the executable's own decompression stub
under emulation** (Unicorn, 16-bit real mode) rather than reimplementing PKLITE's format.
The stub is the specification, so executing it cannot disagree with it.

```sh
pip install unicorn capstone
python3 tools/unpack_pklite.py MAIN.EXE main.unpacked.bin
```

Result: 1.3 million instructions, 78 KB in, 181 KB out, verified as genuine 16-bit code.

The stopping rule is generic and works on other packed DOS binaries: halt when execution
enters memory the program wrote itself, which is what a self-extractor does when it jumps
into its payload. A packer that relocates its own decompressor first trips the same test,
so the first such jump is treated as a stage boundary and the run continues.

## Debug information was left in the build

Borland-style, appended after the image: a line-number table, a module table, and 345
names, including the original source file paths.

**The module table decodes cleanly.** Sixteen-byte records of contiguous start and length
pairs, verified by each start plus its length landing exactly on the next start. So every
one of the eleven source modules has a known code range.

**There is no name-to-address mapping in the file.** Tested, not assumed: the appended
region is fully accounted for as padding, a line-number table and the module records plus
name strings, and a global scan finds no run of 334 records that could be an address table.
See `COMPLETE.md` section 1.2 for everything that was ruled out and how.

Addresses must instead be recovered by matching link order: names are listed in link order,
code is laid out in link order, and each module's code range is already known.

## Animations are scripts, not frame lists

The symbol names give the architecture away. There are entries for sequence, goto, hold,
jump, loop, skip, flip, place, sort and collision operations, alongside an end-of-frame
marker and an animation-replace call.

So animations are **small scripts run by a task VM**, with jumps, loops and collision
hooks, and **characters are composed of several sprite parts per frame**. That is why
creature banks contain loose legs, torsos and heads rather than whole poses, and it is
why the combat reads as positional rather than as trading canned attacks.

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
