# henge

A modern, playable recreation of *Moonstone: A Hard Days Knight* (originally released for Amiga in 1991, DOS in 1992), rebuilt from scratch in Rust.

> **Status:** Playable and actively in development. Combat, traveling the overworld, towns, and the main quest all work today.

**Why it's accurate:** Instead of guessing how the original game behaved, this project reads the original game's code directly and rebuilds each part to match it exactly. Nothing is invented, if the original did something a certain way, henge does it the same way.

---

## Features

- **Combat & Arenas** – 56 different arenas across four terrain types, with up to 4 fighters (you and/or AI) battling it out using 8 directional attacks, blocks, evades, and toggleable blood/gore.
- **Overworld & World Map** – Travel across the map exactly like the original: distance, time of day, and the moon's phase all behave the same way they did in 1991. There are no random ambushes, every fight in the game is one you walk into on purpose.
- **Locations & NPCs** – Visit Highwood, Waterdeep, healers, a stone circle, Math the Wizard, taverns with dice gambling, mystics, and merchants.
- **Bestiary** – The full original enemy roster (Troll, Troggs, Ratmen, Mudmen, Beast, Balok, Demon, Dragon), each behaving exactly like their original AI.
- **Music & Sound** – Original music and sound effects, extracted directly from the DOS version and played through a modern synth.
- **Modern Enhancements** – Rebindable controls (keyboard & gamepad), a headless mode for automation/testing, and cross-platform save/load.

---

## Getting Started

### What you'll need

- [Rust](https://rustup.rs) installed
- Python 3, with the `unicorn` package (`pip install unicorn`) – used to extract assets from the original game files
- A copy of the original *Moonstone* game files (DOS version)
- Linux only: the `libudev-dev` package (needed for gamepad support)

### Running the game

Place your original Moonstone game files next to this repository, or point directly to where they are:

```sh
# Linux / macOS
./play.sh --data /path/to/Moonstone

# Windows
play.bat --data "C:\path\to\Moonstone"
```
