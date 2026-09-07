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
| `ff ff` | end of frame and, **unless a `TASKLOOP` count is running**, end of animation. Clears `task+1`, which is what lets `TASKHANDLE` ask the controller for a new script. The script pointer is left on the `0xff`, so the last frame keeps being drawn |

The terminal form is not unconditional. The `ff ff` branch at `0x99a2` begins
with the same three instructions as the `ff fe` branch at `0x9985`: test the
loop flag, decrement the count, and jump back to the loop resume point if it
is still non-zero. Only when that falls through does it clear `task+1`. This
was found while transcribing the handler for the Rust interpreter. Eleven
scripts reach their `ff ff` with a loop still open and depend on it, among
them `Knight_Burn`, `Beast_BackToss`, `TroggSpear_Toss` and the three
`Balok_*Knight` holds; read the terminator as unconditional and each of those
plays once instead of the stated number of times.

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

## The Rust side

`crates/henge-core/src/taskvm.rs` is the interpreter, and
`crates/henge-formats/src/taskvm.rs` is the reader that turns the image into
its instructions.

### The interpreter

One `Task` per actor: a program counter, the facing, x, y and z, which bank
table `TASKCELBUF` chose, and the 0x1a-byte state record as `VmState`, field
for field with `TaskCommand`. `Task::step` runs from the pointer to the next
end of frame and returns a `Frame`: the task position, the parts to draw, and
the effects. It is integers only, keeps no clock, does no I/O and draws
nothing, which is what lets `Bout::state_hash` fold it in and two machines
agree about it.

Three choices are worth knowing.

* **A script is named, and a jump goes to a name.** The original keeps a raw
  `DS` offset, but every `TASKGOTO`, `TASKDEAD`, `TASKSKIP`, `TASKADDTASK` and
  `TASKSHADOW` target in the data is the first byte of a named script, so the
  exported instruction carries the name. The set is closed: the reader checks
  that every target is a script it exported.
* **Calls out of the machine are effects.** `TASKGOSUB` becomes
  `Effect::Gosub { routine, kind }`; the 37 targets are in `GOSUB_TARGETS` by
  name, with a kind read off the name (`Sound`, `Spawn`, `Gore`, `Control`),
  and a name not in the table comes out `Unknown`. `TASKSOUND` is
  `Effect::Sound { sample }`. Nothing runs them. `TASKTIME`, whose handler
  would spin, stops the task with `Effect::Stalled` instead.
* **A finished script keeps showing its last frame.** The pointer stays on the
  terminal `0xff` as in the original, so a step produces no new parts; the
  task keeps the last non-empty part list and hands it back, rather than
  re-running the frame, which would fire its sounds again every tick.

`TASKJUMP`'s ballistic step is transcribed from `0x9ceb` as it stands,
including two branches that read oddly (the upward form stores the speed into
the y position when the sum is non-negative, and then compares the low byte of
the position against the limit). One script uses it, `Beast_BackToss`, and it
has not been watched running, so it is reproduced rather than corrected.

Placement lives beside the interpreter as `place`: given a part, its bank and
the task position, it returns the frame index in the sheet and the top-left
corner, with the `-(x + cel_width)` term when mirrored. A `Bank` is a sheet
id, the index of the bank's first cel in that sheet, and the size of every
cel, so the simulation can place a blade without ever opening a PNG.

### The reader and the baker

The reader does what `tools/taskvm.py` does, in Rust: finds `mov di, 0x9448`
in `INITTASK`, takes the run of stores after it as the handler table, applies
the link-time correction from `symbols.json`'s `code_shift_map`, reads each
width out of the handler's `add word ptr [di+2], n`, and then compares the
whole set against the table in this document. That comparison is the check
that the image is the right one: a wrong or uncorrected address reads a
different instruction and the widths come out as anything but these.

The baker writes three things into `packs/reference/data/`:

| file | what |
|---|---|
| `scripts.json` | all 221 scripts, as the engine's own `Instr` values |
| `banks.json` | the four bank tables for each of the eleven creature loaders, with a sheet, a frame base and every cel's size |
| `actors.json` | the knight, carrying the closure of the scripts his states reach, his bank tables, which script each state plays, his origin and his frame rate |

Two of those numbers are ours and are marked so in the data's own comments.
The **origin** is where the task's point sits relative to the feet, read off
the standing frame (52 pixels up, for the knight) because this engine places
by the feet and the original places against a point near the head. The
**frame rate**, six ticks a script frame, comes from `Knight_SwWalkOn`, whose
baked offsets advance about 47 pixels over one four-frame stride, at two
pixels a tick. Which script a state plays is also ours: `CONTROLTABLE` is
uninitialised data and not in the load image.

### What the knight does with it

The fighter's five states map onto `Knight_SwStance`, the four
`Knight_SwWalkR` frames cycled in turn, `Knight_SwSwing`,
`Knight_SwShoulderHit` and `Knight_SwDeath`. The swing's hit shape is no
longer a hand drawn line: it is the rectangle of every part the frame flags
`WEAPON`, placed by the same arithmetic that draws it. The data gates it
better than a state check could: the stance carries the sword as a `BODY`
part and only the swing marks it a weapon.

Checked by taking screenshots of the arena every six ticks and reading them
against the `--composite` contact sheets. The walk and the swing match frame
for frame, both facings; the death is the kneel and the collapse; and a traced
practice duel still lands blows, staggers for the length of the recoil script,
and ends in a death.
