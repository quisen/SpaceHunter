#!/usr/bin/env sh
# Installs Space Hunter for the current user (no root needed):
#   ~/.local/bin/spacehunter, an app-menu entry and its icon.
# Run again to update. Remove with: ./install.sh --uninstall
set -eu
cd "$(dirname "$0")"
BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$BIN/spacehunter" "$DATA/applications/spacehunter.desktop" "$DATA/icons/hicolor/512x512/apps/spacehunter.png"
  echo "Space Hunter removed."
  exit 0
fi
mkdir -p "$BIN" "$DATA/applications" "$DATA/icons/hicolor/512x512/apps"
install -m 755 spacehunter "$BIN/spacehunter"
install -m 644 spacehunter.png "$DATA/icons/hicolor/512x512/apps/spacehunter.png"
cat > "$DATA/applications/spacehunter.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Space Hunter
GenericName=Disk usage analyzer
Comment=See where your disk space went
Exec=$BIN/spacehunter %f
Icon=spacehunter
Terminal=false
Categories=Utility;System;Filesystem;
DESKTOP
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$DATA/applications" >/dev/null 2>&1 || true
echo "Installed: $BIN/spacehunter (and an app-menu entry)."
case ":$PATH:" in *":$BIN:"*) ;; *) echo "Note: $BIN is not in your PATH; start it from the app menu or with the full path." ;; esac
