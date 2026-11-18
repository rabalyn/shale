#!/bin/sh
# Makes the macOS app bundle of a release build (BUILD-024); runs on any system, so the layout can
# be tested on Linux. Signing and the dmg are done by the `macos` job of the release workflow.
#
#   sh scripts/macos-app.sh <binary> <out-dir> <version> [icon.icns]
#
# Writes <out-dir>/Shale.app: Contents/MacOS/shale, Contents/Info.plist, Contents/Resources/ with
# LICENSE, README.md, CHANGELOG.md and shale.icns. Without an icns file it is made from
# assets/icon/shale-*.png with `iconutil` (macOS only); where there is no iconutil the icon is
# left out with a warning, but on macOS (Darwin) a missing icon is an error.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)

[ $# -ge 3 ] && [ $# -le 4 ] || { echo "usage: macos-app.sh <binary> <out-dir> <version> [icon.icns]" >&2; exit 2; }
binary=$1
out=$2
version=$3
icns=${4:-}
[ -f "$binary" ] || { echo "macos-app.sh: no program at $binary" >&2; exit 1; }
case "$version" in
  *[!0-9A-Za-z.+-]* | "") echo "macos-app.sh: odd version: $version" >&2; exit 2 ;;
esac
# CFBundleVersion takes up to three numbers: a label like dev-1a2b3c4 is not one
bundle_version=$(printf '%s' "$version" | command grep -Eo '^[0-9]+(\.[0-9]+){0,2}' || true)
[ -n "$bundle_version" ] || bundle_version=0.0.0

app="$out/Shale.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/shale"
chmod 755 "$app/Contents/MacOS/shale"
for f in LICENSE README.md CHANGELOG.md; do
  cp "$here/$f" "$app/Contents/Resources/$f"
done

# the icon: the sizes of assets/icon (16 to 512); the 1024 pixel one (512@2x) does not exist, and
# nothing is scaled up, so the iconset has none (iconutil accepts that)
if [ -z "$icns" ]; then
  if command -v iconutil >/dev/null 2>&1; then
    set_dir=$(mktemp -d)/shale.iconset
    mkdir -p "$set_dir"
    for pair in 16:icon_16x16 32:icon_16x16@2x 32:icon_32x32 64:icon_32x32@2x 128:icon_128x128 \
                256:icon_128x128@2x 256:icon_256x256 512:icon_256x256@2x 512:icon_512x512; do
      cp "$here/assets/icon/shale-${pair%%:*}.png" "$set_dir/${pair#*:}.png"
    done
    iconutil -c icns -o "$out/shale.icns" "$set_dir"
    icns="$out/shale.icns"
  elif [ "$(uname -s)" = Darwin ]; then
    echo "macos-app.sh: iconutil is missing on macOS" >&2
    exit 1
  else
    echo "macos-app.sh: warning: no iconutil and no icns file, the app has no icon" >&2
  fi
fi
icon_key=
if [ -n "$icns" ]; then
  [ -s "$icns" ] || { echo "macos-app.sh: empty or missing icon $icns" >&2; exit 1; }
  cp "$icns" "$app/Contents/Resources/shale.icns"
  icon_key='	<key>CFBundleIconFile</key>
	<string>shale</string>
'
fi

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>Shale</string>
	<key>CFBundleDisplayName</key>
	<string>Shale</string>
	<key>CFBundleExecutable</key>
	<string>shale</string>
	<key>CFBundleIdentifier</key>
	<string>io.github.rabalyn.shale</string>
	<key>CFBundleShortVersionString</key>
	<string>$version</string>
	<key>CFBundleVersion</key>
	<string>$bundle_version</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
${icon_key}	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.productivity</string>
</dict>
</plist>
PLIST
echo "made $app"
