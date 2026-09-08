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
    u8  blit_mask       which output bits the stored planes feed
u8  packed[]
```

Row stride is `((width + 15) / 16) * 2` bytes, so decoded width rounds up to a multiple
of eight and each frame carries up to fifteen dead columns on the right. Colour index 0
is transparent.

### `blit_mask` is a bit set, not a bit length

**A frame stores `popcount(blit_mask)` planes, and stored plane *i* goes to the mask's
*i*th set bit.** Bits the mask does not name are zero in every pixel of the frame.

This is not inferred. `GFX` carries one hand-written routine per mask value, named after
the byte itself, and `MAIN.EXE`'s symbol table names all twenty-two of them: `do_1`,
`do_3` through `do_7`, `do_9` through `do_f`, `do_10`, `do_12`, `do_13`, `do_17`,
`do_1b`, `do_1c`, `do_1e`, `do_1f` and `do_20`. That list is exactly the set of mask
bytes the release uses. Each routine loads one byte per stored plane through `si`, `di`,
`bx`, `bp` and a fifth pointer kept in the code segment, then rolls five bits into `al`
with `rol`/`rcl` pairs, the last one rolled becoming bit 0. `do_17` is the clearest:

```asm
do_17:  xchg bx, dx
        mov  dh, [si]        ; plane 0 -> bit 0
        mov  dl, [di]        ; plane 1 -> bit 1
        mov  ch, [bx]        ; plane 2 -> bit 2
        xor  cl, cl          ; bit 3 has no plane, so it is zeroed
        mov  ah, [bp]        ; plane 3 -> bit 4
        inc si / inc di / inc bx / inc bp     ; four pointers, four planes
```

Mask `0x17` is `10111`, whose set bits are 0, 1, 2 and 4, and those are the four the four
planes land in. `do_1b` (`11011`) skips bit 2 the same way and stores four, `do_13`
(`10011`) stores three, `do_9` (`01001`) two.

Reading the mask as a *bit length* instead gives the same answer for `0x01`, `0x03`,
`0x07`, `0x0f` and `0x1f`, which is most of the release, so most of the game looks right
either way. On a gapped mask it reads one plane too many, takes the head of the **next
frame** as the top plane, and paints the sprite in stripes of `index + 16`. `DW1.CEL`'s
walking druids are mask `0x17`: read as a length they come out navy and striped, read as
a set they come out grey and white.

Verified across the whole release: for every pair of frames adjacent in a bank's blob the
gap between their offsets is `popcount(mask) * stride * height`, in all 3,236 cases where
a gap can be measured, with no exceptions.

`blit_mask == 32` is the one routine that is not its own set bits. `do_20` stores **two**
planes, puts them in bits 0 and 1, and ors them together into bit 4:

```asm
do_20:  xchg bx, dx
        mov  dh, [si]        ; plane 0 -> bit 0
        mov  dl, [di]        ; plane 1 -> bit 1
        xor  ch, ch / xor cl, cl
        mov  ah, [si]
        or   ah, [di]        ; either -> bit 4
