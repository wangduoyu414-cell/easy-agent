#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
dist_dir="${EASY_AGENT_DIST_DIR:-$repo_root/dist}"
if [[ "$dist_dir" != /* ]]; then
  dist_dir="$repo_root/$dist_dir"
fi
build_root="${EASY_AGENT_MACOS_BUILD_ROOT:-$repo_root/target}"
if [[ "$build_root" != /* ]]; then
  build_root="$repo_root/$build_root"
fi
app_name="easy agent"
bundle_dir="$dist_dir/$app_name.app"
image_source_dir="$build_root/dmg-root"
binary_name="easy-agent"
icon_name="easy-agent.icns"
unsigned_build="${ALLOW_UNSIGNED_MACOS_BUILD:-0}"
if [[ "$unsigned_build" == "1" ]]; then
  dmg_name="easy-agent-macos-universal-UNNOTARIZED-VALIDATION.dmg"
  checksum_name="SHA256SUMS-macos-universal-UNNOTARIZED-VALIDATION.txt"
else
  dmg_name="easy-agent-macos-universal.dmg"
  checksum_name="SHA256SUMS-macos-universal.txt"
fi
dmg_path="$dist_dir/$dmg_name"

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
CARGO_TARGET_DIR="$build_root" cargo test --all-targets
CARGO_TARGET_DIR="$build_root" cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
CARGO_TARGET_DIR="$build_root" cargo build --release --target x86_64-apple-darwin
CARGO_TARGET_DIR="$build_root" cargo build --release --target aarch64-apple-darwin

mkdir -p "$dist_dir"
if [[ -e "$bundle_dir" ]]; then
  if [[ ! -d "$bundle_dir" || -L "$bundle_dir" ]]; then
    echo "Refusing to replace a non-directory app bundle: $bundle_dir" >&2
    exit 1
  fi
  rm -R -- "$bundle_dir"
fi
if [[ -e "$dmg_path" ]]; then
  if [[ ! -f "$dmg_path" || -L "$dmg_path" ]]; then
    echo "Refusing to replace a non-regular DMG: $dmg_path" >&2
    exit 1
  fi
  rm -- "$dmg_path"
fi
mkdir -p "$bundle_dir/Contents/MacOS" "$bundle_dir/Contents/Resources"
cp "$repo_root/packaging/macos/Info.plist" "$bundle_dir/Contents/Info.plist"
cp "$repo_root/packaging/macos/$icon_name" "$bundle_dir/Contents/Resources/$icon_name"
/usr/bin/plutil -replace CFBundleShortVersionString -string "$version" "$bundle_dir/Contents/Info.plist"
/usr/bin/plutil -replace CFBundleVersion -string "$version" "$bundle_dir/Contents/Info.plist"
lipo -create \
  "$build_root/x86_64-apple-darwin/release/$binary_name" \
  "$build_root/aarch64-apple-darwin/release/$binary_name" \
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

if [[ -e "$image_source_dir" ]]; then
  if [[ ! -d "$image_source_dir" || -L "$image_source_dir" ]]; then
    echo "Refusing to replace a non-directory DMG source: $image_source_dir" >&2
    exit 1
  fi
  rm -R -- "$image_source_dir"
fi
mkdir -p "$image_source_dir"
cp -R "$bundle_dir" "$image_source_dir/$app_name.app"
ln -s /Applications "$image_source_dir/Applications"
hdiutil create -volname "$app_name" -srcfolder "$image_source_dir" -ov -format UDZO "$dmg_path"
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
(cd "$dist_dir" && shasum -a 256 "$dmg_name" > "$checksum_name")

if [[ "$unsigned_build" == "1" ]]; then
  echo "Built Gatekeeper-blocked validation artifact: $dmg_path"
else
  echo "Built and notarized: $dmg_path"
fi
