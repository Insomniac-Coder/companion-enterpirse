#!/bin/sh
# Prints plan.html to ../Companion-Enterprise-Draft-Plan.pdf with headless Edge (Windows, Git Bash).
# The fonts are local files in fonts/, so the print never depends on the network.
# PREVIEW=1 also writes a PNG per page (needs Python with pymupdf) to check the layout.
D="$(cd "$(dirname "$0")" && pwd -W)"
OUT="$D/../Companion-Enterprise-Draft-Plan.pdf"
timeout 120 "/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" --headless=new --disable-gpu \
  --no-pdf-header-footer --virtual-time-budget=20000 \
  --print-to-pdf="$OUT" "file:///$D/plan.html" >/dev/null 2>&1
[ -n "$PREVIEW" ] || exit 0
OUT="$OUT" python -W ignore - <<'PY'
import os, pymupdf
d = pymupdf.open(os.environ["OUT"])
print("pages", d.page_count)
for i, p in enumerate(d):
    p.get_pixmap(dpi=75).save(f"preview-{i + 1}.png")
PY
