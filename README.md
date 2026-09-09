# henge

A reimplementation of *Moonstone: A Hard Days Knight* (Amiga 1991, DOS 1992) in Rust.

This is an **engine**. It ships no artwork, no sound and no game data. To play it you
need your own copy of the original, which the engine reads and converts locally.

> **Status: playable, and still being finished.** Combat, overworld travel, the
> towns and the quest all work. `docs/BUILD_ORDER.md` says what is and is not
> there, item by item, and never claims more than it can show.

**The rule this project is built on: nothing is invented.** Where the original
does something, its routine is found in the executable, disassembled, and
translated into Rust with the address quoted in a comment beside it. Where the
original does nothing, this engine does nothing either. Every value that could
not be recovered is marked as designed rather than ported, in the code and in
`docs/BUILD_ORDER.md`, so the line between the two is always visible.

That rule is not decoration. Every time something was invented to fill a gap, it
looked wrong to a player immediately and passed every test regardless: a hue
substitution that turned the gold knight brown where the original rewrites three
palette entries, an in-fight health bar over an arena the original draws to the
last row, twenty four lairs placed by eye that turned out to sit in a table fifty
one pixels away. Recovering beat guessing every single time.

## What works

- 56 combat arenas across four families, with real terrain, scenery and depth sorting.
  **The ground you may stand on is what an arena's own header says is not tree**: the
  header is a count and that many impassable rectangles, four layouts carry more than
  one, and walking into the tree line takes away that one direction and leaves you the
  other three, the way the original's `CheckBorder` and `SBORD` clear bits in a byte of
  allowed directions rather than clamping anybody to a box
- An overworld you travel across, with a day cycle and ambushes, laid out by the
  original's own two map tables: which of the four kinds of ground each 8x8 block
  is, and how hard that block is to cross. Nothing on the map is impassable; the
  forest and the marsh are half speed and the mountain spine a quarter, and a
  step the ground refuses still costs you the day
- Where a fight happens is decided by the terrain you are standing on, and which
  of that family's eight arenas you get is the family's own turn counter, the way
  the original rotates through them
- **Places to go**: Highwood and Waterdeep at the coordinates the executable
  sends a knight to, a healer in the woods, the stone circle and Math's tower on
  the landmarks the map already draws. Walk onto one and it opens with its own
  screen and menu. Inside the walls are a merchant, a tavern, a healer, a temple
  and a mystic, each on the original's own art and speaking its own lines: three
  dice for a stake of one to five gold on the odds table in `_TAVERN`, a donation
  the healer spends down on your wounds and your life points, a reading from the
  cosmos that can take a point of an ability as easily as give one, and a counter
  that buys magic back at half price
- **Math the wizard**, in his tower in the northern waste. Ring the bell and one
  roll decides between a magic gift, a point of an ability, a purse of gold and
  being turned into a toad, which costs you the three turns the original's own map
  loop refuses a toad. He remembers you: the grudge is seventy after every visit
  and comes down by ten a day, so a second call the same day is dangerous
- **Twenty four lairs**, six on each kind of ground, each fought in its own arena
  layout and each holding gold, magic, or both. One of the four keys is hidden in
  one lair of each family, and a lair you have beaten but could not carry out of
  stays on the map to come back to
- **The quest, and the end of it.** One of the four keys is in one lair of each kind of
  ground, and four of them open the Valley of the Gods, where the Guardian waits on the
  wastes, because its own loader opens with `LoadWasteBack` and its palette is the
  waste's browns with blues where the greens were. Beating
  it spends all four keys and pays one of the four moonstones, and standing in the stone
  circle with the stone whose night it is ends the game. Short of that, five life points:
  a knight put down is whole again and one point poorer, and only the last of them is
  `GAME OVER`. What every step of it says is the original's own words
- **The moon the game is named after.** Four days to a phase, eight steps to the
  cycle, and a between-days screen with tonight's moon over the night sky and one
  of the fourteen things the Gods have to say. The ratmen are stronger on some
  nights than others, because the original's own set-up routine reads the phase
