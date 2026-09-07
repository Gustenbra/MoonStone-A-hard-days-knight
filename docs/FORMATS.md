# File formats

Recovered by reading the data directly. A file format is a fact about how bytes are
arranged, not a creative work, so this document is freely usable. No code from any
other reimplementation was consulted beyond confirming the existence of these formats.

Everything below was verified against a complete DOS release: 386 files, decoded with
zero failures.

## Packing

Every container in the game uses the same packer.

One control byte, then eight items, most significant bit first.

- **bit clear**: one literal byte
- **bit set**: a 16-bit big-endian back-reference
  - `count  = 0x22 - (word >> 11)`
  - `offset = word & 0x7ff`, measured back from the write head

The copy may overlap the write head, which makes it double as a run encoder, so it has
to be performed byte by byte rather than with a block copy.

**Gotcha:** two stub files carry length fields that are plainly garbage. Clamp a declared
length to what the file actually contains rather than trusting it.

## PIV, CMP and .P: full-screen images

```
u16 kind          4 = four bitplanes (16 colours), 5 = five (32 colours)
u16 _
u16 packed_len
u16 palette[16 or 32]
u8  packed[]
```

Palette entries are Amiga `0x0RGB`, four bits per channel; multiply each by 17 for
eight-bit values.

The pixel data unpacks to 8000 bytes per plane, planes stored consecutively rather than
interleaved. Deplanarise most significant bit first, with plane 0 supplying bit 0 of the
pixel index.

Images are 320x200. **CMP files are PIVs used as tile sheets**, cut into 32x25 cells, ten
across. Files with a `.P` extension are also PIVs.

## CEL, OB, F, FON and .C: sprite banks

```
u16 image_count
u16 _
u16 packed_len
entry[image_count]      ten bytes each:
    u16 pad
    u16 data_offset     into the unpacked blob
    u16 width
    u16 height
    u8  plane_count
    u8  blit_mask       colour mask; its bit length is how many planes are stored
u8  packed[]
```

Row stride is `((width + 15) / 16) * 2` bytes, so decoded width rounds up to a multiple
of eight and each frame carries up to fifteen dead columns on the right. Colour index 0
is transparent.

`blit_mask == 32` is a special case: two planes, with bit 4 of the output set wherever
either plane bit is set. It is a cheap outline blit, used for shadows.

Files with a `.C` extension are banks in this same format.

**Sprite banks carry no palette.** Colours come from whichever full-screen image is
loaded, which is how the game recolours the same creature per region at no cost.

**Banks are not all whole figures.** Player and hero banks contain complete poses, but
creature banks are largely body parts, with legs, torsos and heads as separate frames,
composed at draw time. See `REVERSING.md`.

## .T: combat arena layouts

```
u32 packed_len
u8  packed[]
```

and once unpacked:

```
u16 _
u16 left, right, bottom, top       walkable bounds
placement[]                         six bytes each, until sheet == 0xff:
    u8  sheet
    u8  cell
    i16 x
    i16 y
```

**`x` and `y` are signed.** Scenery may start above or left of the screen, so a tree can
be cut off by the top edge. Reading them unsigned produces values like 65532 for -4.

**`sheet` says which of two sheets the cell is cut from**, and it is the only byte in the
format that is not obvious. Every placement in the game carries 3, 4 or 0xfe there. The
executable's `TileTable` gives each arena family one sheet, `FO1.CMP` for both plain and
forest, `SW1.CMP` for swamp and `WA1.CMP` for waste, and the routine that reads it tests
its argument against 4 first and keeps `FO2.CMP` when it matches. So 4 draws from `FO2`
whatever the family, and 3 and 0xfe from the family's own sheet. Compositing `GL1.T` all
three ways settles it: the mixed reading is the only one that makes a tree rather than a
tangle. Props sort by `y` so that actors occlude correctly.

Every arena shares the same walkable rectangle except for its depth: `x` always spans the
full width and the band always begins at the same `y`, while the bottom edge varies per
arena. That one number is what makes some fights feel cramped and others open.

## COLLIDE.HIT: hit lines

Plain text, unusually for this game.

```
Troll1.cel          bank name
00                  a frame with no hit line
05                  point count
00                  shape type (always 0 in the shipped file)
000005005005...     that many three-digit x,y pairs
99                  end of block
```

A trailing terminator with no open block appears at the end of the file; treat it as a
no-op rather than an error.

In the weapons bank the polylines run straight along the blade, so **these are the paths
a strike sweeps through, not body outlines**. That is what makes the combat positional: a
swing connects when its line crosses the target rather than when two boxes overlap.

## Samples

Creative Voice File (`.VOC`), a documented standard. Eight-bit unsigned PCM.

## Music

The tune files begin with x86 machine code. The music is driver blobs with data welded
into executable code rather than a readable format. It is the hardest thing in the game
to recover and the least worth recovering.

## Open questions

- ~~The **arena family pairing**.~~ **Settled.** `TileTable` in `_LOADER` is four words
  indexed by the landscape code: plain and forest both take `FO1.CMP`, swamp `SW1.CMP`,
  waste `WA1.CMP`, over the `GLB1`, `FOB1`, `SWB1` and `WAB1` backdrops. The positional
  pairing would have given the plain family `FO2`, which is wrong: `FO2` is the sheet
  every family reaches for when a placement's `sheet` byte is 4.
- The **multi-part composition format**, which gates every creature. See `REVERSING.md`.
