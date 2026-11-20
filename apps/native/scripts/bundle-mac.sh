#!/usr/bin/env bash
# Builds a release binary and packages it as dist/Prototypie.app (ad-hoc signed; no notarisation, no install).
# Modelled on Rune's scripts/mac-app (bundle layout, Info.plist keys, codesign), minus its ~/Applications install.
#   scripts/bundle-mac.sh            build + bundle
#   scripts/bundle-mac.sh --no-build bundle the existing target/release/prototypie
#   scripts/bundle-mac.sh --lean     build without agent tooling (--snap/--tree/--script fail; ~3 MB smaller)
# MAC_APP_SIGN=<identity> signs with that identity instead of ad hoc ("-").
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NAME="Prototypie"
BUNDLE_ID="dev.prototypie.app"
EXE="prototypie"
CATEGORY="public.app-category.developer-tools"
DIST="$ROOT/dist"
APP="$DIST/$NAME.app"
ICON_SRC="${PROTOTYPIE_ICON:-$ROOT/assets/icon.png}"
FONTS="$ROOT/crates/proto-app/assets/fonts"
export PATH="$HOME/.cargo/bin:$PATH"

build=1
features="tooling"
for arg in "$@"; do
  case "$arg" in
    --no-build) build=0 ;;
    --lean) features="" ;;
    -h|--help) sed -n 2,7p "$0"; exit 0 ;;
    *) echo "bundle-mac: unknown argument $arg" >&2; exit 2 ;;
  esac
done

if [[ $build == 1 ]]; then
  (cd "$ROOT" && cargo build --release -p proto-app --bin "$EXE" ${features:+--features "$features"})
fi
BIN="$ROOT/target/release/$EXE"
[[ -x "$BIN" ]] || { echo "bundle-mac: no binary at $BIN" >&2; exit 1; }

VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"  # [workspace.package]
VERSION="${VERSION:-0.0.1}"

# Stage beside the destination, then swap in, so a failed run never leaves half a bundle.
mkdir -p "$DIST"
STAGE="$(mktemp -d "$DIST/.$NAME.app.stage-XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
CONTENTS="$STAGE/$NAME.app/Contents"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources/fonts"
cp "$BIN" "$CONTENTS/MacOS/$EXE"
# The UI font is compiled in (include_bytes!); it ships as a resource too, with its licence.
cp "$FONTS"/* "$CONTENTS/Resources/fonts/"

plist_icon=""
if [[ -f "$ICON_SRC" ]]; then
  # The reference's favicon is the only logo it has; scale it into an .iconset (upscaled sizes are soft).
  SET="$STAGE/icon.iconset"
  mkdir -p "$SET"
  BASE="$STAGE/icon-base.png"
  sips -s format png "$ICON_SRC" --out "$BASE" >/dev/null
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$BASE" --out "$SET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$BASE" --out "$SET/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$SET" -o "$CONTENTS/Resources/icon.icns"
  plist_icon="<key>CFBundleIconFile</key><string>icon</string>"
else
  echo "bundle-mac: no icon at $ICON_SRC; bundling without one" >&2
fi

cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>$EXE</string>
  $plist_icon
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSApplicationCategoryType</key><string>$CATEGORY</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSHumanReadableCopyright</key><string>Google Sans Flex: SIL Open Font License (Resources/fonts/OFL.txt)</string>
</dict>
</plist>
PLIST
plutil -lint "$CONTENTS/Info.plist" >/dev/null

IDENTITY="${MAC_APP_SIGN:--}"
if ! codesign --force --sign "$IDENTITY" "$STAGE/$NAME.app" 2>/dev/null; then
  [[ "$IDENTITY" == "-" ]] && { echo "bundle-mac: ad hoc signing failed" >&2; exit 1; }
  echo "bundle-mac: signing with $IDENTITY failed; signing ad hoc" >&2
  codesign --force --sign - "$STAGE/$NAME.app"
fi
codesign --verify --strict "$STAGE/$NAME.app"

rm -rf "$APP"
mv "$STAGE/$NAME.app" "$APP"
signed="$IDENTITY"; [[ "$IDENTITY" == "-" ]] && signed="ad hoc"
echo "bundle-mac: $APP ($(du -sh "$APP" | cut -f1 | tr -d ' '), version $VERSION, signed $signed)"
