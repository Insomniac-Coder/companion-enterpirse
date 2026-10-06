#!/bin/sh
# Assemble each direction (inline the icon set) and screenshot it at 1440x900 with headless Edge.
D="$(cd "$(dirname "$0")" && pwd -W)"
cd "$(dirname "$0")"
for src in ${1:-*.src.html}; do
  out="${src%.src.html}.html"
  python -c "
import sys
icons = open('icons.html', encoding='utf-8').read()
s = open(sys.argv[1], encoding='utf-8').read().replace('<!--ICONS-->', icons)
open(sys.argv[2], 'w', encoding='utf-8').write(s)" "$src" "$out"
  timeout 60 "/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" --headless=new --disable-gpu --hide-scrollbars \
    --window-size=1440,900 --virtual-time-budget=4000 --screenshot="$D/${src%.src.html}.png" "file:///$D/$out" >/dev/null 2>&1
  echo "${src%.src.html}.png"
done
