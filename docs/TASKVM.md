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
| `PLAY_SFX` | image `0x5964`, what `TASKSOUND` and all 23 sound routines call |
| `PLAYSAMPLE` | image `0x932a`, the sampled path, and `SBSampleTab` at DS:`0x865d` |
| the scripts | DGROUP, as named data symbols: `Knight_SwSwing`, `Troll_Walk1` |

The scripts are ordinary data in the executable's data segment, and the symbol
table names 239 of them. A script pointer is a near offset in DS. (The count was
221 until the bestiary was built: the mudmen's prefix is `Mudmen`, where
`MudmanTABLE` had suggested `Mudman`, so their fourteen scripts and
`Rat_TreeBrush` were being filtered out. All fifteen parse like the rest, and
they bring four more `TASKGOSUB` targets: `AddMudVoice`, `AddMudSound`,
`AddCrushSnd` and `PlayScareMusic`. It was 236 until combat depth was built:
three scripts carry no encounter prefix because no encounter owns them, being
handed to a task the game's own code spawns. `SpeedKnife` and `Knife` are the
thrown dagger, `Blood1` the spray `AddBlood` starts. All three parse and end on
`ff ff`, and `tools/taskvm.py --list` still shows the prefixed 236.)

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
| `0x92` | `TASKSOUND` | 2 | `u8 id`. A sound id, not a sample number: the handler at `0x9b38` loads it into `AL` and calls `PLAY_SFX` (`0x5964`), which translates it per sound device. 131 uses, 54 distinct ids, 31 distinct samples. See [Sound](#sound) |
| `0x94` | `TASKSAVE` | 6 | `u8 mode, i16 field, u16 value`. Store into the actor record at `actor+field`; mode bit 0 stores a byte, otherwise a word |
| `0x96` | `TASKSHADOW` | 4 | `u8 on, u16 script`. Sets `actor+0xe` to the shadow script and `actor+0xc` to on/off. Eleven uses; the four non-zero targets are `Ratman_Shadow`, `Beast_KnightShadow`, `Dragon_Shadow`, `Balok_Shadow` |
| `0x98` | `TASKGOSUB` | 4 | `u8 _, u16 routine`. Near call into the game's own code with x, y, z, facing, the bank table and the actor in registers. 141 uses; all 41 distinct targets resolve to named routines |
| `0x9a` | `TASKDEAD` | 4 | `u8 _, u16 target`. Branch to `target` and clear the VM state if `actor+0x38` (hit points) is <= 0. All 20 distinct targets are death scripts |
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
| `ff ff` | end of frame and, **unless a `TASKLOOP` count is running**, end of animation. Clears `task+1`, which is what lets `TASKHANDLE` ask the controller for a new script. The script pointer is left on the `0xff` |

The terminal form is not unconditional. The `ff ff` branch at `0x99a2` begins
with the same three instructions as the `ff fe` branch at `0x9985`: test the
loop flag, decrement the count, and jump back to the loop resume point if it
is still non-zero. Only when that falls through does it clear `task+1`. This
was found while transcribing the handler for the Rust interpreter. Eleven
scripts reach their `ff ff` with a loop still open and depend on it, among
them `Knight_Burn`, `Beast_BackToss`, `TroggSpear_Toss` and the three
`Balok_*Knight` holds; read the terminator as unconditional and each of those
plays once instead of the stated number of times.

All 236 scripts end on `ff ff`.

**A pointer left on the `0xff` draws nothing.** `PerformCOMMAND` builds a frame
by walking the script from `task+2` and emitting a part for every record it
passes; its very first test is `cmp ax, 0xff`, and a pointer sitting on the
terminator jumps straight to `0x993a` and comes back having emitted none. In the
game that is a single tick: `task+1` is clear, so `TASKHANDLE` hands the actor
its next script at once and there is never an actor standing on a dead pointer.
`INTR.EXE` has no controller behind its figures, and its own copy of the walk at
`0x3fbe` behaves the same way, so **an intro figure whose script runs out simply
leaves the picture**. That is how the druids walk out of shot at the left of the
forest rather than piling up against the edge.

## Sound

`TASKSOUND` is how the game makes noise, and there is nothing else to it. The
handler is four instructions:

```
9b38  mov  al, byte ptr [si+1]     ; the byte after the opcode
9b3b  call 0x5964                  ; PLAY_SFX, after the link-time correction
9b3e  add  word ptr [di+2], 2      ; and on to the next record
9b42  ret
```

The `call` is the example the [link-time correction](#one-more-thing-about-the-link-time-correction)
section is about: read naively it lands on `0x5797`, which is mid-instruction
inside `MoveBACKR`; corrected it lands on `0x5964`, which is exactly where all
23 of the named sound routines (`AddTrollSND`, `DrFireSnd`, `GuardianYellSnd`)
also call, and they need no correction because they are in the same shift
segment.

Nothing gates it. No distance, no channel count, no priority, no volume beyond a
constant, and no test for the same sound already playing. `PLAY_SFX` is:

```
5964  push ...
596d  cmp  word ptr [0x8645], 3    ; SFXTYPE: 0 speaker 1 AdLib 2 Roland 3 SB
5972  je   0x5989
5974  mov  bx, 0x7d02              ; the FM effect table
5977  xlatb                        ; al = [bx+al]
5978  mov  bl, 0x0f                ; volume, a constant
597a  mov  ah, 0x40
597c  and  al, 0xff                ; sets flags that nothing reads
597e  cmp  word ptr [0x8645], 3
5983  je   0x5989
5985  int  0x61                    ; the music driver's effect call
5987  jmp  0x5994
5989  mov  bx, 0x7e6e              ; the sample table
598c  xlatb
598d  mov  ah, 0
598f  and  al, 0xff
5991  call 0x932a                  ; PLAYSAMPLE
5994  pop ...  /  ret
```

So **a script's operand is a sound id, not a sample number**. Two 364-byte
tables translate it, one per kind of device: DS:`0x7d02` (image `0x1a0b2`,
`Soundfxtable`) for the FM path and DS:`0x7e6e` (image `0x1a21e`) for the
sampled path. Entries `0x00` to `0x6c` are meant; from `0x6d` to the end both
tables are filled with `0x15`, and five of the ids the shipped scripts use
(`0x6d`, `0x87`, `0x8f`, `0x92`, `0x93`) land in that filler and play `hit3`.

`PLAYSAMPLE` at `0x932a` takes the translated number as an index into
`SBSampleTab` (DS:`0x865d`, 49 four-byte EMS page-and-offset records), copies
16KB out of that page and hands it to the driver with `bx = 6`. The 49 names are
`SBFileTable` (DS:`0x8651`, image `0x1aad1`), which is `SAMPLES/` in alphabetical
order: `baland` to `wipcrak`. **Entries 8 and 9 are both `cheap2b`**. The string
`cheap1b` does not occur in the image at all, though `SAMPLES/CHEAP1B` is on the
disk, so the shipped game loads `cheap2b` twice and sample 9 and the `CHEAP1B`
file are unreachable.

### Every sound in the game

Exactly 26 code sites call `PLAY_SFX`, found by scanning the image for `e8`/`e9`
displacements that resolve to `0x5964`:

| | |
|---|---|
| `0x9b3b` | the `TASKSOUND` handler: **131** commands in the 239 named scripts (130 in the 236 with an encounter prefix, plus one in `SpeedKnife`), **54** distinct ids, **31** distinct samples |
| 23 sites | the sound routines the scripts call through `TASKGOSUB`, listed below |
| `0xb338` | `ShakeDiceSnd`, id `0x10`, unless `MUSICTYPE` (DS:`0x8643`) is 2, which is the Roland |
| `0xd50a` | `AddClickSound`, id `0x0f`, unconditional, seventeen callers |

That is all of it. There is no footstep routine and no swing routine: the
knight's walk scripts carry no sound command at all, and his swing's swish is
`TASKSOUND 0x0b` on the second frame of `Knight_SwSwing`.

Across all 26 sites, 44 of the 49 samples are reachable. The five that are not
are `camel4`, `cheap2b` (the duplicate), `grnt3b`, `mud2` and `nitland`, and two
of those five are explained by the two dead routines below.

### The 23 sound routines, and the three that are silent

All of them are three to seven instructions and all are transcribed in
`crates/henge-core/src/sound.rs` with their addresses. Five roll `_WIZARD:RND`
(`0xbd89`), the one shift register at DS:`0xe22f` the controllers also roll, so a
sound moves it exactly as the original does.

Three are silent in the shipped image, and are translated as silent:

* `KAudio0` / `KAudio1` (`0x5827`) and `HengeThunderSnd` (`0xb419`) are each a
  bare `ret`. Four script calls between them, all silent.
* `KnightGruntSound` / `KnightStruckSound` (`0x3d5a`) cycles a counter at
  DS:`0x77a2` and returns `cnt + 4`, which is one of the grunt ids 4 to 8, and
  then returns. The `jmp PLAY_SFX` at `0x3d73` that would play it is after an
  unconditional `ret`, nothing in the image branches to `0x3d73`, and all ten of
  the scripts' gosubs carry `0x3d66`, the link-time offset of `0x3d5a`. So the
  knight computes a grunt and plays nothing, and `grnt3b` (id 6) is reachable
  only through it. Whether the byte was a patch to quieten the grunts or a
  mistake cannot be read off the image.

Two more are quirks rather than silence, and are also translated as they are:

* `AddRoar` (`0x3d79`), which `TroggRoarSound` (`0x3d76`) and `BalokRoarSound`
  (`0x3e01`) jump into, advances an index at DS:`0x77a4`, tests the indexed entry
  for the `-1` terminator, and then `pop si` **before** `mov ax, [si]`. The index
  is thrown away with the `push`, so the id played is always entry zero. For the
  balok that is invisible: all eight entries of its table translate to
  `lion2c1`. For the trogg it means every one of its thirteen roars is
  `camel3b`, and `camel4` is unreachable.
* `AddCrushSnd` (`0x5816`) increments a counter at DS:`0x7b9e` that nothing else
  in the image reads, and then does `and ax, 3 / jne ret` without loading `AX`
  first. What it tests is what the `TASKGOSUB` handler left there, which is the
  task's own x, so the mudman's crush is heard on three columns in four.

And one lies about what it is: `PlayScareMusic` (`0x57cc`) plays no music. It
rolls and plays sound id 8 or 9, which are `grnt3` and `headchop`.

### Where it goes in Rust

`Task::step` emits `Effect::Sound { sample }` carrying the id. `Bout::step_with`
turns that, and the ids the `Sound`-kind gosubs return, into `bout.sounds`, a
list of `SoundCall { who, id }` cleared every tick beside `parries`. A task put
in the arena mid-tick (`KnifeThrow`, `AddBlood`, `AddDragonFIRE`, `TASKADDTASK`)
shows its first frame where it is built, so `Bout::launch` collects that frame's
sounds too: `SpeedKnife`'s swish is on its first frame and would otherwise be the
one sound in the game nothing played. The desktop reads `bout.sounds`, turns each
id into an asset id with `henge_audio::sfx`, and plays it. `--sounds` prints one
line per sound with the tick, the id, the sample and the script that asked, which
is how the wiring is checked without listening to it.

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
| `0x01` | also push this part onto `BodyPile`, the list the collision code walks (615 records) |
| `0x02` | also push it onto `WeoponPile` (156 records) |
| `0x10` | blit it a second time into the buffer at segment `0xac00` as well as the current target (176 records) |
| `0x40` | do not fold this part into the actor's bounding box (`FindWidth`) (227 records) |
| `0x80` | drop the part entirely, piles included, when the DS:`0x700` mode flag is set (315 records) |
| `0x04`, `0x20` | occur in the data (35 and 462 records) but are read by nothing in `PerformCOMMAND`. `0x08` never occurs |

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
  startup. It gates `TASKSKIP`, the part flag `0x80`, and a few combat
  decisions: when it is set, `TASKSKIP` diverts four of its five targets to the
  bloodless `*_CollapseDead` scripts, and the flag `0x80` parts are dropped.
  **This entry used to say nothing else in the image writes it, and that was
  wrong**: the linear disassembly the claim rested on had lost sync before the
  title code. `OSWITCHES` (image `0x1352`) does `xor word ptr [0x700], 1` when
  the option cursor is on row 1, and `DisplaySelect` (`0x13a5`) prints `TEXTON`
  for zero and `TEXTOFF` otherwise. So it is the title's gore switch, zero is
  gore on, and the shipped game starts with the gore on. `GORESWITCH`, the
  `PUBLIC` name with no address, is presumably this word. The combat decisions
  it reaches are `TroggAttack` (the finisher on a fallen knight only with the
  gore on), `TroggHit` (the spear's toss of the corpse), `KnightHitKnight`
  (the decapitating swing carries through) and `BeastStruck1` (the impale).
* **`TASKJUMP`'s first operand**, stored at `TaskCommand[0x10]`. Nothing in
  `_TASK` reads it back.
* **Part flag bits `0x04` and `0x20`.** Present in the data, read by nothing in
  the interpreter. Probably vestigial from the Amiga original.
* **The eighteen non-sound `TASKGOSUB` kinds.** `Spawn`, `Gore` and `Control`
  beside a name in `GOSUB_TARGETS` are still read off the name. The 23 marked
  `Sound` are not: every one of those bodies has been disassembled, and they are
  in [Sound](#sound) and in `crates/henge-core/src/sound.rs`.

## How it was checked

The decode was not accepted on the strength of the table looking plausible. It
was checked by compositing and looking.

`tools/taskvm.py --verify` parses all 236 named scripts: 3,957 part records and
774 commands, no unknown opcode, every script terminating on `ff ff`, every
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
and `Dragon_Flight*` on `DrBuffer`, which is `MI.C` in every slot. The cel
bounds check catches both and says so rather than drawing the wrong sprite, and
`--actor ratmen` and `--actor dragon_flight` put them right.

### `DrBuffer`: the fifth bank table, and the loader nobody had found

`Dragon_Flight1`..`8` do not run on any of the four `TASKCELBUF` chooses
between. `_MAP:ContinueDragon` (image `0xa5b3`) builds a table of its own at
DS:`0xccbc` by writing the far pointer at DS:`0x8975` into **all five** of its
slots, points the dragon's task at it, and drives it from `DrAnim` at
DS:`0xcc22`, sixteen words holding each of the eight flight scripts twice, which
`DragonDONE` indexes with `[si+0x0a] & 0xf`.

The pointer at DS:`0x8975` is `MI.C`, the map icon bank, and the record had it
as `DRAGON5.CEL`. The loader at `0x88b5` is what settles it: it stores the
address the **next** file will be loaded at, not the one it has just loaded,
because `LOADFILE` advances `di` past what it copied and `AdjustAddr` rounds it
up into `es` with `di` zeroed. So the store that follows `ki.cel` names where
`mi.c` will go. Cels 34 to 41 of `MI.C` composite to a dragon seen from above
with its wings beating; the same eight cels of `KI.CEL` are a mixture of a
moon, a scroll and some five-pixel bars. `DRAGON5.CEL` is slot 4 of the
creature table and it is the fire, which is what `Dragon_Fire` draws from.

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
  `Effect::Gosub { routine, kind }`; the 41 targets are in `GOSUB_TARGETS` by
  name, with a kind read off the name (`Sound`, `Spawn`, `Gore`, `Control`),
  and a name not in the table comes out `Unknown`. `TASKSOUND` is
  `Effect::Sound { sample }`, carrying the id the handler would have handed
  `PLAY_SFX`. The interpreter runs neither: `henge_core::sound` is where the 23
  sound routines are transcribed, `henge_core::bout` collects the ids as
  `bout.sounds`, and `henge_audio::sfx` is the only thing that knows what an id
  means. `TASKTIME`, whose handler would spin, stops the task with
  `Effect::Stalled` instead.
* **A finished script keeps showing its last frame.** The pointer stays on the
  terminal `0xff` as in the original, so a step produces no new parts; the
  task keeps the last non-empty part list and hands it back, rather than
  re-running the frame, which would fire its sounds again every tick. The
  original draws nothing at all in that state, but never stays in it: the
  controller replaces the script on the same tick. Where there is no
  controller, as in the intro, the original's own behaviour is the one to have,
  and `henge_core::content::IntroCast::frame_at` returns nothing rather than the
  last frame.

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
| `scripts.json` | all 239 named scripts (the 236 with an encounter prefix, plus `SpeedKnife`, `Knife` and `Blood1`), as the engine's own `Instr` values, sound commands included |
| `banks.json` | the four bank tables for each of the eleven creature loaders, with a sheet, a frame base and every cel's size |
| `actors.json` | the knight and the ten creatures, each carrying the closure of the scripts its states reach, its bank tables, which table a task starts on, which script each state plays, its origin, hit box and girth read off its standing frame, its frame rate, and the numbers from its `Set*Tables` routine |

Two of those numbers are ours and are marked so in the data's own comments.
The **origin** is where the task's point sits relative to the feet, read off
the standing frame (52 pixels up, for the knight) because this engine places
by the feet and the original places against a point near the head. The
**frame rate**, six ticks a script frame, comes from `Knight_SwWalkOn`, whose
baked offsets advance about 47 pixels over one four-frame stride, at two
pixels a tick. Which script a state plays was thought to be ours too, because
`CONTROLTABLE` is uninitialised data; it is not, and the next section says
where it was found.

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

## The controller tables, and the stat block

The tables that say which script an actor plays for which purpose are `BSS`,
which is why the load image shows nothing at their addresses. They are filled
by two routines in `MOON` with a run of `mov word ptr [si+off], imm16`, and
those immediates are the tables. `SetKnightAnims` (image `0x1771`) fills the
knight's; `SetMonsterAnims` (`0x186b`) fills every creature's. Read out of
the code with the same disassembler that read the handlers:

| table | indexed by | holds |
|---|---|---|
| `*Wal` (`KnightWalSw`, `TroggWalAxe`, `TroggWalSp`, `TroggWalHammer`, `TrollWal`, `MudmenWal`, `RatmenWal`, `DragonWal`, `BeastWal`) | walk frame, times two, plus `WalkOFFSET` | the walk cycle: right-facing scripts at +0, up at +0x10, down at +0x20, each row zero terminated |
| `*Att` (`KnightAttSw`, `TroggAttAxe`, `TroggAttHammer`) | attack kind | the attack script: knight 2 lunge, 4 swing, 6 knife, 8 block, 0xa right thrust, 0xc up thrust, 0xe evade, 0x10 chop |
| `*Hit` (`KnightHitSw`, `TroggHitSp`, `TroggHitAxe`, `TroggHitHammer`, `BeastHit2`, `RatmenHit`, `DragonHit`, `MudmenHit`, `TrollHit`) | the attacker's attack kind, `[attacker+0x28]` | the blow-taken script, read in `TroggStruck` as `[[victim+0x14] + kind]` |
| `*Dam` (`KnightDamSw`, `TroggDamAxe`, `TroggDamHammer`, `RatmenDam`, `TrollDam`, `MudmenDam`, `BalokDam`, `DragonDam`) | attack kind | the damage of that attack, before `CalcDamage` adds strength and weapon |
| `*Blo` (`KnightBloSw`) | the attacker's attack kind | **the block table**, not the blood: the defender's kind that stops that attack, read by `CheckBlock` through actor `+0x1e`. Chop, lunge and rear thrust are stopped by the evade (0xe), the swing by the block (8); the rest of the row is zero |

The walk rows, exactly as filled:

```
knight     R1 R2 R3 R4 / U1..U4 / D1..D4         (Knight_SwWalk*)
trogg axe  R1 R2 R3 / U1..U4 / D1..D4            (also hammer, also spear)
troll      Walk1 Walk2 Walk3 Walk4               (no up or down row)
mudmen     Move1 Move3 Move1 Move2               (no up or down row)
ratmen     Roll1..Roll4 / Leap1..Leap4           (no down row)
dragon     LiftHead1..5, 5, 5, 5 / LowerHead1..5, 5, 5, 5   (rows at +0x10 and +0x20 only)
beast      copied from BEWAL, in the first 2,906 bytes of DGROUP, unreadable until the
           unpacker was fixed; see REVERSING.md, and this has not been re-read since
```

Two slips in the original are worth knowing before anyone reads a table as
gospel. `TrollHit` is filled with the address of `TrollHit` itself, eight
times, where every other creature's `*Hit` holds a script; and `Demon_Slap`
on disk runs straight on into `Demon_Zap` and `Demon_Whip`, so a parse that
stops only on `ff ff` reads all three as one script, though a `TASKGOTO` to
`Demon_Stance2` after the slap's fifth frame ends it before that matters.

**The stat block** is written by a `Set*Tables` routine each `InitKnightvs*`
calls, into the actor record. The offsets, read off the code:

```
+0x10 stance script      +0x38 hit points          +0x52 approach range
+0x12 recovery script    +0x3c maximum hit points  +0x54 back-off range
+0x14 *Hit table         +0x35 kind                +0x56 plane tolerance
+0x16 *Att table         +0x18 bank table (0x8933 knight, 0x8949 creature)
+0x1a *Dam table         +0x1c *Wal table
```

The three ranges are what `MonsterTrack` reads: `CheckZAxis` is on the same
plane when the z difference is within `+0x56`; `CheckXAxis` against `+0x54`
goes to `TrackBack`, which sets the walk bit away from the knight; against
`+0x52` it returns "in range" with no walk bit set; further out,
`TrackOpponent` sets the walk bit towards him. What each creature does once
in range is its own routine, and all of them are now built; the next section
is the account of them.

| creature | `+0x38` | `+0x3c` | `+0x52` | `+0x54` | `+0x56` | `+0x35` | damage |
|---|---|---|---|---|---|---|---|
| knight | derived | derived | 100 | 80 | 4 | 6 | lunge 3, swing 4, knife 3, right thrust 2, up thrust 3, chop 4 |
| trogg with axe | 20 | 20 | 100 | 90 | 5 | 0xc | 3 |
| trogg with hammer | 20 | 20 | 70 | 65 | 5 | 0xe | 2 |
| trogg with spear | 15 | 15 | 130 | 120 | 5 | 0x10 | no table |
| beast | 10 | 10 | 2 | 1 | 5 | 0 | no table |
| ratmen | 5, 7, 12 by moon | same | 40 | 30 | 5 | 0x12 | 3 and 1, rising to 6 and 3, 8 and 5 |
| dragon | 200 | 120 | 60 | 20 | 5 | 0xa | 10 lunge and right thrust, 30 swing and chop |
| dragon's claws | 50 | 50 | | | 10 | 0x16 | |
| Balok | 30 | 30 | 80 | 60 | 10 | 0x18 | 4 |
| mudmen | 30 | 30 | 80 | 75 | 5 | 2 | 2 |
| troll | 40 | 40 | 150 | 90 | 5 | 0x20 | 3 |
| demon | 250 | not set | 95 | 90 | 2 | 4 | no table |

The knight's `Set*Tables` also writes 100, 80 and 4, and his walk speed is in
`BKnightWALKR`: 25, 3, 23, 4 pixels over the four frames, 55 to the 47 that
the frame rate above was read off `Knight_SwWalkOn`, which is close enough
that six ticks a frame stands. The creatures'
are `TroggWALKR` (0, 7, 23), `TrollWALKR` (16, 26, 13, 26), `MudmenWALK`
(12 across and 12 deep, then 10 and 14) and `BeastChargeOffsets` (33, 27,
17, 33). Every `InitKnightvs*` also writes 6 into `DELAY`; nothing in the
image reads it back, so whether it is the frame delay is not known, and the
six ticks a frame stands on the walk measurement rather than on it.

What the pack does with all this: each creature's `walk` is its `*Wal` right
row; its `hurt_by` is its `*Hit` row by the knight's attack kind, and its
`hurt` the entry for a swing; its `death` is where that script's own
`TASKDEAD` goes, and the other entries' deaths follow their own branches; its
`health`, `damage`, `approach`, `back_off` and `depth_tolerance` are the row
above; its `attacks` are the scripts its routine picks, by the kind that
routine writes into `+0x28`. Its default `attack` is a choice among its own,
and `reach`, `speed` and `bounty` are ours; the baker says which beside each.

## The knight's controller

Read for build order items 46 to 49. The routines are in `MOON`, the
controller table at DS:`0x6964` is filled by `InitGameStart` and indexed by the
actor's kind (`+0x35`; the knight is 6), and the task loop at image `0x9702`
calls the controller whenever the actor has hit something (`+0xc`), been struck
(`+0xe`), or its animation has ended (`task+1` clear). The controller returns
the next script in DS:`0x783a`, with `0xffff` meaning carry on and zero meaning
remove the task.

**`KnightAttack`** (`0x4098`): the input word at `+0x26` holds 1 right, 2 left,
4 down, 8 up, 0x10 fire. Fire goes straight here, before any walking. The fire
bit is stripped, the rest doubled, and `Rjoystick` (DS:`0xc32`) or `Ljoystick`
(DS:`0xc48`) read by the facing at `+8`. The two tables are each other's
mirror, so over forward and back:

```
                 up           level          down
forward          0xc UThrust  0x4 Swing      0x2 Lunge
neither          0x10 Chop    0 (stance)     0xe Evade
back             0x6 Knife    0xa RThrust    0x8 Block
```

The result is stored at `+0x28` and indexes `KnightAttSw` (`+0x16`) for the
script. Slot 0 is `Knight_SwStance`, so fire with no direction plays one frame
of standing. `Knight_SwDThrust` and `Knight_SwOThrust` are not in the base
table; `InitKnightvsRatmen` puts them in the block and evade slots for that
fight, and `InitKnightvsTroggSpear` puts `Knight_SwEvade` in both.

**`CheckBlock`** (`0x420d`), with `di` the struck knight and `si` the
attacker:

```
blockflag = 0
if KnightBloSw[attacker.kind] != knight.kind: return
blockflag = 1
if knight.kind == 0xe (evade):
    blockflag = 0
    if !(knight+0x48 & 0x80): blockflag = 1; knight+0x48 |= 0x80
    return
if knight.facing == attacker.facing: blockflag = 0
```

Bit 7 of `+0x48` is cleared in `ControlKnight` on any step that moved him. Only
`TroggStruck1`, `TroggSpearStruck1`, `KnightKnightStruck1` and `BKnightStruck`
call it; the other `*Struck1` routines subtract their damage straight off. A
blocked blow replays the knight's own `KnightAttSw[kind]` and takes nothing
off; against the spear it plays `Knight_SwEvade` whatever he held.

**The struck table.** `KnightGotStruck` dispatches on the attacker's kind
through `StruckTable`: beast, mudmen (which is also the generic
`KnightStruck1`), demon, knight and hero, dragon, the two troggs, the spear
trogg, ratmen, the dragon's fire, the claw, Balok, the knife (kind 0x1a), the
troll. The generic one calls `CalcDamage` (the attacker's `*Dam` entry, plus
strength, plus 2, 3 or 5 for the three better swords, doubled for a chop,
doubled again by a talisman) and then plays `KnightHitSw[attacker.kind]`. When
the knight is already down it plays `Knight_SwCollapse`, or `Knight_SwDeCap`
for a swing; `KnightKnightStruck1` plays the decapitation for any blow.
`TroggStruck1` decapitates for the axe trogg and collapses for the hammer;
`TrollStruck1` takes seven and plays `Knight_Explode` for a chop on a dead
knight.

**What a landed blow does to the attacker.** `KnightHitSomething` runs when
`+0xc` is set: the next script is the recovery at `+0x12` (`Knight_SwRecover`;
every creature's is its stance, Balok's `Balok_Recover`, the beast's
`Beast_TurnAround`), except for an up thrust, a blow on a knight who is
evading, or a swing on a dead knight with the gore on, which carry through.

**The knife.** `Knight_SwKnife` opens with `TASKTESTEQ` on the byte at
`+0x34`, the dagger count `SetKnightEquipment` sets to ten, and goes to the
stance when it is zero. Its last frame calls `KnifeThrow` (`0x3e28`): one off
`+0x34`, then a task of kind 0x1a with `KnifeDam` (3, 3, 3, 3) at `+0x1a` and
kind 6 at `+0x28`, started on `SpeedKnife` (DS:`0xe68`: sound 0x0b, five
pixels forward, the blade `KN4.OB` cel 10 at 66 and cel 13 at 12) at the
knight's position and facing on his bank table. `ControlKnife` hands it `Knife`
(DS:`0xe80`: twenty pixels forward and cel 10 at 61) every frame until `+0xc`
is set or its x passes `0x14a` facing right or falls below -10 facing left.

**Blood.** `AddBlood` (`0x57a8`) is called by `TrollStruck`, `BalokStruck` and
`DragonStruck` and by nothing else. It starts a task of kind 0x14
(`ControlMisc`, which lets a script run to its `TASKKILLTASK`) on `Blood1` at
the strike point the collision code left in the victim's `+0x58` and `+0x5a`,
facing as the knight does, on bank table 4. `Blood1` is four frames of forty
parts on `BLO.CEL`, every one of them flagged 0x80.

## The creatures' own controllers

Read for build order items 33, 36 and 37, and transcribed into
`henge-core/src/monster.rs`. `CONTROLTABLE` at DS:`0x6964` is filled by
`InitGameStart` and indexed by the actor's kind (`+0x35`); the task loop calls
the entry for an actor when it has hit something (`+0xc`), been struck
(`+0xe`), or its animation has ended (`task+1` clear). A controller answers by
writing the attack kind into `+0x28` and the next script into DS:`0x783a`,
where `0xffff` means carry on and zero means remove the task.

Two things follow from that shape and are reproduced. A creature **names its
own script**, so in henge it hands the fighter an `Order` and the joystick path
is never reached; and a controller runs **once per animation frame**, not once
per tick, so a cooldown of ten is ten frames. The state each keeps is the actor
record's own: `+0x0a` the walk frame, `+0x0b` a timer, `+0x48` and `+0x49`
flags, `+0x4a` a cooldown. That is a `Brain` on the fighter here, and it is
folded into `Bout::state_hash` with everything else.

**`MonsterTrack`** (`0x56d9`) decides only where a creature wants to be.
`FaceKnight` first; `CheckZAxis` puts them on the same plane when the depth
difference is within `+0x56`, and off it the walk bit for depth goes on and the
tracker answers "still walking". Then `CheckXAxis` against `+0x54` sends it to
`TrackBack`, which walks *away*; against `+0x52` it answers "in range" with no
bit set; and beyond that `TrackOpponent` walks towards. `CheckXAxis` leaves
`bx` holding the absolute distance, which every caller then uses.

Every controller opens with `mov ax, [KnightTable]; mov [Opponent], ax`. **A
creature's opponent is the knight and nothing else**, which is why a dragon
does not take its own claws for an enemy.

| creature | what its own routine does |
|---|---|
| trogg, axe or hammer | `TroggAttacks`: the overhead from 100 to 120, the swing inside 100, ten frames of the stance between blows and the next on the eleventh (0x2ea7 is `cmp; je; sub; jmp tail`). A `GETPERCENT` roll of 30 or under chops instead when the knight's `+0x28` is 8, the block, which is the guard the chop gets through. `TroggAttack` gives ground inside `+0x54`, and comes in for a fallen knight's head inside 100 with the gore on, raising `DeCapFLAG` as it decides (0x2e9f). `TroggStruck` zeroes the count (0x2f1c), `TroggHit` sets it to ten (0x2f55); neither re-faces. `FaceKnight` runs on every other pass, through `MonsterTrack`, and it is the only thing that turns a trogg; walking away plays the cycle backwards through `MoveBACK` (0x5783) and leaves it facing him |
| trogg, spear | the same routine's kind 0x10 branch: one lunge, only inside `+0x52`, and twenty frames |
| troll | `TrollAttack`: `Troll_Bunt` inside 100, `Troll_Chop` from 100 to 150, and never two chops running, because it compares `+0x28` before it picks |
| ratman | `ControlRatCollide` does not use the tracker at all. Slash inside 40, bite from 40 to 50, leap beyond, and `RatmanHit` sets fifteen frames of `HitDelay` when a blow of its own lands |
| mudman | `ControlMudmen`: reach between 75 and 100, and inside that `MudmenIBury` takes it under the ground; `MudmenAppear` brings it up 75 pixels to the knight's far side. `MudmenHit2` **takes hold of him**: the knight's own task is removed and he is drawn inside `Mudmen_EntangleKnight` for the forty frames `+0x0b` counts, and only fire and down together (`test bx, 0x10`, `test bx, 4`) tear him loose. `MudmenChoke` is `KillKnight` |
| balok | `ControlBalok`: hops to close, uppercut from 70 to 80, grab out to 120, then stands off until 180 or until the knight has a dagger on his belt (`[bx+0x34]`) |
| beast | `ControlBeast` never tracks. `BeastCharge` runs it to the arena edge and turns it round; `SetBEASTZ` alternates a line dead on the knight with one up to 28 rows off, on `BeastFLAGS` bit 0; `SetBeastTimer` waits `RND & 0xf | 5` frames off the edge |
| demon | `DemonAttack`: slap inside 100 (kind 0x10, nine frames, and two turns of `demonbodge` between slaps), zap out to 130 (kind 4, six), whip out to 140 (kind 2, five). The whip then walks four `DemonFLAGS` bits through `Demon_OWhipMiss`, `OWhipHit`, `UWhipMiss` and `UWhipHit`, swapping in `OWhipKnight` or `UWhipKnight` when the crack lands between 120 and 140 or 130 and 150, and handing the caught knight `Knight_SwSlapped` outright. `Demon_Zap` calls `KnightOFF` and `KnightON` from its own frames: ten off him, off the board, and back 0x89 pixels to the demon's side |
| dragon | `ControlDragon`, item 36 and section 3.3 of `COMPLETE.md` |
| claw | `ControlClaw`: slaps whatever comes inside x 100 on its plane, **never calls `CalcDamage`**, and plays `Dragon_ClawDead` when the dragon's hit points are gone |

`TroggAttacks` is the only routine in the set that rolls. `_WIZARD:RND`
(`0xbd89`) is eight rounds of a shift register over one word at DS:`0xe22f`:
each round takes `ror(seed, 3) ^ seed`, keeps bit 1 of it as the bit shifted
into the top, and shifts the seed right one. `GETPERCENT` (`0xbda9`) masks the
result to seven bits and subtracts 27 from anything at 100 or over, giving
0 to 99. Both are transcribed; the register belongs to the bout rather than to
the process, so a fight replays and two machines roll the same way.

## `SETDEMONBORD`, and what a border actually is

The last routine in `GFX` sits at image `0x7ff9`, after the word `CLIPHIEGHT`
at `0x7ff7` and before `INSTALLKBD` at `0x8025`. It is nine stores:

```
es <- [0x88ff]           the segment the arena's own .T file was loaded into
es:[0] <- 1              one border record, big-endian
es:[2] <- 0              left
es:[4] <- 309            right
es:[6] <- 99             bottom
es:[8] <- 10             top
[0x80b5] <- 99           the deepest border row, which is where the ground starts
```

That segment is the arena. The loader at `0x8d93` reads a count out of its
first word and skips that many eight-byte records before the tile placements,
and walks them again taking the deepest `rec+4` into DS:`0x80b5`, which
`FindHalfBORD`, `FindQuarterBORD` and `Find3QuarterBORD` divide up. `SBORD`
(`0x4552`) walks the same list every frame and clears the walk bits in `+0x26`
that would carry an actor across one. So **a `.T` file's header is a count and
a list of border rectangles, not one walkable box**, and a rectangle is ground
you may not stand on rather than ground you may.

**Correction, and it is two corrections.** The earlier note here said every
shipped arena holds exactly one record. Four do not: `FO7`, `SW6` and `SWL2`
hold two and `GLL4` holds three, and reading those four as one record threw
their scenery away. See `docs/FORMATS.md`.

And the earlier note said nothing calls `SETDEMONBORD`. Two things do, and the
claim was an artefact of reading `call` displacements without the link-time
correction this document ends by warning about. Corrected, `0x2752` in
`InitKnightvsDemon` and `0x8cb5` in `GENERATELANDSCAPE` both resolve to it.
`InitKnightvsDemon` calls it **after** `0x2746` has loaded the arena, so there
the record really does replace the arena's list and the demon is fought on 0 to
309 across and 10 to 99 deep; `GENERATELANDSCAPE` calls it **before** it
dispatches to the family's own loader, so there the write is overwritten by the
`.T` a moment later. That second caller is worth knowing: it makes the routine
read more like "write the default border" than like anything to do with the
demon, and the demon path is simply the one place it is called where it
survives.

Here it is the demon's own `ActorDef::border`, applied by
`Bout::apply_actor_borders`, which replaces the arena's list rather than
intersecting with it, and applied before the fighters are stood up so that the
three standing places are measured from the demon's floor rather than the
arena's.

The name still rests on two things and neither is a name in the file: the
routine is the last in `GFX` and `SETDEMONBORD` is the last of the twenty one
`GFX` publics, in the blob's own order; and it writes a border record and
nothing else. With `GENERATELANDSCAPE` calling it too, treat the name as
weaker than that argument made it look, and the behaviour as certain.

## One more thing about the link-time correction

`docs/REVERSING.md` records that stored code addresses need the seven-step
correction. So do **`call` and `jmp` displacements**, which is easy to miss
because a branch inside one module needs no correction at all and most of the
interesting reading is inside `MOON`. A displacement is assembled in link space,
so the target is `fix((next_ip - shift_of_caller) + rel)`. Uncorrected, the
call at `0x243e` in `InitKnightvsDragon` lands three bytes inside an
instruction in the middle of the swamp generator; corrected, it lands exactly on
`LOADDRAGON` at `0x8b7a`, which loads `Dragon1.cel`, `Dragon2.cel` and
`DRAGON5.CEL`. The same correction on `mov bx, 0x8ea4` in `GENERATELANDSCAPE`
turns a stretch of code into the four-entry dispatch table it is.
