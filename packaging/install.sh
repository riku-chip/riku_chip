#!/usr/bin/env sh
# Installs riku from a release .tar.gz.
#
#   ./install.sh                 # into ~/.local/bin (no sudo)
#   sudo ./install.sh --system   # into /usr/local/bin
#   ./install.sh --uninstall [--system]
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
if [ "${1:-}" = "--system" ] || [ "${2:-}" = "--system" ]; then
    BIN=/usr/local/bin; SHARE=/usr/local/share
else
    BIN="$HOME/.local/bin"; SHARE="${XDG_DATA_HOME:-$HOME/.local/share}"
fi

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN/riku" "$SHARE/applications/riku.desktop" "$SHARE/icons/hicolor/scalable/apps/riku.svg"
    echo "riku uninstalled from $BIN"
    exit 0
fi

mkdir -p "$BIN" "$SHARE/applications" "$SHARE/icons/hicolor/scalable/apps"
install -m 755 "$HERE/riku" "$BIN/riku"
install -m 644 "$HERE/riku.desktop" "$SHARE/applications/riku.desktop"
install -m 644 "$HERE/riku.svg" "$SHARE/icons/hicolor/scalable/apps/riku.svg"

echo "riku installed to $BIN/riku"
case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "Add $BIN to your PATH, for example: echo 'export PATH=\"$BIN:\$PATH\"' >> ~/.bashrc" ;;
esac
"$BIN/riku" --version