- **Gold and goods**: coin comes off whoever you put down, and the merchants in
  both towns are open. The original's ten magic items are bought, carried and
  used, and the healer inside the walls takes a donation and spends it down. Your
  own home village is worth a life point. What you carry can leave you too, to a
  cutpurse on the road
- **A title screen, and a knight to choose.** The game opens on its own wordmark,
  which turned out to be the last three frames of the bold font's bank, over the
  picture the original draws it on: `_LOADER:MoonPic` is `CH.PIV`, a night sky,
  and the wordmark sits at the corner `DisplaySelect` loads into its registers
  rather than centred. Under it is
  an option list that is the original's down to its words and its coordinates:
  `Players` and `Gore` at x 86 with their values at x 214, `Practice` and
  `Select Knight` centred below them, all six out of the `OPT1a` record chain,
  and the arrow at the four heights `MOON:ARR` gives it. Every word on it is
  drawn in the letters' own
  five colours, because `CH.PIV` reserves those five entries the way `MESSAGE.PIV`
  does and the original's text path has no ink in it at all. Leave it alone and
  it sits there, because that is all `DoOptions` does.
  The four knights are SIR GODBER, SIR RICHARD, SIR JEFFREY and SIR EDWARD,
  blue, gold, emerald and red because the executable's own colour table says so,
  and each begins in his own corner of the map. Those names are `BNAME`,
  `GNAME`, `ENAME` and `RNAME`, and their initials are the colours' rather than
  the names'. Sir Banner, Sir Dwain, Sir Balain and Sir Gunther, which stood
  here before, are `Enemy1Name`..`Enemy4Name`: the computer knights' names, worn
  by whichever of the four seats nobody sits in
- **Choosing one of them, on a black screen, which is where the original puts it.**
  `ChooseKnight` clears the screen to palette entry 0 and blits four portraits on
  it; there is no picture behind them. Its palette is `SelectPAL`, thirty two
  colours out of the executable's own data segment rather than out of any file,
  and the portraits are painted in it: blue, gold, emerald and red. The knight you
  are on wears a hollow frame drawn entirely in palette entry 15, and entry 15 is
  the entry `ChooseKnight` hands to `COLOURGLOW`, so **the chosen knight breathes
  and nothing else on the screen moves**
- **The status screen, which is a pair of stone arches.** `DisplayPillars` clears
  the screen and `StatusSetup` walks two tables of `[cel][x][y][mirror]` records
  that run into each other, and what they draw is fifteen cels of `KI.CEL`: three
  pillars and two ivied arches. `ABorders` is a third such table and it is the row
  labels, `STR :`, `CON :`, `END :`, `XP :`, `GOLD :` and `HIT :`, in the
  original's own five-pixel lettering. Strength, constitution and endurance, life
  points, daggers, gold, experience, health, the sword in your hand and the armour
  on your back go where `DisplayKnight` puts them, in the colours of `STAPAL`, the
  screen's own thirty two, with two entries of it repainted for whichever knight
  you are. The menu goes in the right hand arch, which is the one the original
  leaves empty here. It is a screen of its own, on a key, and it is the only place
  any of it is drawn: the original's fight loop puts no readout over an arena, so
  neither does this, and the ground runs to the foot of the screen
- **Up to four fighters in one arena**, the original's player count, in any mix of
  people at the keyboard and opponents. A knight is the colour the original makes
  him, by the original's own mechanism: `ColourKnight` writes his three shades into
  palette entries 6 to 8 at the start of every bout, a second knight is the same
  figure painted in 9 to 11, and each creature's block goes in from 9, so a forest
  trogg and a wasteland trogg are one sheet in two palettes. Two knights per palette
  is the original's limit, so a brawl of four puts the extras in the second's colours
