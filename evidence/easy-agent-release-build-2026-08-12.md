# easy agent cross-platform validation build — 2026-08-12

## Outcome

- Windows x64 and ARM64 GUI executables were rebuilt from the current worktree.
- A macOS Universal validation DMG was rebuilt with x86_64 and arm64 slices, the current icon, and an Applications drag target.
- The Mac build launched on the Intel build machine and rendered Chinese text and Arabic numerals correctly.
- The Mac validation artifact is ad-hoc signed, not Developer ID signed or notarized. Its file name therefore remains `UNNOTARIZED-VALIDATION`, and it is not a public-distribution package.

## Windows build checks

- `cargo test --all-targets`: 55 passed, 2 ignored.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo xwin check --locked --target x86_64-pc-windows-msvc --all-targets`: passed.
- `cargo xwin check --locked --target aarch64-pc-windows-msvc --all-targets`: passed.
- x64 PE machine: `0x8664`; ARM64 PE machine: `0xAA64`.
- Both executables use the Windows GUI subsystem and include ICON, GROUP_ICON, and VERSIONINFO resources.
- Generated SHA-256 checksum files verified successfully.

## macOS build and application checks

- The packaging script reran the full test, Clippy, formatting, x86_64 release, and arm64 release stages.
- The final Mach-O contains `x86_64 arm64`.
- Bundle ID: `io.github.wangduoyu414-cell.easy-agent`.
- Minimum declared macOS: `12.0`.
- App and DMG ad-hoc signatures passed `codesign --verify`.
- The DMG mounted read-only and contained `easy agent.app` plus `Applications -> /Applications`.
- The generated SHA-256 checksum verified successfully.
- The application remained running during an actual launch check; a desktop screenshot confirmed the current UI and fonts rendered correctly.

## Live macOS client checks

On the Intel build Mac, the current production verification path downloaded and verified the real x64 and ARM64/Universal artifacts for WorkBuddy, CC Switch, Claude, and ChatGPT. All eight candidates passed their configured Bundle ID, Apple Team ID, version, architecture slice, codesign, and Gatekeeper checks. CC Switch and ChatGPT updater signatures also passed. WorkBuddy continued to report the vendor digest mismatch already covered by its narrowly scoped Apple-platform-signature policy.

Claude x64 and ARM64/Universal candidates were additionally exercised through the full temporary-target loop:

- fresh install: passed;
- in-place update: passed;
- forced failed-update rollback: passed;
- forced failed-fresh-install cleanup: passed.

These tests did not modify `/Applications` or `~/Applications`.

## Remaining release gates

- Windows executables are not Authenticode signed and still require clean Windows x64/ARM64 device acceptance.
- No valid `Developer ID Application` identity or `notarytool` keychain profile exists on the build Mac. The public Mac DMG cannot be generated until those credentials are installed locally and the script completes notarization, staple, and Gatekeeper assessment.
- An Intel Mac cannot replace native Apple Silicon launch acceptance, although the arm64 binary and all arm64 third-party package verification/temporary-install paths were executed.
