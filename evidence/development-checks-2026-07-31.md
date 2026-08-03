# Development checks — 2026-07-31

Environment: Windows 11 Pro build 26100, x64; Rust/Cargo 1.94.1.

Executed successfully:

- `cargo fmt --all`
- `cargo check --all-targets`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test --all-targets` — 17 tests passed (1 platform unit, 5 resolver fixtures, 11 security/orchestration)
- `cargo test --test resolver_fixtures` — 5 passed
- `cargo test --test security_boundaries` — 6 passed
- Debug GUI process smoke test — process remained running for 12 seconds
- Windows UI visual inspection — reference-aligned pure-white centered layout rendered correctly at 1040×760, including Chinese title/subtitle, five product rows, official product icons, compact version/status text, outlined right-side actions, platform footer, and fail-closed unavailable states
- Official icon provenance — WorkBuddy current official-site SVG; Hermes Agent and CC Switch current official-repository app icons; Claude and ChatGPT icons extracted byte-for-byte from installed, signed official Windows packages; sources and hashes recorded in `assets/icons/official/SOURCES.md`
- Hermes current Windows bootstrap proof — 7,597,376-byte official EXE, SHA-256 `505dfb4c2c1052b055e3fc694a76cb7ce093a64962c7713aa294f5549c6734f5`, valid Authenticode signer `Nous Research Inc.`, product identity `Hermes`
- CC Switch `3.19.1` proof — 13,033,472-byte official x64 MSI, SHA-256 `cb13c9f716d4cd891690a1c1e6a07090eedf3a245a6c3750002cf84c3f3b36f1`, product `CC Switch`, version `3.19.1`, MSI template `x64;0`, and current release minisign verified against the embedded official updater key
- Live official resolution during GUI smoke test — Hermes `0.19.1`, Claude `1.24012.9`, WorkBuddy `5.3.5.34189228`, CC Switch `3.19.1`; ChatGPT Windows remained explicit No-Go
- Windows x64 release build after UI redesign — `dist/AI-Client-Installer-windows-x64.exe`, 9,532,928 bytes, SHA-256 `3b0fe7ab32b87cb83465a8765cd2f5737f9b307cc632341446ecef7cf1a9076e`
- Release EXE process smoke test — process remained running for 10 seconds
- `cargo audit` — scanned 473 locked dependencies against 1,177 RustSec advisories; no vulnerability reported
- Windows ARM64 Rust standard library target installed successfully

Not executed / not available:

- No third-party target client was downloaded or installed during implementation checks.
- No clean Windows x64/ARM64 installation matrix.
- Windows ARM64 release cross-build attempted and failed before linking because this host lacks the Visual Studio C++ ARM64/clang-cl compiler component required by `aws-lc-sys`; no ARM64 artifact was produced.
- No macOS build, codesign, notarization, quarantine, or Gatekeeper validation.
- No Windows Authenticode signing of the installer assistant.
