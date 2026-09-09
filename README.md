# henge

A modern engine reimplementation of *Moonstone: A Hard Days Knight* (Amiga 1991, DOS 1992) written in Rust.

> **Status:** Playable and actively in development. Combat, overworld travel, towns, and the main quest are functional.

**Design Philosophy:** Strict accuracy over guessing. Where the original executable defines a routine, it is disassembled and ported directly to Rust with memory addresses cited in comments. Nothing is invented.

---

## Features

* **Combat & Arenas:** 56 arenas across four terrain types with accurate depth sorting, terrain boundaries, up to 4 local/AI fighters, 8 directional attack types, blocks, evades, and blood/gore toggles.
* **Overworld & World Map:** Travel as the original measures it: a day is the knight's stride times sixteen held frames (`_MAP:DistanceDONE`, `GoTheDistance`), slow ground and the map's edge both cost steps (`CheckSLOW`, `MapMovement`), every encounter spends the rest of the day (`EncounterAllDone`), the moon moves every fourth day, and the 24 lairs hold the keys and the loot. There are no random ambushes, because the original has none: every fight is something you stand on and enter.
* **Locations & NPCs:** Highwood, Waterdeep, healers, stone circle, Math the Wizard, taverns (dice gambling), mystics, and merchants.
* **Bestiary:** Full enemy roster (Troll, Troggs, Ratmen, Mudmen, Beast, Balok, Demon, Dragon) matching original AI scripts, attack ranges, and setup attributes.
* **Text & Messages:** Every message chain is the executable's own ten-byte records (`{text, x, y, flags, next}`, walked as `GFX:TextPTop` at `0x7a8a` walks them), glyphs blit in their own indices exactly as `GFX:TextP` at `0x7aee` does, `INSTRUCTMESSAGE`'s red ramp is the six palette words at `0x8f3b`, and a box waits for fire (`WaitFIRE`, `0x8251`) or covers a load, whichever its caller does.
* **Music & Sound:** MIDI notes extracted directly from the original DOS Roland drivers, played through a built-in wavetable synth alongside original SFX.
* **Modern Enhancements:** Rebindable controls (keyboard & gamepads), headless CLI runner, deterministic state serialization, and cross-platform save/load.

---

## Quickstart

### Prerequisites

* [Rust](https://rustup.rs)
* Python 3 (`pip install unicorn` required for binary asset extraction)
* Original game data files from *Moonstone* (DOS)
* Linux only: `libudev-dev` (for gamepad support)

### Running the Game

Place your original game files adjacent to the repository, or pass the path directly:

```sh
# Linux / macOS
./play.sh --data /path/to/Moonstone

# Windows
play.bat --data "C:\path\to\Moonstone"
