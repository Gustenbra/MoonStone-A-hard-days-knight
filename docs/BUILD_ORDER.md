# Build order

Every item, once each, in the order you would actually do it. One flat list.

`COMPLETE.md` is the same work organised by subsystem, with the original's function names
against each part. This file is the checklist.

**77 items. 25 done, 52 remaining.**

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

Item 20 was recorded as settled and negative. **It was wrong**, and 21 is what proved it:
the search had been run against an image that was still packed, because `MAIN.EXE` is
packed twice and the unpacker peeled only the outer layer. Peel both and the symbol table
is plainly there. The correction is written up in `REVERSING.md`; it is left in the record
rather than quietly deleted, because the failure mode is worth passing on.

That unblocked 22 almost for free, and most of the phase with it.

- [x] 20. ~~Settled: there is no symbol address table~~ **Wrong; see 21.** The search was
      sound but it was run against the EXEPACK'd image, where the table sits inside
      RLE-compressed bytes and reads as loose name strings with junk between them
- [x] 21. **Recovered: 2,223 symbols with addresses.** `MAIN.EXE` is PKLITE outside and
      Microsoft EXEPACK inside; `tools/symbolmap.py` peels both and parses the eleven
      per-module blocks of the TASM symbol table. Code addresses take a seven-step monotone
      correction, derived and applied by the tool. 1,778 of the 2,223 corroborate
      independently: code symbols by landing on called branch targets, data symbols by
      being referenced from code, and several by content, with `CelFile1` pointing at
      `KN1.OB` and `map` at `MAP.CMP`
- [x] 22. **Located: `PerformCOMMAND` at 0x97fb**, `PerformLOOP` at 0x97f2. Confirmed by
      disassembly as a bytecode fetch-and-dispatch loop, not by name alone. The command
      table it indexes is `TaskComTable`, and the code's `mov bx, 0x9448` matches that
      symbol's address exactly
- [ ] 23. Recover the task VM opcode set and operand widths, by reading out
      `TaskComTable` and disassembling each handler it points at
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

- [x] 38. Gold and prices. Coin off the fallen, a `bounty` per actor in the data,
      prices on the items, and both towns' merchants open. The healers inside the
      walls charge coin as well as days; the hermit in the woods still charges only days
- [x] 39. Inventory: carrying, using, losing. A bounded pack on the run, items as
      data with a price and a virtue, flasks bought and drunk, and losing made real
      both ways: a flask is spent when drunk, and a cutpurse on the road takes coin
      or, failing that, something out of the pack
- [ ] 40. Character stats and abilities
- [ ] 41. Potions beyond healing. Healing flasks landed with 39, as an item virtue
      in the data; anything a potion does other than mend waits on 42
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

38 is done, so the merchant is open and the tavern has something to charge for.
60 and 61 are cheap wins available now.

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
Moonstone. **51** is small and makes the whole thing feel like a game. **65** is now
cheap: the tavern was shut for want of anything to charge, and 38 fixed that.

## What is not portable

Some of the original has no Rust equivalent worth writing: expanded memory paging, disk
swapping, the manual copy protection, DOS file handling. Accounted for in `COMPLETE.md` so
the symbol coverage is checkable, and deliberately absent here.

## What is design rather than translation

Items 37, 56, 64, 69, and all of phase 7. These were never recovered from the executable,
so finishing them means designing and playtesting, not translating. Worth knowing before
anyone estimates the end of this list.
