# Multi-environment acceptance test — 2026-08-21

## Scope

- Revision: `e7b7eae` on `agent/easy-agent-branding-homepage`
- Host: Windows 11 Pro x64, build 22631
- Test lab: `G:\QIMIStudio\easy-agent-test-lab-20260821`
- Requested coverage: current host, clean Windows 10 x64, clean Windows 11 x64, missing/conflicting developer environments, standard-user behavior, interrupted/retry paths, and product-specific prerequisites.
- Existing Hyper-V virtual machines are out of scope and are not modified.

## Environment matrix

| Environment | Planned scenarios | Status |
| --- | --- | --- |
| Current Windows 11 x64 host | build, unit tests, packaging, launch, installed-product detection | Core checks complete; known failures recorded |
| Clean Windows 10 22H2 x64 VM | fresh launch/install, no Node/Python/Git/WinGet, standard user | Console detection/resolution, downloads, and CC Switch install complete; GUI blocked by OpenGL |
| Clean Windows 11 x64 VM | fresh launch/install, admin vs standard user, latest-version behavior | Full production install and launch complete for WorkBuddy, CC Switch, Claude, ChatGPT, and Hermes; failure/retry/cancel paths also covered |
| Conflicting Node/npm VM snapshot | Hermes with incompatible Node/npm, retry and diagnostics | Partial: isolated portable Node 22 behavior observed; a separately preinstalled incompatible Node was not executed |
| Network-failure VM snapshot | offline/proxy-like failures and recovery | Complete on Win11 by disabling/re-enabling the guest adapter; cancel and retry also covered |
| Windows ARM64 | native launch and installer matrix | Blocked: requires ARM64 Windows hardware or a supported ARM64 virtualization host |
| macOS Intel / Apple Silicon | launch, Gatekeeper, install/update/rollback | Blocked: requires corresponding Mac hardware |

## Findings

### EA-TEST-001 — Claude mirror fixture signature is invalid

- Severity: High
- Reproduction: `cargo test --all-targets`
- Stable reproduction: `cargo test --test resolver_fixtures verifies_the_deployed_claude_mirror_manifest_fixture_before_parsing -- --exact --nocapture`
- Actual result: `Contract("mirror manifest signature is invalid")`
- Location: `tests/fixtures/claude-mirror/latest.json` and `latest.json.minisig`
- Impact: the full test suite fails and the fixture no longer proves the signed Claude fallback contract.
- Status: Confirmed; not fixed in this test pass.
- 2026-08-23 release follow-up: resolved by replacing the stale fixture with the current deployed manifest/signature pair and marking both signed fixture files as byte-preserving in `.gitattributes`. The exact signature test and the full `cargo test --all-targets` run pass.

### EA-TEST-002 — Clippy fails with Rust 1.95

- Severity: Medium
- Reproduction: `cargo clippy --all-targets --all-features -- -D warnings`
- Actual result: `clippy::needless-return` at `src/platform/mod.rs:134`.
- Impact: the documented quality gate and packaging validation cannot pass with the current stable toolchain on this host.
- Status: Confirmed; not fixed in this test pass.
- 2026-08-23 release follow-up: resolved by returning the Windows detection expression directly. `cargo clippy --all-targets --all-features -- -D warnings` passes on the current Windows host.
- 2026-08-23 remote follow-up: GitHub's newer Rust 1.98 added `clippy::chunks-exact-to-as-chunks` in a test. The test now uses `as_chunks::<2>()`, and release workflows pin Rust 1.95.0 so future stable-channel drift cannot silently change the release gate.

### EA-TEST-003 — Windows packaging continues after failed validation

- Severity: Critical
- Reproduction: run `packaging/build-windows.ps1 -Architecture x64` with Cargo available on `PATH`.
- Actual result: Clippy and `resolver_fixtures` fail, but the script continues, creates `dist/easy-agent-windows-x64.exe`, prints a SHA-256, and exits with code 0.
- Cause boundary: `$ErrorActionPreference = 'Stop'` does not by itself turn native executable non-zero exit codes into terminating PowerShell errors in this execution environment; the script does not explicitly inspect `$LASTEXITCODE` after Cargo commands.
- Impact: an artifact can be presented as built even though mandatory validation failed.
- Status: Confirmed; not fixed in this test pass.
- 2026-08-23 release follow-up: resolved by routing every Cargo invocation through an explicit exit-code check. A successful x64 run completed all gates and produced the release EXE; an intentional invalid Rust flag stopped at `cargo check` and propagated exit code 101 instead of continuing to artifact generation.

