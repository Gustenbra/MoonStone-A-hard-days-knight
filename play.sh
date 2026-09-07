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
  echo "Reading the original game files..."
  cargo run --release --bin henge-bake -- "$data"
fi

echo "Building..."
cargo build --release

echo
echo "  arrows move, space swings, escape quits"
echo "  tab switches map and arena, [ and ] change arena, R restarts, C the sheet"
echo "  player two: WASD and F"
echo
# shellcheck disable=SC2086
exec target/release/henge $pass
