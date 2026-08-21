#!/usr/bin/env bash
set -euo pipefail

requested_architecture=${1:-all}
case "$requested_architecture" in
    all|x64|arm64) ;;
    *)
        printf 'usage: %s [all|x64|arm64]\n' "$0" >&2
        exit 2
        ;;
esac

repo_root=$(cd "$(dirname "$0")/.." && pwd)
dist_dir=${EASY_AGENT_DIST_DIR:-"$repo_root/dist"}
if [[ "$dist_dir" != /* ]]; then
    dist_dir="$repo_root/$dist_dir"
fi
build_root=${EASY_AGENT_WINDOWS_BUILD_ROOT:-"$repo_root/target/windows-xwin"}

if ! command -v cargo-xwin >/dev/null 2>&1; then
    printf 'cargo-xwin is required: cargo install cargo-xwin --locked\n' >&2
    exit 1
fi
if [[ -n "${LLVM_BIN:-}" ]]; then
    llvm_bin="$LLVM_BIN"
elif command -v brew >/dev/null 2>&1; then
    llvm_bin="$(brew --prefix llvm)/bin"
else
    printf 'LLVM_BIN must point to a directory containing llvm-rc\n' >&2
    exit 1
fi
llvm_rc="$llvm_bin/llvm-rc"
if [[ ! -x "$llvm_rc" ]]; then
    printf 'llvm-rc was not found at %s\n' "$llvm_rc" >&2
    exit 1
fi

cd "$repo_root"
cargo fmt --all -- --check
CARGO_TARGET_DIR="$build_root/host" cargo check --all-targets
CARGO_TARGET_DIR="$build_root/host" cargo clippy --all-targets --all-features -- -D warnings
CARGO_TARGET_DIR="$build_root/host" cargo test --test resolver_fixtures
CARGO_TARGET_DIR="$build_root/host" cargo test --test security_boundaries

mkdir -p "$dist_dir"
build_architecture() {
    local architecture="$1"
    local target suffix source_executable output_executable
    case "$architecture" in
        x64)
            target='x86_64-pc-windows-msvc'
            suffix='windows-x64'
            ;;
        arm64)
            target='aarch64-pc-windows-msvc'
            suffix='windows-arm64'
            ;;
    esac
    rustup target add "$target"
    PATH="$llvm_bin:$PATH" RC_PATH="$llvm_rc" cargo xwin build \
        --release --locked --target "$target" --target-dir "$build_root" --bin easy-agent
    source_executable="$build_root/$target/release/easy-agent.exe"
    output_executable="$dist_dir/easy-agent-$suffix.exe"
    cp "$source_executable" "$output_executable"
    shasum -a 256 "$output_executable" \
        | sed "s#  .*#  $(basename "$output_executable")#" \
        >"$dist_dir/SHA256SUMS-$suffix.txt"
    printf 'Built %s\n' "$output_executable"
}

case "$requested_architecture" in
    all)
        build_architecture x64
        build_architecture arm64
        ;;
    *) build_architecture "$requested_architecture" ;;
esac

for artifact in "$dist_dir"/easy-agent-windows-*.exe; do
    [[ -f "$artifact" ]] || continue
    shasum -a 256 "$artifact" | sed 's#  .*/#  #'
done | sort >"$dist_dir/SHA256SUMS-windows.txt"
