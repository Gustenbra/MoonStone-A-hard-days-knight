#!/bin/sh
# Build and run Moonstone in Rust.
#
#   ./play.sh                   build if needed, then play
#   ./play.sh --rebake          re-read the original game files first
#   ./play.sh --data /path/to/Moonstone
#
# Anything else is handed straight to the game, so the debug flags in README.md
# work here too:  ./play.sh --start select
set -e
cd "$(dirname "$0")"

data="$(pwd)/../Moonstone"
rebake=""
pass=""

while [ $# -gt 0 ]; do
  case "$1" in
    --rebake) rebake=1; shift ;;
    --data)   data="$2"; shift 2 ;;
    *)        pass="$pass $1"; shift ;;
  esac
done

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust is not installed, or cargo is not on your PATH."
  echo "Install it from https://rustup.rs and open a new terminal."
  exit 1
fi

if [ ! -f "$data/KN1.OB" ]; then
  echo "Could not find the original Moonstone files."
  echo "Looked in: $data"
  echo "That folder needs to hold KN1.OB, MAP.CMP and the rest."
  echo "Point at the right one with:  ./play.sh --data /path/to/Moonstone"
  exit 1
fi

[ -f packs/reference/manifest.json ] || rebake=1

# The baker is asked every time. It compares the pack's recipe stamp against
# its own and does nothing when they match, which costs a moment; when they do
# not, it rebakes without anyone having to remember a flag. Getting this wrong
# is silent: the game starts, and quietly has no music, no animation scripts
# and no overworld grid.
if true; then
  # The music has to be lifted out of the tune drivers before the bake can put
  # it in the pack. It needs python and unicorn, and the game plays without it,
  # so a failure here is a note rather than a stop.
  if [ ! -s research/tunes.json ]; then
    if command -v python3 >/dev/null 2>&1; then
      echo "Reading the music..."
      python3 tools/tunes.py "$data" research/tunes.json || \
        echo "  no music this time. It needs unicorn:  pip install unicorn"
    else
      echo "  no python3, so no music. The rest of the game is unaffected."
    fi
  fi
  # The baker refuses a missing or stale MAIN.EXE image with exit code 3,
  # and that is the one failure this script can fix itself: the unpacker is
  # tools/symbolmap.py, which needs the same python and unicorn the music
  # does. Run it and ask the baker once more. Any other failure stops here,
  # loudly, rather than starting a game with the wrong thing on screen.
  status=0
  cargo run --release --quiet --bin henge-bake -- "$data" ${rebake:+--force} || status=$?
  if [ "$status" = 3 ]; then
    if command -v python3 >/dev/null 2>&1; then
      echo "Unpacking MAIN.EXE..."
      python3 tools/symbolmap.py "$data/MAIN.EXE" research/symbols.json \
        --image research/main.final.bin || exit 1
      cargo run --release --quiet --bin henge-bake -- "$data" ${rebake:+--force} || exit 1
    else
      echo "  that needs python3 and unicorn (pip install unicorn)."
      exit 1
    fi
  elif [ "$status" != 0 ]; then
    exit 1
  fi
fi

echo "Building..."
cargo build --release

echo
echo "  the original's keys: player one enter and the arrows, player two tab W X A D"
echo "  menus: the direction keys move, fire takes"
echo "  fighting: arrows move, enter swings, escape on the title quits"
echo "  F2 switches map and arena, [ and ] change arena, R restarts, C the sheet"
echo "  gamepads work, and J on the title screen calibrates one"
echo
# shellcheck disable=SC2086
[ -n "$HENGE_NO_LAUNCH" ] && exit 0
exec target/release/henge $pass
