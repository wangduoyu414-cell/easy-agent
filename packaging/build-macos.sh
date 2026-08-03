#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
dist_dir="$repo_root/dist"
app_name="AI Client Installer"
bundle_dir="$dist_dir/$app_name.app"
binary_name="ai-client-installer"
dmg_path="$dist_dir/AI-Client-Installer-macos-universal.dmg"
unsigned_build="${ALLOW_UNSIGNED_MACOS_BUILD:-0}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This release script must run on macOS." >&2
  exit 1
fi

if [[ "$unsigned_build" != "1" ]]; then
  : "${APPLE_SIGN_IDENTITY:?Set APPLE_SIGN_IDENTITY to a Developer ID Application identity}"
  : "${APPLE_NOTARY_PROFILE:?Set APPLE_NOTARY_PROFILE to a notarytool keychain profile}"
fi

cd "$repo_root"
version="$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)"
if [[ -z "$version" ]]; then
  echo "Could not read the Cargo package version." >&2
  exit 1
fi
rustup target add x86_64-apple-darwin aarch64-apple-darwin
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin

rm -rf "$bundle_dir"
rm -f "$dmg_path"
mkdir -p "$bundle_dir/Contents/MacOS" "$bundle_dir/Contents/Resources"
cp "$repo_root/packaging/macos/Info.plist" "$bundle_dir/Contents/Info.plist"
/usr/bin/plutil -replace CFBundleShortVersionString -string "$version" "$bundle_dir/Contents/Info.plist"
/usr/bin/plutil -replace CFBundleVersion -string "$version" "$bundle_dir/Contents/Info.plist"
lipo -create \
  "$repo_root/target/x86_64-apple-darwin/release/$binary_name" \
  "$repo_root/target/aarch64-apple-darwin/release/$binary_name" \
  -output "$bundle_dir/Contents/MacOS/$binary_name"
chmod +x "$bundle_dir/Contents/MacOS/$binary_name"
lipo "$bundle_dir/Contents/MacOS/$binary_name" -verify_arch x86_64 arm64

if [[ "$unsigned_build" == "1" ]]; then
  codesign --force --deep --options runtime --sign - "$bundle_dir"
else
  codesign --force --options runtime --timestamp \
    --sign "$APPLE_SIGN_IDENTITY" "$bundle_dir"
fi
codesign --verify --deep --strict --verbose=2 "$bundle_dir"

hdiutil create -volname "$app_name" -srcfolder "$bundle_dir" -ov -format UDZO "$dmg_path"
if [[ "$unsigned_build" == "1" ]]; then
  codesign --force --sign - "$dmg_path"
else
  codesign --force --timestamp --sign "$APPLE_SIGN_IDENTITY" "$dmg_path"
fi
codesign --verify --verbose=2 "$dmg_path"
if [[ "$unsigned_build" == "1" ]]; then
  echo "Built ad-hoc signed validation DMG; Gatekeeper notarization is intentionally pending."
else
  xcrun notarytool submit "$dmg_path" --keychain-profile "$APPLE_NOTARY_PROFILE" --wait
  xcrun stapler staple "$dmg_path"
  xcrun stapler validate "$dmg_path"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg_path"
fi
(cd "$dist_dir" && shasum -a 256 "$(basename "$dmg_path")" > "SHA256SUMS-macos-universal.txt")

if [[ "$unsigned_build" == "1" ]]; then
  echo "Built unsigned validation artifact: $dmg_path"
else
  echo "Built and notarized: $dmg_path"
fi
