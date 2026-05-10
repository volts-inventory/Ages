#!/usr/bin/env bash
# run.sh — generate a fresh world and watch it live.
#
# Random seed each launch; 5000 years of sim time at a paced
# tempo so each frame reads as ~half a sim year. NDJSON event
# log archived under `runs/{date}-{seed}.ndjson` so previous
# runs aren't overwritten.
#
# Tweak the constants below if you want a different default
# experience.
set -euo pipefail

YEARS=5000
TICK_RATE_MS=50
FRAME_EVERY_TICKS=6
LOG_LINES=3

# Build (no-op if the binary is already current).
cargo build --release --bin ages

# Optional positional seed arg lets users replay a memorable
# world. `./run.sh 12345` reruns seed 12345; `./run.sh` (no arg)
# generates a fresh random seed. The planet name shown in the
# viewport's `------ {name} ------` divider derives from the seed,
# so the user can pick out a recognisable name from a previous
# run's stdout / `runs/` filename and pass it back.
if [ -n "${1:-}" ]; then
  SEED="$1"
else
  SEED=$RANDOM$RANDOM
fi

mkdir -p runs
OUT="runs/$(date +%Y-%m-%d-%H%M)-${SEED}.ndjson"

echo "seed=${SEED}  years=${YEARS}  out=${OUT}"

# No `--grid-height` override here: keep the binary's full
# default grid even when the terminal is too short to show every
# row at once. If the planet section scrolls off the top on a
# phone-keyboard-up screen, the workaround is to hide the
# keyboard, rotate to landscape, or zoom the terminal font.
exec ./target/release/ages \
  --seed "${SEED}" \
  --years "${YEARS}" \
  --cli viewport \
  --tick-rate-ms "${TICK_RATE_MS}" \
  --frame-every-ticks "${FRAME_EVERY_TICKS}" \
  --viewport-log-lines "${LOG_LINES}" \
  --out "${OUT}"