- Movement, committed attacks, positional hit resolution, damage, death
- **Eight attacks, chosen by the direction held with fire**, the way the
  original's own joystick tables choose them: the swing forward, the chop up,
  the lunge forward and down, the up thrust, the rear thrust, and the block and
  the evade, which are the original's blocking (`CheckBlock`, reproduced as
  written) and stop the blows its table says they stop. Fire with back and up
  throws a dagger, which flies across the arena as a task of its own and comes
  off the sheet. A creature takes each blow on the script its own table gives
  that kind, and dies the way that script says: a trogg stabbed falls, one cut
  at the waist is split
- **Gore, and the switch on the title screen.** Blood where a sword lands on a
  troll, and a knight left kneeling can lose his head to the next swing. Turn
  the gore off and the same blow is a collapse, because the scripts themselves
  branch on the switch
- **The bestiary, and each of them fights like itself.** Troll, trogg with axe,
  hammer or spear, ratmen, mudmen, beast, Balok, demon and dragon, each running
  the original's own animation scripts on its own sprite banks, with the hit
  points, blows and ranges the original's own set-up routines give them.
  **What each does with those ranges is its own routine, and they all read**:
  the trogg picks the overhead or the swing by distance and chops through a
  held block a third of the time, the troll never swings overhead twice
  running, the ratman slashes inside forty and bites to fifty and leaps at
  anything further, the mudman comes at you on a diagonal and goes under the
  ground to come up beside you and takes hold of you with its arms, Balok hops
  in and stands off until you reach for a dagger, and the beast never tracks
  you at all: it charges from one side of the arena to the other, turns off the
  edge and comes back on a different line. Ambushes on the road field them by
  the ground you are standing on
- **The two set pieces.** The demon arrives rather than walking on, breathes
  through its four stances with its whirl at its feet, and slaps, zaps and
  whips you by range; its zap takes you off the board and puts you back beside
  it. The ground it is fought on is the rectangle `SETDEMONBORD` writes over the
  arena's own border list, which is a movement border and not a decoration. The
  dragon is a head on a neck at the far end of the arena with two claws in
  front of it that take no harm at all: it lifts its head when you close and
  lowers it when you back off, bites when you are near and breathes fire the
  length of the arena when you are not
- **The message system, all three kinds.** A message in the original is a chain of
  ten-byte records and there are three routines that show one: the fourteen things the
  Gods say while a disk loads, an occurrence, and an instruction, which comes up in a
  colour of its own. All three go over `MESSAGE.PIV`, the stone circle against a night
  sky, and each fires where the original fires it: the two cities greet you, the stone
  circle tells you the druids are preparing, and the wizard's tower is the one door that
  takes one off the Gods' pile
- **An intro, and it is the original's.** `INTR.EXE` had never been examined. It unpacks
  the same way `MAIN.EXE` does, but the image the unpacker writes is **still packed**: its
  tail is EXEPACK's run-length stream, which is why its addresses looked as though they
  needed corrections. Expand it and everything lands on its own byte, and the whole intro
  is in there. `.STI` turns out to be a tile map: `INTRO.STI` is 48 rows of ten tiles, a
  **320 by 1200 panorama** out of `bg1a`, `bg1c` and `bg1b`, and the intro opens by panning
  a screen-tall window down it from the moon, through the treeline, to the forest floor,
  on the original's own eleven-step speed ramp. Then the plates, in the order the scene
  routines hand them to the blitter, with **the cast animating on the intro's own scripts**:
  the druids' torchlit procession, the circle filling, the arch-druid on his dais. The
  captions have coordinates after all, in ten-byte records like the message system's, and
  the story card goes over `MESSAGE.PIV` where the original puts it. The credits are the
  loading screens, so who did what is read off the table that steps them rather than
  guessed. `MINDSCAP`, the publisher's logo, is a PIV with no extension, which is the only
  reason nothing had baked it
- **Save and load**, which the original has none of, so that part is designed rather than
  ported: a versioned serialization of the simulation with a fingerprint over it, three
  distinguishable refusals, and no path of any kind inside the file
