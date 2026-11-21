#!/bin/bash
# Double-click me in Finder to remove ClearVox (BlackHole is kept).
cd "$(dirname "$0")"
/bin/bash ./uninstall.sh
echo
read -n 1 -s -r -p "Press any key to close this window…"
