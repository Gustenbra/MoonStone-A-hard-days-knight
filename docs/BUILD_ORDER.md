# Build order

Every item, once each, in the order you would actually do it. One flat list.

`COMPLETE.md` is the same work organised by subsystem, with the original's function names
against each part. This file is the checklist.

**77 items. 20 done, 57 remaining.**

Ordering is by dependency, not preference. Where two items do not depend on each other they
are grouped in the same phase and can go in any order, or in parallel.

---

## Phase 0: done

The engine and a vertical slice. Roughly a quarter of the game.

- [x] 1. Unpack the original executable, by running its own decompression stub
- [x] 2. Decode the packing used by every container
- [x] 3. Decode PIV / CMP / .P full-screen images and palettes
- [x] 4. Decode CEL / OB / F / FON / .C sprite banks
- [x] 5. Decode .T arena layouts, including signed prop coordinates
- [x] 6. Decode COLLIDE.HIT hit lines
- [x] 7. Decode VOC samples
- [x] 8. Asset packs: logical ids, pack stacking, provenance, shippability check
- [x] 9. The baker: original files to indexed PNG, WAV and JSON
- [x] 10. Palette-indexed 320x200 framebuffer, presented at 4:3
- [x] 11. Sprite drawing: transparency, mirroring, masking, colour substitution
- [x] 12. Text: glyph map read off the artwork, silhouette rendering
- [x] 13. Arenas: 57 of them, scenery, walkable bounds, depth sorting
- [x] 14. Combat: positional hit lines, committed attacks, damage, death
- [x] 15. Bouts of up to four fighters, in the simulation, deterministic and serializable
- [x] 16. Overworld: travel, day cycle, ambushes, terrain classification
- [x] 17. Runs: wounds carry, travel mends, death ends the run
- [x] 18. Four locations with menus, and a working healer
- [x] 19. Sound: 49 clips, four cues, silent without a device

---

## Phase 1: the blockers

Research, not construction. **Could take a day or a month**; nothing makes that
predictable. Everything in phases 2 and 4 waits on these.

Item 20 turned out to be a negative result: the thing it asked for does not exist in the
file. That is worth as much as a build, because it stops the next person spending days on
it, and it forced 21 onto a route that can actually work.

- [x] 20. ~~Reverse the debug info's symbol record format~~ **Settled: there is none.**
      The appended region is fully accounted for as 488 bytes of padding, one 37,752-byte
      line-number table, and 184 bytes of module records plus the name strings. No table
      maps a name to an address, at any stride, anywhere in the file. Ruled out: names by
      byte offset, names by index, records positioned before the strings, per-module symbol
      counts, and a global scan for any run of 334 plausible offset/segment pairs
- [ ] 21. Recover addresses the other way: **names are in link order, and so is the code.**
      Find function entry points inside each module's known code range by disassembling,
      then match the Nth entry point to the Nth name for that module. Cross-check against
      the line table, which already maps line numbers to offsets, and against known string
      references (file names, prompts) to anchor specific functions
- [ ] 22. Locate the animation interpreter, using `_TASK.ASM`'s known code range
- [ ] 23. Recover the task VM opcode set and operand widths
- [ ] 24. Recover the per-frame sprite-part record. **Verify by compositing and looking**:
      if it does not make a coherent figure it is not decoded, however plausible it looks
- [ ] 25. Write the VM in `henge-core` as a deterministic interpreter, integers only
- [ ] 26. Export every actor's animation scripts to data
- [ ] 27. Replace the hand-authored knight sequences with the recovered ones

## Phase 2: the bestiary

Unblocked all at once by 25. This is the single biggest change to how the game feels.

- [ ] 28. Troll
- [ ] 29. Trogg with axe
- [ ] 30. Trogg with spear
- [ ] 31. Ratmen
- [ ] 32. Mudmen
- [ ] 33. Demon, including its screen border
- [ ] 34. Beast
- [ ] 35. Balok
- [ ] 36. Dragon, as a set-piece encounter with its own state machine
- [ ] 37. Per-creature behaviour. Not recovered; observe or design

## Phase 3: economy and character

Independent of phase 1. **Can start immediately, in parallel with the research.**

- [ ] 38. Gold and prices
- [ ] 39. Inventory: carrying, using, losing
- [ ] 40. Character stats and abilities
- [ ] 41. Potions
- [ ] 42. Magic: spells, casting, costs
- [ ] 43. Curses
- [ ] 44. The hawk and the gem
- [ ] 45. Experience and levelling

## Phase 4: combat depth

37 needs phase 1. The rest can start now.

- [ ] 46. Attack variety, several by direction and button
- [ ] 47. Blocking and parrying
- [ ] 48. Weapon state: drawn, sheathed, dropped, thrown
- [ ] 49. Gore and dismemberment

## Phase 5: the shell

Independent of everything. Makes it feel like a game rather than a demo.

- [ ] 50. Title screen and attract mode
- [ ] 51. Character select, with the four knights' differing stats
- [ ] 52. A real status panel
- [ ] 53. Mouse pointer and clickable widgets
- [ ] 54. The message system: wait, occurrence and instruction messages
- [ ] 55. The intro sequence (`INTR.EXE`, never examined)
- [ ] 56. Save and load. The original has none, so this is ours to design

## Phase 6: the world

Needs 38 for the merchant to open. 60 and 61 are cheap wins available now.

- [ ] 57. Recover the real terrain table and location graph
- [ ] 58. Recover the arena selection tables
- [ ] 59. The moors arena family, which we do not render at all
- [ ] 60. What makes ground impassable
- [ ] 61. Map scrolling
- [ ] 62. Lairs: placement, entry, contents, the guardian fight
- [ ] 63. Moon phases on a weekly cycle, and the between-days screen
- [ ] 64. What the moon gates
- [ ] 65. Tavern: what it offers
- [ ] 66. Temple and mystic services
- [ ] 67. The wizard: abilities, gold and magic bestowal
- [ ] 68. What the stone circle does
- [ ] 69. The dice game. Rules unknown, so design

## Phase 7: the quest

**The point of the game.** Needs lairs (62) and inventory (39). Almost none of this is
recoverable from the symbols, so most of it is design and playtesting rather than porting.

- [ ] 70. The four keys, one per lair
- [ ] 71. The moonstone: where it is, what retrieving it takes
- [ ] 72. Win condition and ending
- [ ] 73. Scoring and the final tally
- [ ] 74. Losing properly, rather than a run simply stopping

## Phase 8: polish

Any time. None of it blocks anything.

- [ ] 75. Palette fades and colour cycling
- [ ] 76. Gamepads, with calibration and debounce; and rebindable controls
- [ ] 77. Music. The tune files are x86 driver blobs with data welded into code, so either
      trace the driver or commission new music

---

## If you only did three things

**25** unblocks nine creatures at once and is the difference between a duelling game and
Moonstone. **38 and 39** open the merchant and the tavern, and give a run a reason to
accumulate anything. **51** is small and makes the whole thing feel like a game.

## What is not portable

Some of the original has no Rust equivalent worth writing: expanded memory paging, disk
swapping, the manual copy protection, DOS file handling. Accounted for in `COMPLETE.md` so
the symbol coverage is checkable, and deliberately absent here.

## What is design rather than translation

Items 37, 56, 64, 69, and all of phase 7. These were never recovered from the executable,
so finishing them means designing and playtesting, not translating. Worth knowing before
anyone estimates the end of this list.
