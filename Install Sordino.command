#!/bin/bash
# Double-click me in Finder to install ClearVox.
cd "$(dirname "$0")"
/bin/bash ./install.sh
echo
read -n 1 -s -r -p "Press any key to close this window…"