### EA-TEST-004 — Cargo is installed but unavailable on the inherited PATH

- Severity: Environment / Low
- Reproduction: invoke `packaging/build-windows.ps1` from the current desktop shell.
- Actual result: `cargo` is not recognized, while `C:\Users\Administrator\.cargo\bin\cargo.exe` exists and works when called by absolute path.
- Impact: the documented packaging command fails until a new shell is opened or the Cargo bin directory is added to that process's PATH.
- Status: Host environment observation; distinguish from a project defect.

### EA-TEST-005 — The GUI cannot launch without OpenGL 2.0

- Severity: High
- Environment: clean Windows 10 22H2 x64 Hyper-V VM, 2 GB dynamic memory, no Node/Python/Git/WinGet.
- Reproduction: launch `easy-agent-windows-x64.exe` on the default Hyper-V display adapter.
- Actual result: process exits immediately with `Error: OpenGL(PainterError("egui_glow requires opengl 2.0+. "))`.
- Repository evidence: `Cargo.toml` enables only eframe's `glow` renderer.
- Impact: the application cannot run in a standard Hyper-V VM and may also fail in remote, software-rendered, or older-GPU environments. This blocks ordinary clean-VM acceptance automation unless GPU pass-through or a software-rendering fallback is added.
- Status: Confirmed. A test-only WGPU renderer experiment could create a window in Hyper-V but rendered blank content, so it is not evidence of a viable fallback.

### EA-TEST-006 — Current-host ignored test hard-codes an obsolete Hermes version

- Severity: Medium
- Reproduction: `cargo test platform::windows::tests::current_host_detection_script_is_parseable_and_finds_workbuddy -- --ignored --exact --nocapture`
- Actual result: runtime detection correctly returns Hermes `0.20.4`, but the test asserts `0.19.1` and fails.
- Impact: enabling the host proof produces a false regression whenever Hermes is updated.
- Status: Confirmed; the assertion should validate the current detected contract rather than a historical installed version.

### EA-TEST-007 — ChatGPT update action remains enabled after an explicit refresh

- Severity: Medium / known product issue
- Environment: current Windows 11 host, installed ChatGPT `26.818.3698.0`.
- Reproduction: launch the x64 artifact, invoke `刷新状态` through UI Automation, and inspect the ChatGPT controls after the refresh completes.
- Actual result: status remains `已安装 26.818.3698.0 · 可检查并安装更新` and the `更新` button remains enabled.
- Impact: a machine that is already current still appears to have an actionable update, because the Microsoft web-installer path does not supply a comparable target version.
- Status: Confirmed; deliberately not changed in this test pass.

### EA-TEST-008 — Download progress emits an excessive number of updates

- Severity: Low / performance and observability
- Environment: clean Windows 11 x64 VM; CC Switch 3.20.0 direct-install path.
- Reproduction: run the real CC Switch install probe and count download-state callbacks for the 13,508,608-byte MSI.
- Actual result: 913 progress callbacks were emitted on the Win10 VM and roughly 1,600 on the Win11 run for this small package. The 7,946,048-byte Hermes run emitted 560 callbacks.
- Cause boundary: `src/core/download.rs` invokes `on_progress` after every successful 128 KiB-or-smaller network read without a time or byte-delta threshold.
- Impact: unnecessary cross-thread/UI repaint traffic and noisy probe output; the effect will be larger for the 247–415 MB installers.
- Status: Confirmed; not fixed in this test pass.

### EA-TEST-009 — CC Switch is falsely reported as installed for another Windows user

