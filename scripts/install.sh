#!/bin/sh
# Installs Shale from the unpacked Linux download (BUILD-025): the program, the desktop file and the
# icons, for the current user or, with --prefix, for everyone. Packed into the tarball as
# shale/install.sh. Run it from anywhere; it finds its files next to itself.
set -eu

usage() {
    cat <<'USAGE'
Usage: install.sh [--prefix DIR] [--uninstall]

Installs Shale from this folder (the unpacked download).

  (no option)    for the current user: the program in ~/.local/bin, the desktop file and the
                 icons in ${XDG_DATA_HOME:-~/.local/share}
  --prefix DIR   for everyone, e.g. --prefix /usr/local: the program in DIR/bin, the desktop
                 file and the icons in DIR/share (run it as root if DIR needs it)
  --uninstall    removes the files an install with the same options puts in place, and
                 nothing else (your vaults and settings stay)
  -h, --help     this text

Running it again upgrades Shale in place.
USAGE
}

prefix=""
uninstall=0
while [ $# -gt 0 ]; do
    case $1 in
        -h | --help)
            usage
            exit 0
            ;;
        --uninstall) uninstall=1 ;;
        --prefix)
            if [ $# -lt 2 ] || [ -z "$2" ]; then
                echo "install.sh: --prefix needs a folder" >&2
                usage >&2
                exit 2
            fi
            prefix=$2
            shift
            ;;
        --prefix=*)
            prefix=${1#--prefix=}
            if [ -z "$prefix" ]; then
                echo "install.sh: --prefix needs a folder" >&2
                usage >&2
                exit 2
            fi
            ;;
        *)
            echo "install.sh: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

src=$(cd "$(dirname "$0")" && pwd)

if [ -n "$prefix" ]; then
    bindir=$prefix/bin
    datadir=$prefix/share
else
    : "${HOME:?HOME is not set}"
    bindir=$HOME/.local/bin
    datadir=${XDG_DATA_HOME:-$HOME/.local/share}
fi
sizes="16 32 48 64 128 256 512 1024"
program=$bindir/shale
desktop=$datadir/applications/shale.desktop
svg=$datadir/icons/hicolor/scalable/apps/shale.svg

refresh_caches() {
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$datadir/applications" >/dev/null 2>&1 || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t "$datadir/icons/hicolor" >/dev/null 2>&1 || true
    fi
}

if [ "$uninstall" = 1 ]; then
    for f in "$program" "$desktop" "$svg"; do
        rm -f "$f"
    done
    for n in $sizes; do
        rm -f "$datadir/icons/hicolor/${n}x${n}/apps/shale.png"
    done
    refresh_caches
    echo "Shale removed from $bindir and $datadir."
    exit 0
fi

if [ ! -f "$src/shale" ]; then
    echo "install.sh: the program 'shale' is not next to this script ($src)" >&2
    exit 1
fi
for f in shale.desktop shale.svg; do
    if [ ! -f "$src/$f" ]; then
        echo "install.sh: $f is missing next to this script ($src)" >&2
        exit 1
    fi
done

# put MODE FILE DEST: copies FILE to DEST with MODE, making the folders of DEST first. No
# `install -D`: BSD install (macOS) has no such option.
put() {
    mkdir -p "$(dirname "$3")"
    install -m "$1" "$2" "$3"
}

put 755 "$src/shale" "$program"
put 644 "$src/shale.svg" "$svg"
for n in $sizes; do
    png=$src/assets/icon/shale-$n.png
    if [ -f "$png" ]; then
        put 644 "$png" "$datadir/icons/hicolor/${n}x${n}/apps/shale.png"
    fi
done

# the desktop file: the same as the shipped one, but Exec is the absolute path, so the menu entry
# works when the program's folder is not on PATH (a path with a space needs quotes there)
case $program in
    *[' ']*) exec_path="\"$program\"" ;;
    *) exec_path=$program ;;
esac
exec_path=$(printf '%s' "$exec_path" | sed 's/%/%%/g')
mkdir -p "$datadir/applications"
tmp=$desktop.tmp.$$
while IFS= read -r line || [ -n "$line" ]; do
    case $line in
        Exec=*) printf 'Exec=%s\n' "$exec_path" ;;
        *) printf '%s\n' "$line" ;;
    esac
done <"$src/shale.desktop" >"$tmp"
chmod 644 "$tmp"
mv -f "$tmp" "$desktop"

refresh_caches
echo "Shale installed: $program"

case ":${PATH:-}:" in
    *":$bindir:"*) ;;
    *)
        echo "Note: $bindir is not on your PATH, so 'shale' does not start from a terminal by name;" >&2
        echo "      the menu entry works anyway (it uses the full path)." >&2
        ;;
esac

if command -v ldd >/dev/null 2>&1; then
    missing=$(ldd "$program" 2>/dev/null | sed -n 's/^[[:space:]]*\([^ ]*\) => not found.*/\1/p' || true)
    if [ -n "$missing" ]; then
        {
            echo "Warning: Shale needs libraries this system does not have:"
            echo "$missing" | sed 's/^/  /'
            # besides glibc the program links only ALSA (sound); the window libraries (Wayland,
            # X11, xkbcommon, OpenGL) are loaded at start, so ldd does not see them
            echo "Install them with your package manager (libasound.so.2 is ALSA):"
            echo "  Arch:          sudo pacman -S alsa-lib"
            echo "  Debian/Ubuntu: sudo apt install libasound2"
            echo "  Fedora:        sudo dnf install alsa-lib"
        } >&2
    fi
fi
