#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
dist_dir="$repo_root/dist"
app_name="AI Client Installer"
bundle_dir="$dist_dir/$app_name.app"
binary_name="ai-client-installer"

: "${APPLE_SIGN_IDENTITY:?Set APPLE_SIGN_IDENTITY to a Developer ID Application identity}"
: "${APPLE_NOTARY_PROFILE:?Set APPLE_NOTARY_PROFILE to a notarytool keychain profile}"

cd "$repo_root"
rustup target add x86_64-apple-darwin aarch64-apple-darwin
cargo test --test resolver_fixtures
cargo test --test security_boundaries
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin

mkdir -p "$bundle_dir/Contents/MacOS" "$bundle_dir/Contents/Resources"
cp "$repo_root/packaging/macos/Info.plist" "$bundle_dir/Contents/Info.plist"
lipo -create \
  "$repo_root/target/x86_64-apple-darwin/release/$binary_name" \
  "$repo_root/target/aarch64-apple-darwin/release/$binary_name" \
  -output "$bundle_dir/Contents/MacOS/$binary_name"
chmod +x "$bundle_dir/Contents/MacOS/$binary_name"

codesign --force --options runtime --timestamp \
  --sign "$APPLE_SIGN_IDENTITY" "$bundle_dir"
codesign --verify --deep --strict --verbose=2 "$bundle_dir"

dmg_path="$dist_dir/AI-Client-Installer-macos-universal.dmg"
hdiutil create -volname "$app_name" -srcfolder "$bundle_dir" -ov -format UDZO "$dmg_path"
codesign --force --timestamp --sign "$APPLE_SIGN_IDENTITY" "$dmg_path"
xcrun notarytool submit "$dmg_path" --keychain-profile "$APPLE_NOTARY_PROFILE" --wait
xcrun stapler staple "$dmg_path"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg_path"
shasum -a 256 "$dmg_path" > "$dist_dir/SHA256SUMS-macos-universal.txt"

echo "Built and notarized: $dmg_path"