- Severity: High
- Environment: clean Windows 11 x64 VM with `ealab-admin` and standard user `ealab-user`.
- Reproduction: install CC Switch 3.20.0 through the project as `ealab-admin`, then run the project detection probe as `ealab-user`.
- Actual result: Windows Installer creates an HKLM uninstall record, but `InstallLocation` is `C:\Users\ealab-admin\AppData\Local\Programs\CC Switch\`. The standard user cannot access the installed executable, while project detection still returns `installed=true`, version `3.20.0`, evidence `Uninstall:CC Switch [HKLM]`.
- Impact: a second user is offered neither a usable launch nor a new install because a machine-wide registry record masks a per-user, inaccessible installation.
- Status: Confirmed. A fresh standard-user install succeeds into that user's profile, so the defect is specifically the HKLM record masking another user's inaccessible per-profile installation.

### EA-TEST-010 — Cancelling the Hermes vendor bootstrap is not represented or contained correctly

- Severity: High / integration and recovery
- Environment: clean Windows 11 x64 VM, interactive administrator session, with no Node, npm, Python, Git, or Hermes present before launch.
- Reproduction: start Hermes 0.20.4 through the real project install path, click the vendor `INSTALL` action, allow dependency setup to reach the desktop `npm ci` stage, then click the vendor `Cancel` action.
- Actual result:
  - The vendor bootstrap exits with code 0 even though Hermes is not installed.
  - easy-agent therefore treats the process as a normally completed installer, performs all 46 postcheck attempts at two-second intervals, and can only return `厂商安装器已正常退出，但暂未确认目标版本` instead of a cancelled result.
  - The vendor bootstrap's descendant PowerShell, `cmd.exe`, and `node.exe` processes continue after its top-level UI process exits.
  - The cancelled run leaves no Hermes product registration but leaves about 2,069.6 MiB under `%LOCALAPPDATA%\hermes`, 652.6 MiB of WinGet packages, the 7.6 MiB verified installer copy, PATH-visible Git/Node/npm/uv/ripgrep/ffmpeg tools, and installed WinGet packages `BurntSushi.ripgrep.MSVC` and `Gyan.FFmpeg`.
- Impact: users can wait an additional ~92 seconds after cancelling, receive a misleading normal-exit/result-unknown message, retain active background work, and be left with multiple gigabytes of partial environment changes.
- Cause boundary: the vendor bootstrap reports success and does not contain its child process tree; easy-agent currently trusts the top-level exit code and has no product-specific recovery disclosure for this case.
- Status: Confirmed; not fixed in this test pass. A later uninterrupted 5 GiB run completed all 16 stages and proves that the cancellation result is not the normal successful-install behavior.

### EA-TEST-011 — Hermes has substantial prerequisites and resource needs but no preflight warning

- Severity: Medium / environment readiness
- Environment: Windows 11 VM dynamically reduced to 1,024 MiB assigned RAM under host pressure; Hyper-V reported about 3,553 MiB guest demand.
- Reproduction: launch the current Hermes bootstrap on a machine with no Node, npm, Python, or Git.
- Actual result: the vendor's 16-step interactive bootstrap installs or configures uv, Python 3.11, portable Git, portable Node.js 22 and npm, ripgrep, ffmpeg, a Git checkout, a Python virtual environment, Python dependencies, Node dependencies, browser-use, and the desktop build. It also depends on WebView2, WinGet, GitHub, Microsoft WinGet CDN, npm/Python package sources, and several gigabytes of disk. At 1–1.5 GiB RAM it spent roughly an hour and reached severe paging before completion.
- Additional observation: the bootstrap tries GitHub SSH first; where outbound port 22 is blocked it waits for failure before falling back successfully to HTTPS.
- Impact: users can interpret the long vendor phase as a hang, and failure can occur only after large downloads and machine changes. The current confirmation UI does not state the resource, WinGet, network, or expected-duration requirements.
- Status: Confirmed environment/readiness gap; not fixed in this test pass.
- 2026-08-23 follow-up: with the guest allowed to grow to 5 GiB, the vendor's 16 stages completed successfully in about 30 minutes after clicking `INSTALL`. No system Node, npm, Git, Python, ripgrep, or ffmpeg needed to be preinstalled; the vendor bootstrap prepared them itself. The full easy-agent probe, including the final user interaction, independent relaunch, and postcheck, took 3,075 seconds and returned exit code 0.

### EA-TEST-012 — The original Win11 clean snapshot performs a delayed Windows reconfiguration reboot

- Severity: Test-lab environment / High for result validity
- Environment: `EasyAgentLab-Win11-25H2`, original `Baseline-Clean-OS` checkpoint.
- Reproduction: restore the original checkpoint, switch the console to `ealab-admin`, and wait about three minutes.
- Actual result: `CloudExperienceHostBroker.exe` requests an operating-system reconfiguration reboot (`User32` event 1074, reason `0x20004`). The first WorkBuddy attempt was terminated while its verified vendor installer was running, leaving no installed product.
- Impact: an interrupted install can be misattributed to easy-agent or the vendor unless system events are checked. The post-reconfiguration automatic login is also temporary and logs off after about ten seconds.
- Resolution for testing: complete the reconfiguration, configure the administrator login again, verify a stable desktop for more than three minutes, confirm all five products absent, and create `Baseline-Clean-OS-Stable`. Subsequent clean restores used that checkpoint successfully.
- Status: Test harness corrected; not a product defect.

### EA-TEST-013 — WorkBuddy requires several visible vendor-wizard actions

- Severity: Low / user guidance
- Environment: stable clean Windows 11 x64 VM.
- Actual result: after easy-agent downloads and verifies the 415,167,976-byte package, the vendor installer waits for the user to choose the installation scope, accept the default installation directory, and click `完成`. First launch also shows a normal Windows Firewall prompt.
- Impact: without interacting with the vendor window, WorkBuddy appears to remain in the installing state even though easy-agent and the package are functioning correctly.
- Result: choosing `仅为我安装 (ealab-admin)` completed installation as WorkBuddy `5.3.14`; easy-agent matched it to API build `5.3.14.36279234`, verified the x64 executable and Tencent publisher, and launched the login window.
- Status: Installation succeeds; clearer product-specific guidance would reduce false hang reports.

### EA-TEST-014 — Hermes emits contradictory optional-component warnings

- Severity: Medium / diagnostics
- Environment: successful clean Windows 11 x64 Hermes `0.20.5` run.
- Actual result:
  - the vendor log reports `TUI npm install failed`, while the captured npm output says Node dependencies were installed;
  - it reports that the Computer Use driver did not produce a compatible runtime, while the driver installer reports success.
- Independent validation:
  - both root and TUI `npm list --depth=0 --json` exit 0;
  - `cua-driver 0.21.0 --version` and `list-tools` exit 0;
  - the `cua-driver-serve` scheduled task starts successfully and `autostart status` becomes `registered (running)`.
- Impact: users can receive apparent failure warnings for components that are actually usable, making support diagnosis unreliable.
- Status: Confirmed vendor diagnostic inconsistency; the final Hermes stage result and easy-agent postcheck both succeeded.

### EA-TEST-015 — The locally built Hermes desktop executable is unsigned

- Severity: Informational / trust boundary
- Actual result: the downloaded Hermes bootstrap is verified before launch, but the final locally built `Hermes.exe` reports Authenticode status `NotSigned`.
- Additional evidence: the checkout origin is exactly `https://github.com/NousResearch/hermes-agent.git`, commit `706f33d42415d706b8f93dd299f4b317428e4a6b`, branch `main`; the bootstrap marker and fixed product layout pass easy-agent detection as Hermes `0.20.5` x64.
- Impact: Windows cannot attribute the final locally built desktop binary to an Authenticode publisher even though the bootstrap and source/layout checks pass.
- Status: Upstream packaging behavior observed; not changed in this test pass.

