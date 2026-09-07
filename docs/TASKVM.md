# The animation task VM

Moonstone does not hold animations as frame lists. Each actor runs a small
bytecode program, and every frame of it names the sprite parts the actor is made
of that tick. This is the recovered instruction set, the frame record, and the
account of how each part of it was established.

`tools/taskvm.py` reads it back out of `MAIN.EXE`, disassembles any script, and
draws one:

```sh
python3 tools/taskvm.py MAIN.EXE --verify
python3 tools/taskvm.py MAIN.EXE --disasm Knight_SwSwing
python3 tools/taskvm.py MAIN.EXE --composite Knight_SwWalkOn
```

## Where it lives

| | |
|---|---|
| `PerformCOMMAND` | image `0x97fb`, the fetch and dispatch loop |
| `PerformLOOP` | image `0x97f2`, the loop head |
| `INITTASK` | image `0x9543`, fills `TaskComTable` and `TaskCelTable` |
| `TaskComTable` | DS:`0x9448`, 22 word slots, 19 filled |
| `TaskCelTable` | DS:`0x9474`, 4 word slots, the sprite bank tables |
| `TaskTable` | DS:`0x947e`, ten task records of 0x24 bytes |
| `TaskCommand` | DS:`0x960a`, ten VM state records of 0x1a bytes |
| the scripts | DGROUP, as named data symbols: `Knight_SwSwing`, `Troll_Walk1` |

The scripts are ordinary data in the executable's data segment, and the symbol
table names 221 of them. A script pointer is a near offset in DS.

### Reading the handler table needs the link-time offset correction

`TaskComTable` is BSS, so in the load image it is 44 zero bytes. `INITTASK`
fills it at run time with a run of `mov word ptr [di+off], imm16`, and those
immediates are the only copy of the handler addresses in the file.

The immediates are **link-time code offsets, not image offsets**. They need the
same correction `symbolmap.py` fits for code symbols: a coordinate space up to
473 bytes longer than the shipped image, changing in seven steps. Apply it and
all nineteen entries land exactly on a routine entry point. Skip it and none of
them do, several are not even instruction boundaries. That is the check that the
correction is a real property of the binary and not an artefact of the fit, and
the same correction turns every `TASKGOSUB` operand into the exact entry point
of a named routine (`KnightGruntSound`, `DrDropHead`, `KnifeThrow`).

## Dispatch

Each pass reads one byte at the script pointer, `[task+2]`.

| byte | meaning |
|---|---|
| `0xff` | end of frame. The **next** byte says what happens after it |
| `0xfd` | script pointer <- `TaskCommand[0x18]`, the resume point set by `TASKJUMP` |
| `0xfe` | script pointer <- `TaskCommand[6]`, the resume point set by `TASKLOOP` |
| bit 7 set | a command: `call TaskComTable[op & 0x7f]` |
| anything else | a six-byte sprite-part record |

Because the table is indexed by `op & 0x7f` as a **byte** offset into a table of
**words**, commands step by two: `0x80`, `0x82`, `0x84` and so on. An odd
opcode would read a misaligned word; none occurs in the data.

## The commands

Operand widths are read out of the code, not guessed: every handler advances the
script pointer itself with `add word ptr [di+2], n`, and that `n` is the width.

