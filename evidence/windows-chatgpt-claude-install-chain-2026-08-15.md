# Windows ChatGPT / Claude installation-chain evidence — 2026-08-15

## Objective

Close the Windows download-to-install loop for the two products that repeatedly failed on new computers, while preserving verified package identity, architecture, version, and post-install checks.

This evidence covers the current working tree based on Git revision `a80348a4c0ba`. The working tree contains the implementation under validation and is not represented as a signed public release.

## Implemented chain

### Claude on Windows

- Resolve Anthropic's official complete x64 or ARM64 MSIX first.
- Use the signed Hong Kong manifest only for explicitly classified availability failures, and select its complete MSIX rather than Claude Setup.
- Verify digest where supplied, AppX signature, Publisher, Identity, architecture, and version before installation.
- Request administrator permission and deploy the already-downloaded local MSIX with the fixed machine-wide AppX provisioning command.
- Capture the elevated process result and surface the actual deployment error instead of treating UAC launch as installation success.
- Recheck the installed Claude package identity, architecture, and version before reporting success.

The installation stage no longer runs Claude Setup and therefore does not need Claude Setup to download the package again. In mainland China, the initial package download may still use the Hong Kong fallback when the official endpoint reports regional unavailability; after the full MSIX is present and verified locally, the administrator deployment itself is local.

### ChatGPT on Windows

- Keep Microsoft's standard OpenAI-provided web installer as the primary route.
- Verify Microsoft Authenticode and the embedded Store product contract before launch.
- Treat Windows Installer `1612 / 0x64C` as the observed "installation source unavailable" condition and automatically enter the existing complete MSIX plus offline-license route.
- Keep cancellation, UAC rejection, signature, identity, architecture, policy, and license errors fail-closed rather than masking them with fallback.
- Recheck the final ChatGPT Package Identity, Family, Publisher, architecture, and version before reporting success.

### Shared network behavior

- Retry transient metadata connection, timeout, and interrupted-body failures up to three attempts.
- Do not retry or bypass certificate, allowlist, signature, identity, architecture, or contract failures.
- The Hong Kong service remains one endpoint; client retry improves short interruptions but is not represented as multi-host redundancy.

## Validation performed

- `cargo test --all-targets`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- Windows x64 and ARM64 target checks: passed.
- Live Claude official/mirror resolution for Windows x64, Windows ARM64, macOS Intel, and macOS Apple Silicon: passed.
- Live ChatGPT macOS official/mirror candidate matching: passed.
- Both Windows executables passed PE architecture, GUI subsystem, ICON, GROUP_ICON, VERSIONINFO, and SHA-256 checks.
- The macOS Universal DMG passed checksum, DMG, x86_64/arm64, ad-hoc codesign, and actual launch checks from a newly mounted image.

## Artifacts

| Artifact | Size | SHA-256 |
|---|---:|---|
| `easy-agent-windows-x64.exe` | 9,610,240 bytes | `660cb99f995a7740093f30d73a37b6d600d2c588efec1252d1c81cb4e2eb16eb` |
| `easy-agent-windows-arm64.exe` | 8,554,496 bytes | `c8737be5b5e5b7c8e9d66fdfd9371b19af351a843606f7d296d201bc76df2605` |
| `easy-agent-macos-universal-UNNOTARIZED-VALIDATION.dmg` | 12,609,009 bytes | `48edf84e3103e5bfd4aba60434f22cc32c498f63e989a43eeb03897dcff58898` |

## Validation still pending

- Windows x64 clean-machine Claude administrator deployment and final installed-package check.
- Windows ARM64 clean-machine Claude administrator deployment and final installed-package check.
- Windows x64/ARM64 clean-machine ChatGPT standard-installer success path and forced `1612/0x64C` offline fallback.
- UAC cancellation and managed-device policy behavior on real Windows machines.
- Windows Authenticode signing for public release.
- Apple Developer ID signing, notarization, and native Apple Silicon launch acceptance for the macOS public release.

Status: implementation and static/live-resolution validation complete; real clean-Windows installation validation pending.

Documentation impact: implementation status, installation-chain evidence, trust/configuration descriptions, and user installation/maintenance documentation were updated because the runtime behavior and recovery path changed.
