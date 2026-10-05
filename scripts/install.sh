#!/usr/bin/env bash
# Install khz-player from an unpacked release tarball.
#
#   ./install.sh                  install to ~/.local
#   PREFIX=/usr/local sudo -E ./install.sh
#   ./install.sh --uninstall      remove what install.sh put in place
set -euo pipefail

cd "$(dirname "$0")"
prefix="${PREFIX:-$HOME/.local}"
files=(bin/khz-player share/applications/khz-player.desktop)
while IFS= read -r f; do files+=("$f"); done < <(find share/icons -type f | sort)

if [[ "${1:-}" == "--uninstall" ]]; then
    for f in "${files[@]}"; do rm -f "$prefix/$f"; done
    echo "khz-player removed from $prefix"
else
    for f in "${files[@]}"; do
        mode=644
        [[ "$f" == bin/* ]] && mode=755
        install -Dm"$mode" "$f" "$prefix/$f"
    done
    echo "khz-player installed to $prefix"
    case ":$PATH:" in
        *":$prefix/bin:"*) ;;
        *) echo "note: $prefix/bin is not on your PATH" ;;
    esac
fi

command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$prefix/share/icons/hicolor" 2>/dev/null || true
