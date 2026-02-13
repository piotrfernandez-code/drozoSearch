#!/bin/bash
set -e

APP_NAME="drozoSearch"
BUNDLE_DIR="target/${APP_NAME}.app"
CONTENTS_DIR="${BUNDLE_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

echo "Building release binary..."
cargo build --release

echo "Creating app bundle..."
rm -rf "${BUNDLE_DIR}"
mkdir -p "${MACOS_DIR}"
mkdir -p "${RESOURCES_DIR}"

# Copy binary
cp target/release/drozosearch "${MACOS_DIR}/drozosearch"

# Copy icon
cp assets/icon.icns "${RESOURCES_DIR}/icon.icns"

# Create Info.plist
cat > "${CONTENTS_DIR}/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>drozoSearch</string>
    <key>CFBundleDisplayName</key>
    <string>drozoSearch</string>
    <key>CFBundleIdentifier</key>
    <string>com.drozosearch.app</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleExecutable</key>
    <string>drozosearch</string>
    <key>CFBundleIconFile</key>
    <string>icon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.utilities</string>
</dict>
</plist>
PLIST

echo ""
echo "Done! App bundle created at: ${BUNDLE_DIR}"
echo ""
echo "To install, drag drozoSearch.app to /Applications:"
echo "  cp -r ${BUNDLE_DIR} /Applications/"
echo ""
echo "Or open it directly:"
echo "  open ${BUNDLE_DIR}"
