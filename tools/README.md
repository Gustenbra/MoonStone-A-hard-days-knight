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

## What the unpacked image contains

- The game's complete data-file table, in load order, including the disk prompts
  (`Please insert Disk A`, `Please insert Sample Disk in Drive`).
- 16-bit code for all 11 modules named in the symbol table.
- The animation scripts, as named data in DGROUP, image `0x13024`-`0x206a5`: 221 of
  them, decoded in `docs/TASKVM.md` and readable with `taskvm.py`.

## What the symbol table gives

`MAIN.EXE` carries Borland-style debug info appended after the image: a
line-number table, a module table, and 345 names. The module table decodes
cleanly as contiguous start/length pairs, so each of `MOON.ASM`, `GFX.ASM`,
`_KBD.ASM`, `_DOS.ASM`, `_LOADER.ASM`, `_EMM.ASM`, `_TASK.ASM`, `_MAP.ASM`,
`_TAVERN.ASM`, `_WIZARD.ASM` and `_STATUS.ASM` has a known code range.

The name-to-address mapping is **not yet recovered**. That is the next thing
worth having, because it would land `TASKSEQ`, `TASKPLACE` and `CALCHIT` exactly.
