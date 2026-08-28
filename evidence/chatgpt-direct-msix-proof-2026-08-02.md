# ChatGPT direct MSIX proof — 2026-08-02

> Historical/superseded evidence. On 2026-08-11 the current manifest moved ahead of both direct MSIX objects (HTTP 404), while an older reachable package declared the restricted `appLicensing` capability. This proof remains valid only for the 2026-08-02 package snapshot and already-authorized local updates; it is not evidence that a new Windows computer can install ChatGPT directly. See `chatgpt-windows-store-recovery-2026-08-11.md`.

## Scope and safety

- Repository: `E:\Obsidian\workspaces\ai-client-installer`
- Host: Windows 11 Pro build 26100, x64
- This proof did not install or update ChatGPT and did not invoke Microsoft Store, WinGet, Desktop App Installer, an installer bootstrap, or account login.
- No OpenAI package was committed to the repository or retained in `dist/`.

## Previously successful local-package path

Windows AppX deployment logs show successful local-file Add/Update operations for the fixed package family, including:

- `Codex-Windows-x64.msix`
- `ChatGPT-Windows-x64-26.721.3996.0.msix`
- `ChatGPT-26.721.11231.0-x64.msix`
- `OpenAI.Codex_26.707.3748.0_x64.msix`

The logged successful deployment used a local MSIX path and AppX deployment flags; it did not require a Store page or a web bootstrap. The current installed identity is:

| Field | Value |
|---|---|
| Package Identity | `OpenAI.Codex` |
| Package Family | `OpenAI.Codex_2p2nqsd0c76g0` |
| Publisher | `CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B` |
| Installed version | `26.721.11231.0` |
| Architecture | `x64` |

## OpenAI production update contract

Read-only inspection of the installed, signed OpenAI desktop application found the production Windows update manifest:

`https://persistent.oaistatic.com/codex-app-prod/windows-store-update.json`

The signed application constructs a complete package URL as:

`releases/{buildVersion}/ChatGPT-{process.arch}.msix`

Live read-only response on 2026-08-02:

```json
{
  "schemaVersion": 1,
  "buildVersion": "26.727.6591.0",
  "storeProductId": "9PLM9XGG6VKS",
  "packageIdentity": "OpenAI.Codex"
}
```

`storeProductId` is not used for installation. The implementation consumes only the fixed manifest contract, the four-part build version and the fixed package identity, then constructs the complete MSIX URL locally.

HEAD proof:

| Architecture | URL suffix | HTTP | Content-Type | Content-Length |
|---|---|---:|---|---:|
| x64 | `/releases/26.727.6591.0/ChatGPT-x64.msix` | 200 | `application/vnd.ms-appx` | 759,477,276 |
| ARM64 | `/releases/26.727.6591.0/ChatGPT-arm64.msix` | 200 | `application/vnd.ms-appx` | 753,464,470 |

These observed version and sizes are evidence for this date, not repository constants.

## Implemented contract

1. Fetch only the fixed OpenAI manifest over HTTPS with the existing bounded metadata client.
2. Require schema `1`, identity `OpenAI.Codex` and an exact four-component AppX version with each component in `u16` range.
3. Map only x64 → `ChatGPT-x64.msix` and ARM64 → `ChatGPT-arm64.msix`.
4. Construct the URL locally under the fixed `persistent.oaistatic.com/codex-app-prod/releases/` prefix; the manifest cannot provide a host, path or command.
5. Download once into a private temporary directory with a 2 GiB limit, redirect allowlist and cancellation.
6. Require a valid AppX signature, exact Publisher, Identity, target architecture and package manifest version equal to the manifest candidate; rebind the file immediately before execution.
7. Deploy the local file with structured `Add-AppxPackage -Path ... -ForceTargetApplicationShutdown` arguments. The implementation does not enable `ForceUpdateFromAnyVersion`; preinstall logic rejects downgrades.
8. Re-detect exact Package Family, Identity, Publisher, architecture and a version not lower than the candidate. Exit code zero alone is not success; delayed registration is bounded and may end as `ResultUnknown`.
9. Any failure stops this product. There is no fallback to Store, WinGet, Desktop App Installer, a bootstrap package, FE3 capture or third-party mirror.

## Trust note when no separate SHA is published

The manifest does not currently publish a package SHA-256. The direct path remains fail-closed because the MSIX/AppX signature covers package contents and execution additionally requires the fixed HTTPS path, exact Publisher/Identity/Family/architecture/version, stable-file rebinding and postcheck. No identity check is relaxed because an independent hash is absent.

## Validation status

- The manifest adapter, contract-change rejection, direct resolver trust entry, URL boundary, package-version binding, AppX postcheck identity/publisher and Hermes regression fixture have automated tests.
- Current official manifest and x64/ARM64 asset availability were verified read-only.
- `cargo fmt --all -- --check`, `cargo check --all-targets`, Clippy with `-D warnings`, and `cargo test --all-targets` passed. The final test set has 70 passed and 3 environment-sensitive proofs ignored by default.
- The current-host detection proof passed explicitly and confirmed Hermes `0.19.1` plus the final x64 WorkBuddy application identity.
- Live GUI inspection showed ChatGPT `26.721.11231.0` with one enabled update action to `26.727.6591.0`; the confirmation card displayed `X64 · Msix · persistent.oaistatic.com`. The action was cancelled before execution.
- Final unsigned Windows x64 test artifact: `dist/AI-Client-Installer-windows-x64.exe`, 10,597,888 bytes, SHA-256 `d7f7e6e3fac236ec67d0600a19f1f5cd014b42c9f746c61a394e1cabe9afde17`.
- The final release EXE remained running during a five-second process smoke test and was then closed by the test harness.
- Real first install and old-version update were not executed on this host.
- Disposable Windows x64/ARM64 evidence remains required, including proof that Store, WinGet, bootstrap and login windows do not start.

Current conclusion: `Implementation complete; validation pending`.
