# Research tools

Python, not Rust, because these run once during investigation rather than as part
of the game. Nothing here ships.

## `unpack_pklite.py`

`MAIN.EXE` is PKLITE-compressed, so none of the game logic can be read until it
is unpacked. This does that by **running the executable's own decompression stub
under emulation** (Unicorn, 16-bit real mode) rather than reimplementing PKLITE's
format. The stub is the specification, so executing it cannot disagree with it.

```sh
pip install unicorn capstone
python3 tools/unpack_pklite.py MAIN.EXE main.unpacked.bin
```

It stops when execution enters memory the program wrote itself, which is the
generic signal that a self-extractor has jumped into its payload. A packer that
relocates its own decompressor first trips that test early, so the first such
jump is treated as a stage boundary and the run continues.

On Moonstone's `MAIN.EXE`: 1.3 million instructions, 78 KB in, 181 KB out.

## `symbolmap.py`

Continues through the second layer (`MAIN.EXE` is PKLITE outside and Microsoft EXEPACK
inside) and parses the TASM symbol table appended after the image. Writes
`research/symbols.json` and `research/main.final.bin`, which the baker reads.

```sh
python3 tools/symbolmap.py MAIN.EXE research/symbols.json --image research/main.final.bin
```

## `taskvm.py`

Disassembles and draws the animation scripts. See `docs/TASKVM.md`.

## `tunes.py`

The music. Each `xTUNEn.BIN` is a relocatable x86 driver with the song welded into it,
so there is no format to parse; but three of them ship for every tune, one per sound
card, and **the Roland one talks plain MIDI to an MPU-401**. So this runs the game's own
driver under the same Unicorn harness, answers the MPU's status port, and writes down
what the driver sends.

```sh
python3 tools/tunes.py "path/to/Moonstone" research/tunes.json
```

The driver is entered as an interrupt handler with `ah = 0` to start, `1` to tick and
`2` to stop, and the game ticks it from its 54.62 Hz timer, so one call is one tick. All
six tunes come out as note, channel, velocity, start and length. `henge-bake` then puts
them in the pack, and `henge-bake --render-music <dir>` writes them out as WAVs.

The same argument as the unpacker: the driver is the specification, so running it
cannot disagree with it.

## What the unpacked image contains

- The game's complete data-file table, in load order, including the disk prompts
  (`Please insert Disk A`, `Please insert Sample Disk in Drive`).
- 16-bit code for all 11 modules named in the symbol table.
- The animation scripts, as named data in DGROUP, image `0x13024`-`0x206a5`: 236 of
  them, decoded in `docs/TASKVM.md` and readable with `taskvm.py`.

## What the symbol table gives

`MAIN.EXE` carries Borland-style debug info appended after the image: a
line-number table, a module table, and the names. The module table decodes
cleanly as contiguous start/length pairs, so each of `MOON.ASM`, `GFX.ASM`,
`_KBD.ASM`, `_DOS.ASM`, `_LOADER.ASM`, `_EMM.ASM`, `_TASK.ASM`, `_MAP.ASM`,
`_TAVERN.ASM`, `_WIZARD.ASM` and `_STATUS.ASM` has a known code range.

The name-to-address mapping **is** recovered: 2,223 symbols, by `symbolmap.py`. See
`docs/REVERSING.md`, including the correction to the earlier negative result.

One thing to watch when reading the disassembly: code addresses take a seven-step
monotone correction, so a relative call that crosses one of those steps decodes to a
target that is off by the difference between the two steps. Applying the source's own
correction and then looking for the region the target lands in fixes it; without that,
`COLCON`'s two calls into `GFX` look like they point into the middle of an instruction.
