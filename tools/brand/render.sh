#!/usr/bin/env bash
# render.sh <svg> <out.png> <intrinsic_size> <scale> — rasterize an SVG via headless
# Chrome with an isolated profile (avoids lock conflicts with a running Chrome).
# Output px = intrinsic_size * scale. @dep Chrome stable.
set -e
SVG="$1"; OUT="$2"; SZ="${3:-512}"; SC="${4:-2}"
CHROME="/c/Program Files/Google/Chrome/Application/chrome.exe"
PROF="/c/Users/YOUR_USER/AppData/Local/Temp/cr-$$-$RANDOM"
"$CHROME" --headless --disable-gpu --no-sandbox --user-data-dir="$PROF" \
  --force-device-scale-factor="$SC" \
  --default-background-color=00000000 --window-size="$SZ,$SZ" \
  --screenshot="$OUT" "$SVG" 2>/dev/null || true
# leave PROF for OS temp cleanup (Chrome holds locks briefly after exit)
[ -s "$OUT" ] && echo "OK $OUT ($(wc -c < "$OUT") bytes)" || echo "FAIL $OUT"
