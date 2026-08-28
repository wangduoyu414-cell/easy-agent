# easy agent macOS test build — 2026-08-11

## Artifact

- File: `dist/easy-agent-macos-universal.dmg`
- Size: approximately 12 MiB
- SHA-256: `c4cbf1092878e353660002b7420e53d8ae5084d5054f6fa6a34fd27c31934501`
- Bundle display name: `easy agent`
- Bundle ID: `io.github.wangduoyu414-cell.easy-agent`
- Version: `0.1.0`
- Minimum macOS: `12.0`
- Architectures: `x86_64` and `arm64`

## Checks executed

- Fresh-target default Rust test suite passed; environment-sensitive tests remained ignored unless run explicitly.
- Clippy passed with `-D warnings`.
- Both Apple release targets compiled and were merged into one Universal Mach-O.
- App bundle and DMG ad-hoc code signatures passed `codesign --verify`.
- DMG mounted read-only and contained the expected `easy agent.app`, Info.plist, icon resources and Universal executable.
- The application launched directly from the mounted DMG, remained running for ten seconds, and produced no stderr/stdout failure.
- A desktop screenshot confirmed Chinese text, Arabic numerals, application icons and action buttons rendered normally without the prior missing-font symptom.
- Live metadata tests passed for all enabled macOS products on Intel and Apple Silicon; ChatGPT and Claude verified fallback contracts also passed.

## Distribution status

This is an ad-hoc-signed validation artifact because no Apple Developer ID Application identity or notary profile is installed on the build Mac. It is suitable for local testing but is not a notarized public release. Gatekeeper may require the tester to use Finder's Open action; public distribution still requires Developer ID signing, notarization and stapling.

The copied test bundle is available at:

`smb://192.168.0.119/qimistudio/easy-agent-测试版-2026-08-11-1558/`