```

It is the cheap outline blit, and `PO.CEL`, the pointer, is the only file in the release
that uses it.

The routines are reached through `plane_tab`, thirty-three words indexed by the mask byte
and sitting in the code segment beside them. Twenty-two of its slots hold a routine and
are exactly the twenty-two `do_` symbols, each in its own slot; the other eleven hold the
address of a bare `ret`. So masks `0x00`, `0x02`, `0x08`, `0x11`, `0x14`, `0x15`, `0x16`,
`0x18`, `0x19`, `0x1a` and `0x1d` **draw nothing at all**. Nine of those never occur in
the release. The two that do are `0x00`, the forty-two placeholder entries, and `0x11`,
which is `TROLL1.CEL` frame 46 and `MUDMEN2.CEL` frame 15: two frames the artwork carries
and the blitter refuses.

`plane_count` at `+8` is 1 in every frame of the release and nothing reads it.

### Where the fonts keep their colours

`BOLD.F`'s glyphs are mask `0x0f` and every one of them is drawn in five indices and no
others: **5 is the outline**, which rings the letter and fills its counters, and **9, 10,
11 and 12 are the letter face**, a bright stroke shaded across four steps inside that
ring. `SMALL.FON` is mask `0x01`: one plane, index 1, a true silhouette.

Those five entries are reserved. `MESSAGE.PIV`, the plate every message in the game is
written over, carries `000`, `fed`, `dc9`, `b95`, `842` at exactly 5, 9, 10, 11 and 12,
and uses none of them in its own picture; `INTR.EXE` writes the same five words itself
around each caption. So a bold glyph must be blitted with its own indices, not flattened
to one colour: flattening paints the ring and the face alike, every counter closes and
the line reads as a row of blobs.

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
u16be count                        how many border rectangles follow
border[count]                      eight bytes each:
    i16be left, right, bottom, top
placement[]                         six bytes each, until sheet == 0xff:
    u8  sheet
    u8  cell
    i16 x
    i16 y
```

**The header is a list, not one rectangle, and a rectangle is impassable ground rather
than the walkable box.** The loader at image `0x8d93` reads the count, skips `count * 8`
bytes to reach the placements, and walks the records again taking the deepest `bottom`
into DS:`0x80b5`; `SBORD` (`0x4552`) walks the same list every frame. The walkable ground
is everything *below* the rectangles.

Fifty two of the fifty six layouts hold exactly one record, the tree line, which is why
reading the header as `u16 _; u16 left, right, bottom, top` looked right for years. **Four
do not**, and each extra record is a patch of scenery that hangs lower than the tree line
in its own columns:

| file | records |
|---|---|
| `FO7.T` | `0..319` down to 91, and `66..164` down to 103 |
| `SW6.T` | `0..319` down to 110, and `150..359` down to 124 |
| `SWL2.T` | `0..319` down to 115, and `222..541` down to 128 |
| `GLL4.T` | `0..319` down to 97, `42..132` down to 123, and `217..468` down to 112 |

`right` is not clipped to the screen: `SWL2` and `GLL4` both name a rectangle that runs off
the right edge. Reading those four files as one record leaves the placement walk eight or
sixteen bytes out of step, which is not a small error: it turns the first placements into
nonsense and stops the walk on the first one that is off screen. It cost `FO7`, `SW6` and
`GLL4` most of their scenery, and it cost `SWL2` **all** of it. `SWL2` is not, as this
project used to record, a lair floor with no scenery on purpose; it has eighty nine
placements, a rune wall and a chest among them.

The two stub layouts, which ship three times over as `F09.T`, `SW9.T` and `INTRO1.STI`,
declare a count of 19,342. A count above about sixteen is how they are told apart from a
layout.

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

Every arena's first record spans the full width and starts at the same `y`, 10, and only
its `bottom` varies, from 80 in `GLL2` to 159 in `WA6`. That one number is the tree line,
and it is what makes some fights a strip of ground and others most of the screen.

## `.STI`: tile maps

Three files, all in the intro's own set: `INTRO.STI` and `CO.STI` at 960 bytes and
`INTRO1.STI` at 105. **`INTRO1.STI` is not one of these at all**: it is byte for byte
`F09.T` and `SW9.T`, a stub `.T` arena that ships three times under three names.

The other two are tile maps, and the engine that reads them is `GFX`'s tile half
(`PlaceTile`, `FindTile`, `CalcOffset`, `ClipTile`, `Dump_Tile`), which `MAIN.EXE` carries
as well. Nothing in the shipped game data needs it: these three files are the only tile
maps in the release.

```
u16be tile[]        ten to a row, rows top to bottom
```

- **Ten tiles to a row.** The walker steps x by 32 until it passes 319.
- **A tile is 32 by 25**, cut from a 320x200 image ten across: `FindTile` computes
  `x = (n % 10) * 32`, `y = (n / 10) * 25`. That is the same grid a `CMP` scenery sheet is
  cut on, so a `PIV` used this way holds eighty tiles.
