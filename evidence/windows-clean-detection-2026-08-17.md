# Windows clean-machine detection evidence — 2026-08-17

## Objective

Make the initial status scan deterministic on a Windows x64 computer with none of the five managed products installed, without changing download sources, artifact trust, or installation concurrency.

## Implemented behavior

- A full Windows refresh no longer launches five identical PowerShell/AppX scans.
- Claude and ChatGPT share one exact AppX `Main`-package probe.
- WorkBuddy, Hermes, and CC Switch share an independent uninstall-registry/fixed-directory probe.
- Each probe has a 20-second process timeout and produces UTF-8 JSON.
- AppX and registry errors are preserved as detection failures instead of being suppressed and reported as absence.
- A detection failure disables installation and is rejected again at the orchestrator boundary.
- The UI shows the completed local result while the independent online version lookup continues, for example `未安装 · 正在获取最新版本`.

## Validation

- `cargo test --all-targets`: passed; 60 passed, 2 ignored in the main unit-test target, plus branding/resolver/security targets passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo xwin check --locked --target x86_64-pc-windows-msvc --all-targets`: passed.
- `cargo xwin check --locked --target aarch64-pc-windows-msvc --all-targets`: passed with the existing cross-build resource-tool warning.
- PowerShell parser check for the embedded detection script: passed.
- `./packaging/build-windows-from-macos.sh x64`: passed.
- PE machine: `IMAGE_FILE_MACHINE_AMD64 (0x8664)`.
- Subsystem: `IMAGE_SUBSYSTEM_WINDOWS_GUI`.
- Resources: ICON, GROUP_ICON, VERSIONINFO present.

## Artifact

- File: `easy-agent-windows-x64.exe`
- Size: 9,630,208 bytes
- SHA-256: `2199cefb886183041139d3388eaa80591b3eab9650800916c4beef811a9d569b`

## Validation pending

- Launch on a clean Windows x64 computer with none of the five products installed.
- Confirm WorkBuddy/Hermes/CC Switch reach `未安装 · 正在获取最新版本` independently when AppX is delayed or unavailable.
- Confirm Claude/ChatGPT show a bounded, explicit detection failure when the AppX probe is blocked rather than remaining indefinitely in `检测中`.
- Confirm a slow or unavailable vendor metadata endpoint changes only the online-version state and does not erase the local installed/absent state.

Status: implementation complete; Windows x64 clean-machine validation pending.

Documentation impact: implementation status, maintenance rules, and installation guidance were updated because the Windows scan lifecycle and user-visible status contract changed.