| op | name | bytes | operands and effect |
|---|---|---|---|
| `0x80` | `TASK_FLIP` | 2 | `u8 facing`. Sets `task+0x14`, and copies it to `actor+8`. `0xff` toggles bit 1 instead, which is the only form any script uses. 1 is facing right, 3 is facing left; bit 1 is the mirror |
| `0x82` | `TASKGOTO` | 4 | `u8 mode, u16 target`. Mode 3 jumps immediately. Any other mode arms a pending jump (`TaskCommand[8]`, flag at `[0xa]`) taken at the next end of frame |
| `0x84` | `TASKHOLD` | 2 | `u8 count`. Show the following frame `count` times. 0 is treated as 1. Counter in `TaskCommand[0]`, flag `[1]`, resume point `[2]` |
| `0x86` | `TASKJUMP` | 8 | `u8 a, u8 ticks, u8 flags, u8 yspeed, u8 ymin, u8 xspeed, u8 xmin` into `TaskCommand[0x10,0x13,0x11,0x14,0x15,0x16,0x17]`, resume point `[0x18]`. Runs ballistic motion for `ticks` frames |
| `0x88` | *(empty slot)* | | `INITTASK` never fills it; calling it would call offset 0 |
| `0x8a` | `TASKLOOP` | 2 | `u8 count`. Repeat the block up to the next `ff fe` that many times. Counter `TaskCommand[4]`, flag `[5]`, resume point `[6]` |
| `0x8c` | `TASKSKIP` | 4 | `u8 _, u16 target`. Branch if the DS:`0x700` mode flag is set. Ten uses, five targets, and four of the five are the bloodless `*_CollapseDead` scripts |
| `0x8e` | `TASKTIME` | 0 | The handler is a bare `RET`. It does not advance the script pointer, so it would spin. Never emitted in any script |
| `0x90` | `TASKMOVE` | 8 | `u8 flags, i16 x, i16 y, i16 z`. With flags bit `0x40`, sets the task position outright. Otherwise adds: x by bit 0 and the facing, y by bit 3, z by bit 5 (see below) |
| `0x92` | `TASKSOUND` | 2 | `u8 sample`. 124 uses, 49 distinct sample numbers |
| `0x94` | `TASKSAVE` | 6 | `u8 mode, i16 field, u16 value`. Store into the actor record at `actor+field`; mode bit 0 stores a byte, otherwise a word |
| `0x96` | `TASKSHADOW` | 4 | `u8 on, u16 script`. Sets `actor+0xe` to the shadow script and `actor+0xc` to on/off. Eleven uses; the four non-zero targets are `Ratman_Shadow`, `Beast_KnightShadow`, `Dragon_Shadow`, `Balok_Shadow` |
| `0x98` | `TASKGOSUB` | 4 | `u8 _, u16 routine`. Near call into the game's own code with x, y, z, facing, the bank table and the actor in registers. 124 uses; all 37 distinct targets resolve to named routines |
| `0x9a` | `TASKDEAD` | 4 | `u8 _, u16 target`. Branch to `target` and clear the VM state if `actor+0x38` (hit points) is <= 0. All 19 distinct targets are death scripts |
| `0x9c` | `TASKADDTASK` | 4 | `u8 _, u16 script`. Spawn a second task on that script. One use in the whole game, `Beast_BackToss` |
| `0x9e` | `TASKKILLTASK` | 2 | `u8 _`. Marks the task inactive and zeroes the actor's first word |
| `0xa0` | `TASKCELBUF` | 2 | `u8 n`. `task+0x18 <- TaskCelTable[n-1]`, choosing which set of sprite banks the frame records index. Only 1 and 2 are used |
| `0xa2` | *(empty slot)* | | |
| `0xa4` | `TASKTESTEQ` | 6 | `u8 mode, i16 field, u16 target`. Branch if `actor+field` is zero; mode bit 0 compares a byte, otherwise a word |
| `0xa6` | `TASKTESTNE` | 6 | as above, branch if non-zero. Not used by any shipped script |
| `0xa8` | `TASKANIMCLR` | 2 | `u8 _`. Zero the 0x1a-byte VM state, preserving the shadow fields |
| `0xaa` | *(empty slot)* | | |

`TASKMOVE`'s signs, read off `MoveX`/`MoveY`/`MoveDone` at `0x9b5d`:

```
facing right (task+0x14 != 3)   bit 0 clear: x -= v     bit 0 set: x += v
facing left  (task+0x14 == 3)   bit 0 clear: x += v     bit 0 set: x -= v
                                bit 3 set:   y -= v     bit 3 clear: y += v
                                bit 5 set:   z -= v     bit 5 clear: z += v
```

### End of frame

`0xff` is followed by one more byte. The handler at `0x993a` checks, in order:

1. a running `TASKHOLD` count: decrement, and if it is still non-zero, jump back
   to the held frame;
2. a running `TASKJUMP`: decrement its tick counter, and while it lasts, run the
   ballistic step and jump back to the point after the `TASKJUMP`;
3. a pending `TASKGOTO`: clear it and take it;
4. otherwise the byte after `0xff`:

| | |
|---|---|
| `ff 00` | end of frame, next frame follows |
| `ff fe` | end of frame; loop back if a `TASKLOOP` count is running, else advance |
| `ff ff` | end of frame and end of animation. Clears `task+1`, which is what lets `TASKHANDLE` ask the controller for a new script. The script pointer is left on the `0xff`, so the last frame keeps being drawn |

All 221 scripts end on `ff ff`.

## The sprite-part record

Six bytes. Read off `PerformCOMMAND` at `0x983b` through `0x9937`, which ends
with `add word ptr [di+2], 6`.

```
 0   u8   bank selector, in the low five bits; it is the slot number times four
 1   u8   cel index within that bank
 2   i8   y offset
 3   u8   flags
 4   i16  x offset
```

