# ChatGPT one-click Windows proof — 2026-08-01

> Historical design evidence, reactivated in a narrower form on 2026-08-11. The current implementation again uses the fixed Store ID and trusted WinGet/App Installer background path, but it does not permit the former `winget download --skip-license` local-package fallback because the OpenAI package declares `appLicensing`. See `chatgpt-windows-store-recovery-2026-08-11.md`.

## Scope and safety

- Repository: `E:\Obsidian\workspaces\ai-client-installer`
- Host: Windows 11 Pro build 26100, x64
- This proof did not install or update ChatGPT, Microsoft Desktop App Installer, or any other target client.
- Microsoft/OpenAI packages were downloaded only to `C:\Users\admin\AppData\Local\Temp\ai-client-installer-winget-proof-20260801` for read-only inspection. They are not part of the repository or release output.

## Authoritative contracts checked

- OpenAI desktop download page: <https://chatgpt.com/download/>. Its Windows link targets `get.microsoft.com/installer/download/9PLM9XGG6VKS`; the implementation uses only the fixed product ID and never executes that bootstrap response.
- WinGet install/upgrade: <https://learn.microsoft.com/en-us/windows/package-manager/winget/install> and <https://github.com/microsoft/winget-cli/blob/master/doc/windows/package-manager/winget/upgrade.md>.
- WinGet Store-package download and `--skip-license`: <https://learn.microsoft.com/en-us/windows/package-manager/winget/download>.
- WinGet return-code contract: <https://github.com/microsoft/winget-cli/blob/master/doc/windows/package-manager/winget/returnCodes.md>.
- App Installer stable release: <https://github.com/microsoft/winget-cli/releases/latest> and `https://api.github.com/repos/microsoft/winget-cli/releases/latest`.

## Fixed target identity

Installed unified ChatGPT package observed without mutation:

| Field | Value |
|---|---|
| Store ID | `9PLM9XGG6VKS` |
| Package Identity | `OpenAI.Codex` |
| Package Family | `OpenAI.Codex_2p2nqsd0c76g0` |
| Publisher | `CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B` |
| Version | `26.721.11231.0` |
| Architecture | `x64` |

Local WinGet state before implementation proof:

| Field | Value |
|---|---|
| WinGet CLI | `1.6.10121` |
| Desktop App Installer | `1.21.10120.0` |
| `msstore` health | `0x8A15005E` / pinned certificate mismatch |

This is the bounded self-heal trigger; the implementation does not enable certificate-pinning bypass or reset Store/WinGet policy.

## Current Microsoft stable App Installer artifacts

GitHub Release API returned stable tag `v1.29.280`.

| Asset | Bytes | Release API digest / observed SHA-256 |
|---|---:|---|
| `Microsoft.DesktopAppInstaller_8wekyb3d8bbwe.msixbundle` | 216,775,738 | `0809fa9f52e395d6e7de692331dce847ac991952675116bb4d8aae2ddcc20946` |
| `DesktopAppInstaller_Dependencies.zip` | 97,760,717 | `3bbfcaa5cb011c48fac48d896d64a5c7c6898859a9f3d01555c8cd000f4e2962` |

Bundle proof:

- Authenticode status: `Valid`
- Signer/Publisher: `CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US`
- Bundle Identity: `Microsoft.DesktopAppInstaller`
- Bundle version: `2026.623.1704.0`
- Application payload version: `1.29.280.0`
- Application architectures present: x86, x64, ARM64; resource packages are bundled.

x64 dependency proof:

| Identity | Version | Architecture | Signature/Publisher |
|---|---|---|---|
| `Microsoft.VCLibs.140.00` | `14.0.33519.0` | x64 | Valid / Microsoft |
| `Microsoft.VCLibs.140.00.UWPDesktop` | `14.0.33728.0` | x64 | Valid / Microsoft |
| `Microsoft.WindowsAppRuntime.1.8` | `8000.616.304.0` | x64 | Valid / Microsoft |

The same official dependencies ZIP also contains corresponding ARM64 packages. ARM64 execution remains disabled pending a disposable ARM64 machine proof.

## Implemented execution contract

1. Detect exact ChatGPT AppX identity/family/publisher/architecture/version.
2. Resolve a typed `MicrosoftStore` install plan; no fake direct URL is created.
3. Probe `winget --version`, then the fixed Store ID through `msstore`.
4. If WinGet is missing, below `1.29.0`, or returns `0x8A15005E`, self-heal once from the official stable Release after digest, signature, identity, publisher, architecture and dependency verification.
5. Run exactly one structured `winget install` or `winget upgrade` command with the fixed Store ID, target architecture, silent/non-interactive flags and consent bound to the app confirmation action.
6. Only official download/service transport error codes may invoke one `winget download --skip-license` fallback into a private staging directory.
7. Verify every AppX artifact, read the selected main package manifest (including the target-architecture package inside a bundle), resolve the complete transitive `PackageDependency` graph, enforce Publisher and minimum versions, reject missing/duplicate/unreferenced packages, rebind file identities, and install with structured `Add-AppxPackage` parameters.
8. Re-detect exact target identity/family/publisher/architecture/version; exit code 0 alone is never success, and a lower version is rejected.
9. Policy, entitlement/authentication, license, hash/signature, identity, dependency, architecture and install-policy failures stop immediately and never open Store UI or a bootstrap URI.

## Checks executed

- `cargo fmt --all -- --check` — passed.
- `cargo check --all-targets` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed after explicit Windows PowerShell module-environment isolation.
- `cargo test --all-targets` — 57 passed, 0 failed, 3 environment-sensitive proofs ignored by default: 36 Store/platform/UI/core unit tests, 6 resolver contract tests and 15 security-boundary tests. WorkBuddy coverage now also proves that registered `5.3.8` matches API build `5.3.8.34705286` consistently across UI, preinstall and postcheck.
- All three ignored proofs were executed explicitly and passed: current-host product detection with final WorkBuddy x64 executable inspection; current official WorkBuddy package verification through the embedded x86 bootstrap policy; and `AI_CLIENT_INSTALLER_WINGET_PROOF_ROOT` validation of the registered WinGet plus the current official App Installer bundle/dependency closure.
- Earlier debug/release GUI visual checks passed at the compact default `800×610` inner size. All five products were visible without a scrollbar; ChatGPT showed one `更新` action; opening that action produced a centered confirmation card. The final binary changed detection/verification rather than layout; its exact visual recheck was not repeated, while automated UI regression and final process smoke passed.
- Windows x64 release build — `dist/AI-Client-Installer-windows-x64.exe`, 10,568,704 bytes, SHA-256 `33887fbddaa940cbd291827c7067680685ca5c87dd6e93bbe8e23a0d19836037`; automated UI regression renders WorkBuddy `5.3.8` as current against API `5.3.8.34705286`.

## Remaining environment-sensitive gates

- Disposable Windows x64: first install, installed-old-version update, old/broken WinGet self-heal, `msstore` policy/entitlement failure, and Store UI/process monitoring.
- Disposable Windows x64: determine whether `winget download --skip-license` for `9PLM9XGG6VKS` produces a locally installable complete closure; if not, record fallback No-Go while retaining the primary Store path.
- Disposable Windows ARM64: native package selection, App Installer dependency selection and exact postcheck.
- Final installer Authenticode signing and clean-machine release validation.

Current conclusion: `Implementation complete; validation pending`. No claim is made that the real Store install or fallback passed on this host.