- **The words are big-endian**, which the walker does with an `xchg ch, cl` after the load.
- **`n / 80` chooses the sheet.** The intro loads three and keeps their segments in a table
  of three words, with a second table of the first tile number in each.

`INTRO.STI` is therefore 48 rows: a 320 by 1200 panorama, its three sheets `bg1a.piv`,
`bg1c.piv` and `bg1b.piv`, with the moon at the top, the treeline in the middle and a
colonnade of trunks at the foot. Only the first sheet's palette is copied to the live one,
so all three are drawn in `bg1a`'s. `CO.STI` is 48 rows as well, one sheet, the bottom
eight rows a whole screen and everything above them tile 0 repeated.

Nothing draws the map whole. `Dump_Tile` stamps one 32x25 tile straight into mode X, the
pan scrolls what is already on the screen by whole rows of pixels and stamps two fresh tile
rows at the edge it uncovered, and `ClipTile` handles the tile row that is half off the
top.

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

## Music: `xTUNEn.BIN`

**Not a format, and recovered anyway.** Each tune file is a relocatable x86 driver with
the song welded into it. There are eighteen, which is `MusicTable`'s six tunes by three
sound cards, and the first letter of the name says which card.

Every one of them opens with the same dispatcher, which is the whole of its interface:

```text
1e              push ds
53              push bx
8c cb  8e db    mov bx, cs ; mov ds, bx      the blob addresses itself
80 fc 00  74 xx cmp ah, 0 ; je  start
80 fc 01  75 03 cmp ah, 1 ; jne over
e9 xx xx        jmp tick
80 fc 02  75 03 cmp ah, 2 ; jne over
e9 xx xx        jmp stop
5b  1f  cf      pop bx ; pop ds ; iret
```

`_LOADER`'s `LOADMUSIC` (image `0x900d`) reads the file whose record is
`MusicTable + tune * 4 + MUSICTYPE * 32` to segment `0xd7f`, `Install_Timer` points
`int 60h` at `0xd7f:0000`, and the timer handler's first instructions are `mov ah, 1;
int 60h`. The timer runs at 1193182 / 0x5555, so **a tune ticks at 54.62 Hz**.

| prefix | card | how it plays |
|---|---|---|
| `a` | AdLib / OPL2 | `out 0x388, register` then `out 0x389, value`, with six dummy reads of 0x388 between them as the chip wants |
| `b` | PC speaker | ports 0x43, 0x42 and 0x61: mode 3 square waves on timer channel 2 |
| `r` | Roland | an MPU-401 at 0x330 and 0x331, and **the bytes are plain MIDI** |

The Roland driver is what makes the music recoverable. Its data port carries ordinary
MIDI messages with running status, so running the driver under emulation and writing
down what it sends gives note, channel, velocity and length back exactly. That is
`tools/tunes.py`, and it needs no understanding of the song format at all: the driver is
the specification, the same argument that unpacked the executable.

What comes out is note events on the driver's own tick, which `henge-bake` splits into
one score per tune in the pack. Four of the six loop, and the loop point is found by
comparing what is sounding tick for tick rather than by counting events.

The AdLib and PC speaker drivers are not decoded. They would give the same notes with a
sound of their own, and reproducing either would mean writing an OPL2 or a square-wave
emulator; the Roland one already gives the notes, so neither was attempted.

## Open questions

- ~~The **arena family pairing**.~~ **Settled.** `TileTable` in `_LOADER` is four words
  indexed by the landscape code: plain and forest both take `FO1.CMP`, swamp `SW1.CMP`,
  waste `WA1.CMP`, over the `GLB1`, `FOB1`, `SWB1` and `WAB1` backdrops. The positional
  pairing would have given the plain family `FO2`, which is wrong: `FO2` is the sheet
  every family reaches for when a placement's `sheet` byte is 4.
- The **multi-part composition format**, which gates every creature. See `REVERSING.md`.