- A deterministic simulation with a state fingerprint, proven by test to agree tick for
  tick across independent runs and across a save/restore
- Sound: swings, blows, deaths and footfalls
- **Music, and it is the original's own.** The tune files are x86 driver blobs with the
  song welded into the code, but three ship for every tune and the Roland one speaks
  plain MIDI, so running the game's own driver under emulation gives the notes back.
  All six, playing where the original's own `LOADMUSIC` calls put them: the dice table,
  the stone circle, the wizard's tower and the mystic, and nowhere else. The voices are
  ours; the notes are not
- **The palette moves.** Fades in and out on every screen change, sixteen steps at one a
  frame, and the original's own colour cycling and colour glows: the overworld's water,
  the frame round the knight you are choosing, and the mudmen
- **Gamepads**, read the way the original reads a stick, with its own dead zone, its own
  two-corner calibration and its own debounce; and controls that are a table of data
  rather than a match on key codes
- 382 tests, all of it verifiable headlessly with no display or sound card

## Running it

You need [Rust](https://rustup.rs) and your own copy of the original game's data
files. On Linux you also need `libudev-dev`, which is what the gamepad library builds
against; without it, build with `--no-default-features` and play on the keys. Put the
folder holding `KN1.OB`, `MAP.CMP` and the rest next to this one, so that both sit side
by side, then:

```
play.bat                 Windows
./play.sh                Linux and macOS
```

That reads the original files once, builds, and starts the game. If your copy of the
original lives somewhere else, say where:

```
play.bat --data "C:\path\to\Moonstone"
./play.sh --data /path/to/Moonstone
```

You never need to ask for a rebake. The pack carries a recipe stamp and a fingerprint of
the unpacked `MAIN.EXE` it was read from, and the baker rebakes when either differs from
what it has now. `--rebake` is still there for forcing one. Anything else you pass goes
straight to the game, so `play.bat --start select` opens on character select.

The bake reads most of the game out of the unpacked `MAIN.EXE`, and the unpacker is
`tools/symbolmap.py`, which needs Python and `pip install unicorn`. The launcher runs it
when the image is missing or was left by an older unpacker, and the baker refuses to
carry on without it: a pack baked around a stale image used to start a game that quietly
had the wrong thing on screen. The music comes out the same way and is the one thing the
launcher lets go of when Python is not there.

The long way, if you would rather drive it yourself:

```sh
python3 tools/tunes.py "path/to/Moonstone" research/tunes.json   # the music, once
cargo run --release -p henge-formats --bin henge-bake -- "path/to/Moonstone" packs/reference
cargo run --release
```

The first line needs `pip install unicorn`, and it is the only step that does. Skip it
and everything works except the music, which the bake will say it could not find.

It opens on the intro, out of `INTR.EXE`: the publisher's logo, the wordmark over the
moon, the credits, the pan down the panorama, and the plates with their cast. Space skips
it, and the title follows. Up and down move
the highlight, left and right change the setting on the row you are on, and space takes
it. The moon quest goes to the select screen, where left and right pick a knight and
space takes him.

**There is a pointer.** `PO.CEL` is the original's arrow and `MovePointer` is how it
moves: two pixels a frame in whichever direction is held on the second player's keys,
or wherever your mouse is. Whatever it is over is the highlighted line, and the button
takes it, on the title, the select screen, a town's menu and the character sheet.

**F5 saves and F9 loads.** The original has no save at all, so that part is ours.

**Gamepads work.** The original read a stick on the gameport and henge reads a modern
pad the same way: the same five-bit word, the same four calibration thresholds, and the
same dead zone, because `AdjustJoy` pulls each threshold an eighth of the measured range
inwards and that is what is implemented. Seat one takes the first pad, seat two the
second, and a pad is OR'd into the keys rather than replacing them, exactly as the
original ORs `JOY1` into its key word. **F11 calibrates**, in the original's own words:
*Move joystick to the top left and press the fire button*, and then the bottom right.

**Controls are rebindable, and they are data.** That part is ours; the original's keys
are five instructions with scancodes in them. The table lives in `henge-controls.json`
beside the save, and it can be edited by hand or from the command line:

```sh
henge --bind 0:fire=Enter --bind 1:up=pad:DPadUp   # rebind, and save the file
henge --controls-write                             # write the defaults out to edit
henge --original-keys                              # start from the original's layout:
                                                   # Enter and the arrows, and Tab W X A D
```

Walking onto a town, a lair, the healer, the stone circle or the wizard's tower
opens it. In a place, up and
down move the highlight, space takes the option, and Tab is always a way back out.

Player one uses the arrows and space, and the direction held with space is the
attack: forward swings, up chops, forward and down lunges, forward and up
thrusts, back thrusts behind, back and down blocks, down evades, back and up
throws a dagger. Player two uses `WASD` and `F`. `1` and `2` set
how many people are playing; the remaining seats are filled by opponents. `C` shows
the character sheet. Tab switches between the overworld and the arena, `[` and `]`
change arena, `,` and `.` change which creature fills the opponents' seats, `R`
restarts the bout, escape quits.

The knight's and the creatures' animations are the original's own scripts, read out of
the unpacked `MAIN.EXE`. The baker looks for `research/main.final.bin` and
`research/symbols.json`, which `tools/symbolmap.py` writes (`docs/REVERSING.md`), and
bakes a knight with no animation and no creatures at all if they are missing, saying so.

The bake step converts your copy of the game into indexed PNGs, WAVs and JSON. **After
it runs, the engine reads only PNG, WAV and JSON.** It has no knowledge that the original
formats exist; those live entirely in `henge-formats`, which is not linked into a release
build.

## Headless

Nothing about this project requires a display, which is what makes it testable in CI.

```sh
henge --screenshot out.png [ticks] [arena] [--walk] [--fight]
henge --trace [ticks] [arena]
```

`--foe <actor>` fills the opponents' seats with one creature, which is how each of the
bestiary is captured and traced on its own:

```sh
henge --screenshot troll.png 40 0 --start arena --foe troll --walk
henge --trace 600 0 --start arena --knight 0 --foe balok
```

Both accept a scripted run, which is how a screen you have to walk to and a menu
you have to drive get reached with no keyboard and no display:

```sh
henge --trace 400 0 --goto 158,102 --input "....d.d.s"   # walk there, then choose
henge --screenshot out.png 6 0 --at healer --hurt 30 --input "..s"
henge --trace 12 0 --start title --input "ldddss"        # two players, quest, choose
henge --screenshot out.png 120 0 --start arena --knight 2 --fight --sheet
```

`--keys <0..4>` starts a run already holding that many of the four lair keys, `--stone
<new|full|half|gibbous>` already carrying a moonstone and `--lives <n>` with that many
life points, which is how the Valley, the win and the game over are reached without
clearing four lairs and dying five times first.

`--point x,y` puts the pointer somewhere, which is how a clickable widget is reached with
no mouse and no display, and `p` in an `--input` script is the pointer's button. `S` and
`L` in a script save and load, the same two calls F5 and F9 make; `--save <path>` says
which file, and `--load` reads it at start.

`--goto x,y` steers across the map a step a tick, and swings back at whatever
ambushes it on the way. `--at <place>` starts inside a place, `--hurt <hp>` starts
the run already wounded so a healer has something to do, `--gold <n>` starts it
with coin so a stall can be reached without first winning the fights that pay for
it, and `--input` feeds one key press per tick: `u`/`d` move the highlight, `s`
takes the option or swings, `hjkl` walk or move the highlight sideways, `.` waits,
and a digit is fire held with a direction, laid out like a numpad (`8` up, `6`
right, `1` down and left), which is how each of the eight attacks is reached.
Presses arrive through the same edge-detected path the keyboard uses, so a
script exercises the game rather than a stub. `--bloodless` is the title's gore
switch, off; `--trace ... --scripts` names the script each fighter is on.

`--start <intro|title|select|map|arena>` says which screen to open on. It defaults to the
map, so every recipe written before the shell existed still does what it did; the
window opens on the title. `--knight <0..3>` begins a run as one of the four
without going through the select screen, and `--sheet` holds the character sheet
open over whatever is drawn.

`--trace` runs the simulation and prints every state change, which is how combat,
travel and what a town does to a run are verified as behaviour rather than by
looking at screenshots:

```
    0  MAP     day 1   at 146,115 hp 100  gold    0 -   won 0  fought 0        on forest
   21  MAP     day 1   at 157,115 hp 100  gold    0 -   won 0  fought 0        on swamp
   30  COMBAT  sw1   swamp    knight Attack  20 @ 56,117 | mudmen Walk    30 @109,111
   50  COMBAT  sw1   swamp    knight Attack  20 @ 69,117 | mudmen Attack  25 @ 99,111
```

The map line carries the position because the ground is a table now: `on swamp` is
`MapType` under the traveller's feet, not a guess at the colour of the picture. The
combat line carries the arena's own name because which of a family's eight you get
is a rotation, and watching it go round is how that is checked; and each fighter's
actor, because what waits in a swamp is a mudman and that is worth seeing too.

```
    0  PLACE   day 1  hp  30  gold  100  -         The Healer  > Tend my wounds
    2  PLACE   day 4  hp 100  gold  100  -         The Healer  > Tend my wounds  Rest well...
```

```
    0  PLACE   day 1  hp  40  gold  100  -         Merchant  > Potion of healing [20]
    1  PLACE   day 1  hp  40  gold   80  potion    Merchant  > Potion of healing [20]  A fair trade.
    6  PLACE   day 1  hp  80  gold   60  potion    Merchant  > Drink a potion          You drain it.
```

## How it is put together

| Crate | Purpose |
|---|---|
| `henge-core` | The simulation: combat, animation, arenas, overworld. One dependency, serde. No rendering, no I/O, no platform. |
| `henge-audio` | What to play, and where it comes out, split apart so only the second half is platform specific. |
| `henge-assets` | Logical asset ids resolved through a stack of packs. |
| `henge-formats` | Reads the original's files. Research and baking only; never linked into a release. |
| `henge-desktop` | Window, input, palette framebuffer. Produces the executable. |

`henge-core` being austere is deliberate. It compiles for a server, a browser and a
headless test unchanged, and it is the precondition for networked play. See
`docs/ROADMAP.md`.

### Assets are addressed by name, never by path

`actor.knight`, `sfx.swish`, `palette.forest`. Ids resolve through a stack of packs,
first match wins:

```
packs/original/     original artwork, safe to distribute   <- searched first
packs/reference/    baked from your copy of the game       <- gitignored, never distributed
```

Replacing a sprite is dropping a PNG into `packs/original` and adding one manifest
entry. No code change, no rebuild, and the game stays playable throughout.

Every pack declares its provenance, so the project can always answer whether a build
contains anything derived:

```sh
cargo run --release -p henge-assets --bin henge-pack-status packs
```

## Text

The game can say things now. The fonts decoded from the start, and which glyph draws which
character was read off the artwork: A-Z, then a-z, then 0-9, then punctuation. **That table
has since been found in the executable, and the reading was right.** `GFX:TextASCII` is 95
bytes indexed by `character - 32`, and it agrees entry for entry; the only correction is the
bold font's last glyph, read as a bar and actually a slash, and glyph 63 is the one entry
no character maps to, so the `?` there is still a reading of the artwork. The spacing came
with it: the bold face tracks three pixels tight, because `TextP` takes three off every
glyph's advance when the current font is the bold one, and the small face does not. A few
ornaments at the end are unidentified and left unmapped rather than guessed at; an unmapped
glyph is simply never drawn.

The mapping lives in the pack as content, so a replacement font needs no code change.

**A glyph is a sprite and it is drawn in its own colours.** `TextP` looks the glyph up,
puts its width and height in the blitter's registers and calls the same routine every
other cel goes through; there is no ink anywhere in the original's text path. A bold
glyph carries five indices, 5 the ring round the letter and 9 to 12 the face inside it,
and three of the game's thirty seven pictures keep those five entries out of their own
painting and hold the face's ramp there instead: `MESSAGE.PIV`, `CH.PIV` and `bg8.piv`.
Those three are exactly the plates the game writes on, which is a check on both. Text
over one of them is blitted, not flattened; over an arena, whose palette owes the font
nothing, henge still draws a silhouette in a chosen colour, and that part is ours.

```sh
henge --screenshot out.png 5 0 --say "Day 1|forest|100 of 100"
```

`--say` draws a line over whatever was rendered, which is how the font and its mapping are
checked without hunting for a frame that happens to show the status bar.

`--peaceful` suppresses ambushes. Without it, checking how the map draws anywhere but the
starting corner is impossible: the traveller is killed en route long before arriving, so
half the map could never be looked at.

`--palette` prints the composed palette after a capture, which is the only way to watch a
cycle or a glow on an entry the picture hardly uses. `--fade` lets a capture show the fade
it is in the middle of; without it a capture runs the fade out first, so that every recipe
written before fades existed still shows its screen rather than a black rectangle.

```sh
henge --screenshot out.png 24 0 --start map --peaceful --palette   # watch the river move
henge --screenshot out.png 8 0 --start map --fade                  # halfway through a fade in
```

## Sound

`henge-audio` keeps two things separate. Deciding **what** should be heard is done by
watching the fight, and is pure logic with no platform behind it, so it is tested without
a sound card. Deciding **where it comes out** sits behind a trait, so a browser build or a
headless server replaces only that half.

Cues are derived rather than scripted: a swing is announced when it starts, so the sound
leads the blow; footfalls land on chosen frames of the walk cycle rather than every frame;
and hits come from the simulation itself, so a blow is never unheard and a miss is never
announced.

Building without a sound stack at all is supported and checked in CI:

```sh
cargo build --workspace --no-default-features
```

**No audio device is a normal state, not a failure.** Containers, CI and plenty of
machines have none. The game says so once and plays silently.

## Music

**There is music, and it is the original's.** The tune files looked like a dead end: each
`xTUNEn.BIN` is a relocatable x86 driver with the song welded into the code, and there is
no format in there to parse. But three of them ship for every tune, one per sound card,
and **the Roland one talks plain MIDI to an MPU-401**. So `tools/tunes.py` runs the game's
own driver under the same 8086 emulator that unpacked the executable, answers the sound
card's status port, and writes down what the driver sends. All six tunes come out with
their notes, channels, velocities and lengths intact, on the driver's own tick, which is
the game's timer at 54.62 Hz. Four of them loop, and the loop point falls out of comparing
what is sounding tick for tick.

Where each one plays is recovered as well. Five places in the game call `LOADMUSIC`, and
nothing else in it has music at all: the tavern's dice table, the stone circle, Math's
tower and the mystic's counter. The map is silent, the arenas are silent and so is the
title, because that is how the original is.

**The notes are the original's; the sound is ours.** A MIDI stream names a Roland's
instrument numbers and nothing else, and how those actually sounded belonged to a
synthesiser this project does not have and will not pretend to. So the tunes are played
on a small wavetable synthesiser of ours, with voices chosen by instrument family. It is
labelled that way everywhere rather than passed off as a recording.

```sh
python3 tools/tunes.py "path/to/Moonstone" research/tunes.json  # lift the notes
cargo run --release -p henge-formats --bin henge-bake -- --render-music research/music
```

The second line writes each tune out as a WAV, which is how they were checked.

## Colour

The palette moves, and all of it is recovered. The original keeps a live palette of 32
twelve-bit colours and queues one routine on the frame list, `COLCON`, which walks six
colour-cycle slots and six colour-glow slots. A **cycle** rotates a span of entries by one
every so many frames; a **glow** walks one entry one step a channel towards a target
colour and swaps back on arrival, so it breathes. The game installs exactly two from a
screen, and both are built: the overworld's water, which is entries 21 to 23 rotating
every twelfth frame under a symbol the original itself calls `RiverHANDLE`, and the
character select screen's entry 15 breathing towards a teal, which on that screen is the
hollow frame round the chosen knight and nothing else. A third belongs to the mudmen and
arrives with them.

Fades are the other half: sixteen steps, one a frame, linear, in and out. Every screen
change fades in, and the two screens that end on their own fade out.

None of it touches the framebuffer. It is arithmetic on the palette on its way to the
screen, which is the whole reason the framebuffer is indexed.

## Combat

Positional, not box-on-box. Every attack frame carries a **hit line**, a polyline swept
in the actor's own space, and a swing connects when that line crosses the target's body
*and* their depths line up. That is the shape the original stores for its creatures.

- An attack **commits**: no turning, no second swing, no walking out of it.
- A swing connects **once**, however many of its frames carry a line, but a swing that
  misses keeps offering its line for the rest of the animation.
- Frames carry the fighter through their own `dx`, so a lunge that travels is a property
  of the swing rather than something bolted on beside it.

All of it is data. Retuning the feel of the game is editing JSON, not editing Rust.

## Gold, goods and a pack

A run carries a purse and a pack, both of them ordinary simulation state that
serializes with everything else. Coin comes off the fallen: what a fighter is
worth is a `bounty` on its actor definition, so a troll can be worth more than a
rat without a line of Rust changing. A purse is picked up after a fight, which
means only a winner still on their feet collects one.

Items are data like everything else. Each one names a price and a virtue, and a
menu line takes its price from the goods rather than repeating it in the label,
so the two can never drift apart:

```json
"potion": { "name": "Potion of healing", "price": 20, "consumed": true,
            "virtue": { "does": "restore" } }
```

**Things leave the pack as well as entering it.** The original names a routine
`TAKEFROMKNIGHT`, so losing is a real operation rather than an afterthought on a
list that only ever grows: a potion drunk is a potion gone, and a cutpurse on the
road takes a share of the purse or, failing that, something out of the pack. Who
gets robbed is decided by a seeded roll carried in the run, so two machines
walking the same road are robbed on the same step.

A stall is its own place, marked `hidden` so walking can never stumble into it,
reached through the town's own menu and leaving back into it. That keeps a
town's front door short and gives the shop a box wide enough for goods and their
prices.

A bout takes **one `Intent` per fighter and cannot tell where they came from**. A
keyboard, an opponent and a network packet are interchangeable, which is the seam
networked play plugs into without touching combat. See `docs/ROADMAP.md`.

## Documentation

- [`docs/FORMATS.md`](docs/FORMATS.md): the original's file formats, fully documented
- [`docs/REVERSING.md`](docs/REVERSING.md): unpacking the executable, and what is still unknown
- [`docs/BUILD_ORDER.md`](docs/BUILD_ORDER.md): every item once, in the order to do it
- [`docs/COMPLETE.md`](docs/COMPLETE.md): everything left to build, against the original's own function names
- [`docs/ROADMAP.md`](docs/ROADMAP.md): what is next, including the multiplayer architecture
- [`CONTRIBUTING.md`](CONTRIBUTING.md): including the rules that keep the simulation networkable

## Licence

[MIT](LICENSE), for the source in this repository and nothing else.

`Moonstone: A Hard Days Knight` belongs to its rights holders. None of it is here:
no artwork, no audio, no level data, no code. What is here is an independent
reimplementation written from measurements of the file formats and from the
published game's own behaviour. Playing it needs a copy of the original that you
already own; the engine reads that copy on your machine and converts it there,
and `.gitignore` is written so nothing derived from it can be committed by
accident.

No code was taken from any other reimplementation. OpenMoonstone is AGPL-3.0 and
was deliberately never read. Every decoder here was written from scratch against
the raw data.
