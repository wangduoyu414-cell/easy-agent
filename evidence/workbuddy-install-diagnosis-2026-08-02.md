# WorkBuddy installation diagnosis — 2026-08-02

## Observed host state

- Windows 11 Pro build 26100, x64.
- Registered uninstall entry: `WorkBuddy 5.1.7` / version `5.1.7` / Publisher `Tencent Technology (Shenzhen) Company Limited` / HKCU.
- Existing install path from the uninstall command: `E:\workbuddy`.
- The latest reported click is preserved in `%LOCALAPPDATA%\AI Client Installer\logs\operations.jsonl`: download completed, verification started, then failed with `unsupported PE machine 0x014c`. No `AwaitingUserInstall` or `Installing` state was emitted, so the vendor installer was never launched.
- No recent MsiInstaller, AppX, Defender, Code Integrity or AppLocker evidence exists for WorkBuddy, and the installed files retained their earlier timestamps. Process-creation audit event 4688 was not available, but the persistent operation log now closes the application-side failure stage.

## Root causes closed in code

1. The previous detector required `DisplayName == WorkBuddy`, so it missed the real versioned entry `WorkBuddy 5.1.7` and incorrectly offered a first install.
2. Direct-package postcheck performed one immediate registry read after installer exit, which could misclassify asynchronous installers or delayed registration.
3. Operation updates existed only in memory; closing the GUI discarded the stage and final error needed for diagnosis.
4. The verifier treated the EXE bootstrap PE machine as if it had to equal the target application architecture. The official WorkBuddy x64 channel currently ships an x86 NSIS bootstrap, so the valid package was rejected before execution.
5. After the bootstrap fix, installation succeeded and registered `5.3.8`, while the update API target was `5.3.8.34705286`. Generic four-component comparison treated the vendor's three-component registered version as older, causing a 90-second `ResultUnknown` despite successful installation.

## Implemented behavior

- PowerShell returns bounded uninstall candidates; Rust accepts only `WorkBuddy` or a pure numeric dotted suffix and requires the exact Tencent Publisher.
- Existing `5.1.7` is now shown as installed and the `5.3.8.34705286` official candidate is shown as an update.
- The embedded WorkBuddy Windows x64 trust entry pins the current bootstrap PE machine to x86 and pins the final executable name to `WorkBuddy.exe`. The exception is entry-scoped; Hermes and other EXE products still require their normal target machine.
- Registry detection resolves the pinned final executable through InstallLocation, DisplayIcon or UninstallString and reads its PE machine. Postcheck requires the final `WorkBuddy.exe` to be x64; a present malformed/wrong-architecture file fails, while a not-yet-created file remains retryable.
- WorkBuddy version comparison now uses the three release components actually persisted by the vendor. UI state, preinstall duplicate prevention and postinstall verification all treat registered `5.3.8` as the same release as API build `5.3.8.34705286`; other products retain full numeric comparison.
- A zero installer exit code starts a bounded postcheck window of about 90 seconds. Wrong identity/architecture fails immediately; absent/unknown/old state retries; timeout becomes `ResultUnknown`.
- Batch events are persisted to `%LOCALAPPDATA%\AI Client Installer\logs\operations.jsonl`, with repeated download progress suppressed, sensitive paths/URLs redacted and 1 MiB rotation.

## Verification

- Final checks passed: `cargo check --all-targets`, Clippy with `-D warnings`, and `cargo test --all-targets` with 57 passed, 0 failed and 3 environment-sensitive proofs ignored by default.
- The current official response resolved version `5.3.8.34705286` and `https://download.codebuddy.cn/workbuddy/saas/win32-x64-user/WorkBuddy-win32-x64-user-5.3.8.34705286-e9991e2b.exe` (407,285,928 bytes). Observed SHA-256: `C111BC3F54A0E53FA04924313AE660125EEBFFAFCD5AC7722DA7C3C03402CB7A`.
- Full read-only inspection: PE machine `0x014c` (x86), NSIS marker present, Authenticode `Valid`, signer `Tencent Technology (Shenzhen) Company Limited`, ProductName `WorkBuddy`, ProductVersion `5.3.8`.
- Existing local files confirm the vendor split: `E:\workbuddy\WorkBuddy.exe` is x64 and signed by Tencent; `Uninstall WorkBuddy.exe` is x86.
- Explicit current-host detection proof passed for all five products and confirmed WorkBuddy is installed with the trusted Publisher and its pinned final executable is x64.
- Explicit current-official-package proof passed through the same Rust verification path with the embedded x86 bootstrap trust pin.
- Explicit current official App Installer/WinGet bundle and dependency-closure proof passed.
- An earlier release GUI check showed `已安装 5.1.7 · 可更新至 5.3.8.34705286` and an enabled `更新` button; the final build retained the same UI path and passed its automated regression, but was not clicked again.
- Final unsigned Windows x64 artifact: `dist/AI-Client-Installer-windows-x64.exe`, 10,568,704 bytes, SHA-256 `33887fbddaa940cbd291827c7067680685ca5c87dd6e93bbe8e23a0d19836037`.
- The user completed the vendor-guided WorkBuddy update successfully. Registry, signed main executable and current-host proof confirm installed version `5.3.8` and x64 `WorkBuddy.exe`; the remaining `ResultUnknown` was only the now-fixed version-granularity mismatch.

## Remaining gate

A disposable Windows x64 snapshot must still execute the WorkBuddy update through the single UI action and preserve the operation log, installer exit outcome, final registered version, Publisher and final `WorkBuddy.exe` x64 proof. Until that proof exists, the product remains `validation pending` rather than a signed production release.