## 2026-08-23 real-install completion

All results below used easy-agent's production `resolve_install_plan` and `run_install_plan` path, not a direct manual package install.

| Product | Production result | Installed identity/version | Independent launch result | User/environment reason it was previously not installed |
| --- | --- | --- | --- | --- |
| WorkBuddy | Success, probe exit 0 | `WorkBuddy 5.3.14`, Tencent publisher, x64 | Login window rendered and responded | First attempt was interrupted by the VM's delayed Windows reboot; successful attempt required completing the vendor wizard |
| Claude | Success, probe exit 0 | `Claude_1.34493.1.0_x64__pzs8sxrjxfjjc` | `Claude for Windows` / `Get started` window rendered | Earlier testing stopped after download and package verification; no missing prerequisite blocked installation |
| ChatGPT | Success, probe exit 0 | `OpenAI.Codex_26.818.5229.0_x64__2p2nqsd0c76g0` | ChatGPT login window rendered | Earlier testing only proved the Microsoft installer contract; the real 726.5 MB Microsoft install had not been allowed to finish |
| Hermes | Success, probe exit 0 | fixed local identity `com.nousresearch.hermes`, version `0.20.5`, x64 | Provider setup, main session, Capabilities, Messaging, Artifacts, and Scheduled jobs pages rendered; Python backend `/api/health` returned 200 | Earlier run was cancelled during severe paging at roughly 1 GiB; the 5 GiB run completed all 16 stages |

Account-bound validation remains pending for actual model responses and third-party messaging delivery because no model-provider account/API key or messaging credentials were supplied. This does not affect installation, version detection, local backend health, page rendering, or the validated bundled tools.

## Passed checks so far

