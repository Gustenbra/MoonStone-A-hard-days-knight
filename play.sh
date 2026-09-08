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

if [ -n "$rebake" ]; then
  # The music has to be lifted out of the tune drivers before the bake can put
  # it in the pack. It needs python and unicorn, and the game plays without it,
  # so a failure here is a note rather than a stop.
  if [ ! -f research/tunes.json ]; then
    if command -v python3 >/dev/null 2>&1; then
      echo "Reading the music..."
      python3 tools/tunes.py "$data" research/tunes.json || \
        echo "  no music this time. It needs unicorn:  pip install unicorn"
    else
      echo "  no python3, so no music. The rest of the game is unaffected."
    fi
  fi
  echo "Reading the original game files..."
  cargo run --release --bin henge-bake -- "$data"
fi

echo "Building..."
cargo build --release

echo
echo "  menus: arrows move, enter or space takes"
echo "  fighting: arrows move, space swings, escape quits"
echo "  tab switches map and arena, [ and ] change arena, R restarts, C the sheet"
echo "  player two: WASD and F"
echo "  gamepads work, and F11 calibrates one"
echo
# shellcheck disable=SC2086
exec target/release/henge $pass