Placement, from `TASKRIGHT` / `TASKLEFT` / `TASKPLACE` at `0x985f`, `0x9864`,
`0x986c`:

```
screen_y = task_y + task_z + y

task+0x14 bit 1 clear (facing right)    screen_x = task_x + x
task+0x14 bit 1 set   (facing left)     screen_x = task_x - (x + cel_width)
```

`cel_width` and `cel_height` come from the bank's own header, entry
`bank + 10 + cel*10`, big-endian words at `+4` and `+6`. Parts are drawn in
script order, so a later part covers an earlier one.

Flag bits, and where each is read:

| bit | effect |
|---|---|
| `0x01` | also push this part onto `BodyPile`, the list the collision code walks (578 records) |
| `0x02` | also push it onto `WeoponPile` (153 records) |
| `0x10` | blit it a second time into the buffer at segment `0xac00` as well as the current target (166 records) |
| `0x40` | do not fold this part into the actor's bounding box (`FindWidth`) (163 records) |
| `0x80` | drop the part entirely, piles included, when the DS:`0x700` mode flag is set (315 records) |
| `0x04`, `0x20` | occur in the data (35 and 376 records) but are read by nothing in `PerformCOMMAND`. `0x08` never occurs |

The pile entry the collision code sees is ten bytes: the bank far pointer, the
cel index, screen x, screen y, and a zero.

### The bank tables

`task+0x18` is the DS offset of a bank table. The table's first word is the
segment its banks were loaded into; the far pointers follow, so slot `n` is at
`table + 2 + 4n`. The interpreter uses the record's selector as a raw byte
offset into that table, which is why the selector is the slot number times four.
`TASKCELBUF n` sets `task+0x18` from `TaskCelTable[n-1]`.

| table | DS | filled by | slots |
|---|---|---|---|
| 1 | `0x8933` | the knight loader at `0x893e` | 0 `KN1.OB`, 1 `KN2.OB`, 2 `KN3.OB`, 3 `KN4.OB`, 4 `KN5.OB` |
| 2 | `0x8949` | whichever creature loader ran | see below |
| 3 | `0x8919` | `0x88b5` | 0 `mi.c`, 1-4 `ki.cel`, 5 `po.cel` |
| 4 | `0x895f` | `0x8996` | 0-4 all `BLO.CEL` |

Table 2, one creature at a time, read out of the loaders:

| creature | slots |
|---|---|
| hero | 0 `HE1.OB`, 1 `HE2.OB`, 2 `HE3.OB`, 3 and 4 borrowed from the knight's `KN4.OB` and `KN5.OB` |
| trogg with spear | 0 `TroggSp1.cel`, 1-4 `TroggSp2.cel` |
| trogg with axe | 0 `TroggAx1.cel`, 1 `TroggAx2.cel` |
| ratmen | 0 `Ratmen1.cel`, 1 `Ratmen2.cel` |
| mudmen | 0 `Mudmen1.cel`, 1 `Mudmen2.cel` |
| balok | 0 `Balok1.cel`, 1 `Balok3.cel`, 2 `Balok2.cel` |
| dragon | 0 `Dragon1.cel`, 1 `Dragon2.cel`, 4 `DRAGON5.CEL` |
| beast | 0 `be1.c`, 1 `be2.c` |
| demon | 0 `Demon2.cel`, 2 `Demon3.cel`, 3 `Demon4.cel`, 4 `Demon1.cel` |
| troll | 0 `Troll1.cel`, 1 `Troll2.cel` |

## The records the VM keeps

The task record is 0x24 bytes, ten of them in `TaskTable`.

```
+0x00 u8   active                  +0x0e u16  cel width
+0x01 u8   animation still running  +0x10 u16  cel height
+0x02 u16  script pointer           +0x13 u8   current cel index
+0x04 i16  x                        +0x14 u8   facing: 1 right, 3 left, bit 1 mirrors
+0x06 i16  y                        +0x16 u16  actor record
+0x08 i16  z                        +0x18 u16  bank table
+0x0a i16  screen x (out)           +0x1a u16  CONTROLTABLE offset
+0x0c i16  screen y (out)           +0x1c u16  VM state record
                                    +0x1e u16  weapon pile head
                                    +0x20 u16  body pile head
                                    +0x22 u16  standby
```

The VM state record is 0x1a bytes, ten of them in `TaskCommand`, one per task.