- `cargo fmt --all -- --check`
- `cargo build --release --locked`
- 98 test cases passed before the one confirmed fixture failure; 9 live/host-specific cases were ignored by their declared gates.
- The generated x64 executable contains Windows version resources; it is intentionally not Authenticode signed and remains a validation artifact.
- The clean Windows 10 VM booted and completed unattended setup. The executable hash matched the host artifact, and the guest had no Node, npm, Python, Git, or WinGet installed before launch.
- Clean Windows 11 25H2 admin and standard-user probes both detected all five products as absent without false failures. Official metadata resolution succeeded for Hermes `0.20.4`, Claude `1.34493.1`, ChatGPT Store ID `9PLM9XGG6VKS`, WorkBuddy `5.3.14.36279234`, and CC Switch `3.20.0`.
- Clean Windows 11 real-package download and verification passed for Hermes (7,946,048 bytes), Claude (247,405,438 bytes), WorkBuddy (415,167,976 bytes), and CC Switch (13,508,608 bytes). Authenticode/AppX/minisign identity checks matched the embedded product contracts.
- CC Switch 3.20.0 completed the full admin path on clean Windows 11: resolve, download, signature verification, silent MSI execution, and post-install version detection.
- CC Switch 3.20.0 also completed the full standard-user path with no preinstalled Node/Python/Git. The MSI installs into that user's local profile.
- With the guest adapter disabled, CC Switch resolution failed clearly in 3.37 seconds. Re-enabling the adapter and retrying completed the same 13,508,608-byte download and verification successfully in 56.64 seconds.
- Hermes download cancellation at approximately 0.4% returned the project's `Cancelled` state after 25 progress updates, left no visible installer or `easy-agent-*` temporary directory, and a subsequent full download/verification retry succeeded.
- Clean Windows 10 22H2 admin and standard-user probes both detected all five products as absent and resolved the same current official versions as Win11, despite having no Node, npm, Python, Git, or WinGet. WebView2 was present.
- Clean Windows 10 standard-user artifact validation passed for CC Switch (13,508,608 bytes), Hermes (7,946,048 bytes), and Claude (247,405,438 bytes). Claude's AppX identity, publisher, version `1.34493.1.0`, architecture, and signature were all readable on build 19045.
- CC Switch 3.20.0 completed the full standard-user install and postcheck path on Windows 10 without WinGet or any developer tools.
- The live ignored ChatGPT web-installer contract test passed, and the current official WorkBuddy bootstrap matched the embedded trust entry.
- WorkBuddy completed the full current-user production path on clean Windows 11: 415,167,976-byte download, identity/signature verification, vendor wizard, `5.3.14` postcheck, firewall acknowledgement, and visible login window.
- Claude completed the full production MSIX path on clean Windows 11 in 85.146 seconds and independently launched to its first-run window.
- ChatGPT completed the Microsoft web-installer path in 251.19 seconds, including the 726.5 MB application download, and independently launched to its login window. The current Windows package identity is `OpenAI.Codex`, family `OpenAI.Codex_2p2nqsd0c76g0`.
- Hermes `0.20.5` completed all 16 vendor stages, easy-agent postcheck, independent desktop relaunch, Python CLI help, root/TUI npm validation, ripgrep/ffmpeg checks, and Computer Use driver startup. Its Python backend listened on loopback and returned HTTP 200 from `/api/health`, `/docs`, and `/openapi.json`.

## Evidence artifacts

- Main report: `G:\QIMIStudio\01_code_yunbeifen\01_code\easy-agent\evidence\multi-environment-test-2026-08-21.md`
- Test-only probe worktree: `G:\QIMIStudio\easy-agent-test-lab-20260821\wgpu-probe`
- Hermes initial install prompt: `G:\QIMIStudio\easy-agent-test-lab-20260821\hermes-screen-win11-4.png`
- Hermes dependency preparation: `G:\QIMIStudio\easy-agent-test-lab-20260821\hermes-screen-win11-5.png`
- Hermes live output showing portable Node, WinGet, SSH fallback, and dependency stages: `G:\QIMIStudio\easy-agent-test-lab-20260821\hermes-screen-win11-8.png`
- Hermes Python/Node dependency progress: `G:\QIMIStudio\easy-agent-test-lab-20260821\hermes-screen-win11-10.png`
- WorkBuddy successful production run: `G:\QIMIStudio\easy-agent-test-lab-20260821\logs\workbuddy-win11-success-20260823`
- Claude successful production run: `G:\QIMIStudio\easy-agent-test-lab-20260821\logs\claude-win11-success-20260823`
- ChatGPT successful production run: `G:\QIMIStudio\easy-agent-test-lab-20260821\logs\chatgpt-win11-success-20260823`
- Hermes successful production run and component/runtime validation: `G:\QIMIStudio\easy-agent-test-lab-20260821\logs\hermes-win11-success-20260823`

## Host limitation

The two pre-existing VMs remained running and were not modified. The new Win10 and Win11 test VMs were run serially. The low-memory Hermes path remains valid evidence, and a separate 5 GiB completion run now proves that the same clean VM can complete the vendor install successfully when sufficient memory is available. Windows ARM64 and macOS still require corresponding hardware.
