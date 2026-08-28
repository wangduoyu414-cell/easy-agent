# ChatGPT Windows deployment recovery — updated 2026-08-12

## Outcome

The former WinGet/App Installer repair path was removed. It coupled ChatGPT installation to repair of shared Windows components and caused failures such as `0x80073D02` when Windows Web Experience or another process held those components open.

The current implementation follows OpenAI's official Windows deployment page:

1. Download the Microsoft web installer for Store product `9PLM9XGG6VKS`.
2. Verify Microsoft Authenticode and its signed `MSStoreTag001` product configuration.
3. Launch the Microsoft installer and postcheck the exact OpenAI package.
4. Only when normal Microsoft distribution is explicitly unavailable, download the stable architecture-specific MSIX plus `ChatGPT-License.xml`, verify both contracts, request UAC, deploy, and postcheck.

## Official endpoints and observed contract

- Web installer: `https://get.microsoft.com/installer/download/9PLM9XGG6VKS?cid=website_cta_psi`
- x64: `https://persistent.oaistatic.com/codex-app-prod/ChatGPT-x64.msix`
- ARM64: `https://persistent.oaistatic.com/codex-app-prod/ChatGPT-arm64.msix`
- Offline license: `https://persistent.oaistatic.com/codex-app-prod/ChatGPT-License.xml`
- Identity: `OpenAI.Codex`
- Family: `OpenAI.Codex_2p2nqsd0c76g0`
- Publisher: `CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B`
- Observed package version on 2026-08-12: `26.803.10989.0`
- Minimum OS: Windows 10 `10.0.19041.0`
- No framework dependencies were declared in the observed MSIX manifests.
- License: Product ID `9PLM9XGG6VKS`, PFM `openai.codex_2p2nqsd0c76g0`, `Full`, `Offline`, `LeaseRequired=False`.
- Web-installer signed configuration: Product ID `9PLM9XGG6VKS`, PFN `OpenAI.Codex_2p2nqsd0c76g0`, installer type `WindowsUpdate`, `isHarbor=true`, `autoUpdate=false`.

## Failure boundary

Fallback is allowed only for narrowly classified network, Windows Update, or Microsoft distribution-service unavailability. It is not allowed for user cancellation, rejected UAC, managed installation, policy/security rejection, signature or product-binding mismatch, wrong identity/architecture, or an invalid license.

Timeout after system deployment begins is `ResultUnknown`; easy agent does not kill Windows deployment and asks the user to refresh status later. Exit code zero is never sufficient: the final installed package identity, family, publisher, architecture, and version must all match.

## Validation status

- Official documentation and all four endpoints were rechecked on 2026-08-12.
- Both architecture packages and the offline license were inspected.
- Unit, trust-boundary, cross-compilation, and package builds are recorded by the current build run.
- Disposable Windows x64/ARM64 clean-machine installation and update remain the final real-device acceptance gate.