```
+0x00 u8  hold count      +0x0c u8  shadow on      +0x13 u8  jump ticks left
+0x01 u8  hold running    +0x0e u16 shadow script  +0x14 u8  y speed
+0x02 u16 hold resume     +0x10 u8  jump arg       +0x15 u8  y limit
+0x04 u8  loop count      +0x11 u8  jump flags     +0x16 u8  x speed
+0x05 u8  loop running    +0x12 u8  jump running   +0x17 u8  x limit
+0x06 u16 loop resume                              +0x18 u16 jump resume
+0x08 u16 pending goto
+0x0a u16 pending goto armed
```

`ADDTASK` and `REPLACEANIM` zero all 0x1a bytes. `TASKANIMCLR` zeroes them too
but puts `+0x0c` and `+0x0e` back, so a shadow survives an animation change.

## What is inferred rather than read

Everything above is read out of the code except the following.

* **The command names.** The `TASK*` names live in the `PUBLIC` blob appended to
  `MAIN.EXE` and carry no addresses. They are emitted in definition order:
  the ones that do have addresses (`INITTASK`, `CLEARTASKS`, `REPLACEANIM`,
  `ADDTASK`, `FINDTASK`, `TASKSTANDBY`, and `TASKRIGHT`/`TASKLEFT`/`TASKPLACE`
  as three consecutive labels inside `PerformCOMMAND` at `0x985f`, `0x9864`,
  `0x986c`) come out in exactly the blob's order. Zipping the nineteen handler
  addresses against the nineteen names between `TASKSORT` and `TASKWALKCOLLIDE`
  gives the table above, and every name it produces matches what the handler
  does. It is still an ordering argument, so treat the names as very likely
  rather than certain. The opcode numbers, widths and behaviour do not depend
  on it.
* **DS:`0x700`.** A word, set to zero by the code at image `0x7c` during
  startup. Nothing else in the load image writes it, in any addressing form; it
  is only ever compared. It gates `TASKSKIP`, the part flag `0x80`, and a few
  combat decisions. What it gates fits a gore switch closely: when it is set,
  `TASKSKIP` diverts four of its five targets to the bloodless
  `*_CollapseDead` scripts, and the flag `0x80` parts are dropped. `GORESWITCH`
  is a `PUBLIC` name with no address. Reading DS:`0x700` as `GORESWITCH` is
  still an inference, and since nothing in the image ever sets it, the switch
  appears to be permanently off in the shipped build.
* **`TASKJUMP`'s first operand**, stored at `TaskCommand[0x10]`. Nothing in
  `_TASK` reads it back.
* **Part flag bits `0x04` and `0x20`.** Present in the data, read by nothing in
  the interpreter. Probably vestigial from the Amiga original.

## How it was checked

The decode was not accepted on the strength of the table looking plausible. It
was checked by compositing and looking.

`tools/taskvm.py --verify` parses all 221 named scripts: 3,709 part records and
724 commands, no unknown opcode, every script terminating on `ff ff`, every
part's bank selector a multiple of four, every `TASKGOTO` and `TASKDEAD` target
the first byte of a named script, and every `TASKGOSUB` target the exact entry
point of a named routine.

Then the frames themselves. `--composite` walks a script, resolves each part's
bank selector through the creature's bank table above, places it with the mirror
term, and writes a PNG per frame plus a contact sheet. It draws from the baked
pack in `packs/reference`, so it decodes nothing itself and anyone with the pack
can rerun it. The PNGs are derived from the original game, so they default into
`research/shots`, which is gitignored:

```sh
P="--image research/main.final.bin --symbols research/symbols.json"
python3 tools/taskvm.py $P --composite Knight_SwWalkOn
python3 tools/taskvm.py $P --composite Knight_SwSwing
python3 tools/taskvm.py $P --composite Troll_Walk1
python3 tools/taskvm.py $P --composite Knight_SwWalkOn --facing 3 --out research/flip
```

`Knight_SwWalkOn` comes out a coherent thirteen-frame walk cycle. `Knight_SwSwing`
comes out a sword swing with its blade arc. `Troll_Walk1` comes out a troll
mid-stride with its club. `--facing 3` mirrors the whole figure with the parts
still assembled, which is what confirms the `-(x + cel_width)` term; get that
term wrong and the sword detaches from the hand.

That is the test that matters: the earlier guessed record shape passed every
structural test and still did not draw a figure.

Two things to know when compositing something else. The palette defaults to
`palette.forest`, so a creature drawn in it has the wrong colours and the right
shape; pass `--palette` for its own. And a script's name prefix is the
encounter, not the bank table: `Knight_HangSd` is played on the ratman's banks
and `Dragon_Flight*` on a table with `DRAGON5.CEL` in slot 0. The cel bounds
check catches both and says so rather than drawing the wrong sprite, and
`--actor ratmen` puts `Knight_HangSd` right.
