#!/usr/bin/env bash
# Regenerates the screenshots of the user guide (docs/userguide/z_media/screenshots/) from the recipes
# in recipes.conf (DEV-007). The recipes and their fixtures live here, the pictures in the guide vault.
#
#   docs/screenshots/regen.sh              all pictures
#   docs/screenshots/regen.sh mail-view    only these (by recipe name)
#   docs/screenshots/regen.sh --list       the recipe names
#
# Environment:
#   SHALE      the program (default: target/release/shale, else target/debug/shale)
#   OUT        where the pictures go (default: docs/userguide/z_media/screenshots)
#   SHALE_FAKE_NOW  the frozen clock of the pictures (default 2026-11-15T10:00:00, UTC): the status
#                   bar, the build stamp, the calendar's today and the ages in lists do not change
#                   between runs, so only real changes show in a diff (a recipe's env may override it)
#   SHALE_REGEN_DISPLAY=host   use the display of the session instead of a private Xvfb
#
# Every picture is made by one `shale --screenshot` run of its own, on a private X display
# (xvfb-run, so the person's desktop, theme and scale factor do not matter and nothing flashes
# on their screen). Needs xvfb-run and a Vulkan or OpenGL driver (Mesa's software ones do).
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/../.." && pwd)
recipes="$here/recipes.conf"
out=${OUT:-$root/docs/userguide/z_media/screenshots}

shale=${SHALE:-}
if [ -z "$shale" ]; then
    for c in "$root/target/release/shale" "$root/target/debug/shale"; do
        [ -x "$c" ] && shale=$c && break
    done
fi

# prints one recipe per line as: name^Asize<TAB>view<TAB>steps<TAB>vault<TAB>env
read_recipes() {
    awk '
        function flush() { if (name != "") print name "\001" size "\001" view "\001" steps "\001" vault "\001" env }
        /^[ \t]*(#|$)/ { next }
        /^\[.*\]$/ { flush(); name = substr($0, 2, length($0) - 2); size = "1360x840"; view = ""; steps = ""; vault = ""; env = ""; next }
        {
            i = index($0, "=")
            k = substr($0, 1, i - 1); v = substr($0, i + 1)
            gsub(/^[ \t]+|[ \t]+$/, "", k); gsub(/^[ \t]+|[ \t]+$/, "", v)
            if (k == "size") size = v
            else if (k == "view") view = v
            else if (k == "steps") steps = v
            else if (k == "vault") vault = v
            else if (k == "env") env = v
        }
        END { flush() }
    ' "$recipes"
}

if [ "${1:-}" = "--list" ]; then
    read_recipes | cut -d$'\001' -f1
    exit 0
fi

[ -n "$shale" ] || { echo "regen: no shale binary; run cargo build --release or set SHALE" >&2; exit 1; }
# an old build may not know a newer option and take its value for something else (BUILD-019, #85)
newer=$(find "$root/src" "$root/Cargo.toml" "$root/Cargo.lock" -type f -newer "$shale" -print -quit 2>/dev/null || true)
[ -z "$newer" ] || { echo "regen: $shale is older than ${newer#"$root"/}; build it again (or set SHALE)" >&2; exit 1; }
if [ "${SHALE_REGEN_DISPLAY:-}" != host ]; then
    command -v xvfb-run >/dev/null || { echo "regen: xvfb-run is missing (or SHALE_REGEN_DISPLAY=host to use this session's display)" >&2; exit 1; }
fi

wanted=("$@")
for w in "${wanted[@]}"; do
    read_recipes | cut -d$'\001' -f1 | grep -qx -- "$w" || { echo "regen: no recipe named $w (see --list)" >&2; exit 1; }
done
failed=()
made=0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

while IFS=$'\001' read -r name size view steps vault env; do
    if [ ${#wanted[@]} -gt 0 ] && [[ ! " ${wanted[*]} " == *" $name "* ]]; then
        continue
    fi
    steps=${steps//\{fixtures\}/$here/fixtures}
    args=(--screenshot "$tmp/$name.png" --size "$size")
    [ -n "$view" ] && args+=(--view "$view")
    [ -n "$steps" ] && args+=(--keys "$steps")
    if [ -n "$vault" ]; then
        # a copy: the recipe may change the vault (an import), the folder in the repo stays as it is
        rm -rf "$tmp/vault-$name"
        mkdir -p "$tmp/vault-$name"
        cp -r "$here/$vault" "$tmp/vault-$name/"
        args+=(--vault "$tmp/vault-$name/$(basename "$vault")")
    fi
    echo "== $name"
    runner=(env TZ=UTC "SHALE_FAKE_NOW=${SHALE_FAKE_NOW:-2026-11-15T10:00:00}")
    [ -n "$env" ] && runner+=($env)
    if [ "${SHALE_REGEN_DISPLAY:-}" = host ]; then
        launch=("${runner[@]}" "$shale" "${args[@]}")
    else
        launch=(env -u WAYLAND_DISPLAY xvfb-run -a -s "-screen 0 1920x1200x24" "${runner[@]}" "$shale" "${args[@]}")
    fi
    if "${launch[@]}" 2>"$tmp/$name.log" && [ -s "$tmp/$name.png" ]; then
        mkdir -p "$out"
        mv "$tmp/$name.png" "$out/$name.png"
        made=$((made + 1))
    else
        grep -E "^(script|--keys|--size|could not|screenshot)" "$tmp/$name.log" >&2 || true
        echo "regen: $name FAILED" >&2
        failed+=("$name")
    fi
done < <(read_recipes)

echo "$made picture(s) made${failed:+, failed: ${failed[*]}}"
[ ${#failed[@]} -eq 0 ]
