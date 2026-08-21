<p align="center">
  <img src="assets/branding/easy-agent-icon-512.png" width="132" alt="easy agent application icon" />
</p>

<h1 align="center">easy agent</h1>

<p align="center">
  面向五款固定 AI 桌面客户端的安全安装助手<br />
  <sub>A fail-closed installer assistant for five AI desktop clients.</sub>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-5f5f5f.svg" alt="MIT license" /></a>
  <img src="https://img.shields.io/badge/language-Rust-dea584.svg" alt="Rust" />
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS-3d7eff.svg" alt="Windows and macOS" />
  <img src="https://img.shields.io/badge/status-validation--gated-f0a63b.svg" alt="Validation gated" />
</p>

<p align="center">
  <a href="#安装">安装</a> ·
  <a href="#它如何工作">工作方式</a> ·
  <a href="#平台与交付状态">平台状态</a> ·
  <a href="#安全边界">安全边界</a> ·
  <a href="docs/installation.md">完整安装指南</a> ·
  <a href="CONTRIBUTING.md">参与贡献</a>
</p>

> [!IMPORTANT]
> `easy agent` 目前处于安全验证阶段，而非已签名的终端用户发行版。它不会通过跳过校验、绕过 Gatekeeper 或猜测下载地址来假装“可安装”。请只从 [GitHub Releases](https://github.com/wangduoyu414-cell/easy-agent/releases) 下载将来发布的已签名制品；当前可用的是下方列出的本地构建和验证路径。

## 30 秒了解

`easy agent` 管理固定的五款客户端：WorkBuddy、Hermes Agent、CC Switch、Claude Desktop 和 ChatGPT。它不是软件管家，也不执行来自网页的脚本；它把官方入口、少数产品专用的受验证网络回退边界、包类型、签名主体、Bundle/Package 身份和架构规则编译进应用，以“证据不足即停止”为原则执行安装。

| 你会得到什么 | 它如何做到 |
| --- | --- |
| 一个简洁的原生桌面界面 | 识别本机平台和已安装版本，逐项显示安装/更新状态。 |
| 多个客户端可以同时处理 | 每个产品独立下载、校验、取消和显示错误；真正写入系统及安装后复检按先后排队，避免多个系统安装器互相干扰。 |
| 只使用固定可信来源 | 默认先访问固定官方入口；只有明确的网络/地区可用性失败才能进入产品专用回退。回退清单签名、时效、大小、摘要和厂商平台签名必须全部通过，远端响应不能扩展信任边界。 |
| Windows 与 macOS 的同一安全编排 | 私有暂存下载、摘要/签名/身份/架构验证、验证后保存可见副本到系统“下载”目录、仍从私有副本执行、安装后版本复检。 |
| 可解释的失败 | 不支持、验证待完成、取消、下载失败和“结果未知”被明确区分，不伪造成功。 |

## 安装

详细步骤、签名校验和排错见 [完整安装指南](docs/installation.md)。选择适合你的路径：

| 场景 | Windows | macOS |
| --- | --- | --- |
| 使用正式发行版 | 正式签名 EXE 发布后，从 [Releases](https://github.com/wangduoyu414-cell/easy-agent/releases) 下载 `easy-agent-windows-*.exe`。 | Developer ID 签名并公证的 DMG 发布后，从 Releases 下载 `easy-agent-macos-universal.dmg`。 |
| 本地试运行 | 安装 Rust 后运行 `cargo run --release`。 | 安装 Xcode Command Line Tools 与 Rust 后运行 `cargo run --release`。 |
| 构建可携带验证包 | `./packaging/build-windows.ps1 -Architecture x64` | `ALLOW_UNSIGNED_MACOS_BUILD=1 ./packaging/build-macos.sh` |
| 验证产物完整性 | `Get-FileHash -Algorithm SHA256 .\dist\easy-agent-windows-x64.exe` | 正式公证包：`shasum -a 256 dist/easy-agent-macos-universal.dmg`；内部验证包文件名必须带 `UNNOTARIZED-VALIDATION` |

当前没有可宣称为生产级的终端用户安装包。未公证的 macOS 验证 DMG 会被 Gatekeeper 拒绝，这是预期安全行为；请不要要求用户关闭 Gatekeeper 或移除 quarantine 来绕过它。

## 它如何工作

```text
检测平台与现有安装
        ↓
解析内置的可信分发合同（官方优先）
        ↓
私有暂存下载 + 受控重定向
        ↓
摘要 / updater 签名 / 平台签名 / 身份 / 架构验证
        ↓
执行前二次绑定
        ↓
Windows：结构化安装命令     macOS：只读挂载或安全展开 → 原子替换 .app
        ↓
安装后精确身份、架构和版本复检
```

在 macOS 上，应用自身是一个包含 Intel 和 Apple Silicon slice 的 Universal `.app`；下游包按照实际硬件而非当前进程 slice 选择。DMG 只读挂载，ZIP/tar.gz 在展开前后检查路径穿越、链接逃逸、重复路径和大小上限；新应用先在目标卷的私有暂存目录复验，再原子替换，最终复验失败会恢复旧版本。

## 平台与交付状态

| 平台 | easy agent 应用 | 厂商客户端操作 | 当前结论 |
| --- | --- | --- | --- |
| Windows x64 | 原生单文件 EXE 构建链已具备 | 五款产品的受控解析/验证/执行链已实现 | 干净机首次安装与更新矩阵仍待关闭。 |
| Windows ARM64 | 原生单文件 EXE 已可交叉构建 | Claude 等提供 ARM64 包的产品按各自合同启用 | PE/图标/版本资源已验证；仍需 Windows ARM64 真机启动与安装矩阵。 |
| macOS Intel | Universal 应用、检测、验证、原子复制/回滚已实现 | WorkBuddy、CC Switch、Claude、ChatGPT 已启用；Hermes 不支持 | 四款真实 x64 包已完成下载与 Apple 身份校验；正式 DMG 签名公证仍待发布凭据。 |
| macOS Apple Silicon | 同一 Universal 应用原生运行 | WorkBuddy、CC Switch、Claude、ChatGPT 已启用；Hermes bootstrap 禁用 | 四款真实 ARM64/Universal 包已完成下载与 Apple 身份校验；仍需 Apple Silicon 真机启动验收。 |

macOS 按产品独立处理：WorkBuddy 的官方 API SHA-256 已确认错误，因此只在 WorkBuddy/macOS 上改用 Apple 平台签名、固定 Team/Bundle/版本/架构和稳定文件绑定；其他产品仍严格执行自己的摘要或 updater 签名。ChatGPT 固定官方 appcast 与 ZIP 直连优先，只有明确网络可用性失败时才使用签名副本，并继续强制 OpenAI Sparkle 与 Apple 双重签名。Claude 公开稳定重定向在当前地区会被拦截，因此 Intel 与 Apple Silicon 都可回退到固定香港清单；客户端仍必须验证 Claude Bundle/Team、版本、目标 slice、codesign 与 Gatekeeper。Hermes Apple Silicon DMG 仍是尚未建模最终桌面/runtime 状态的 vendor bootstrap。

## 安全边界

- 只管理五款固定客户端；受验证回退仅限固定的 Claude Windows/macOS 四个平台条目与 ChatGPT macOS 条目，不做通用镜像、远程规则平台或可由服务器扩展的客户端规则服务。
- 不执行服务器返回的 PowerShell、Shell 或安装参数；平台命令均由本地编译代码构造。
- 下载在私有临时目录进行，限制重定向、文件名和大小；验证通过后把可见副本保存到 Windows/macOS 的系统“下载”目录，但安装仍从绑定的私有副本执行，避免公开目录文件被替换。目标文件同名且内容不同时会使用摘要后缀，绝不覆盖用户已有文件。完整 URL、用户目录和临时目录会从操作日志中脱敏。ChatGPT Windows 的动态微软安装器和完整离线部署包只在私有暂存中执行，不作为长期下载副本保存。
- Windows 使用固定系统工具、Authenticode/AppX 身份和架构检查；ChatGPT 每次下载并验证绑定固定 Store Product ID 的微软轻量安装器，网络、Windows Update、微软分发服务不可用或 `1612/0x64C` 安装源缺失时自动进入官方完整 MSIX 与离线许可证兜底，不探测或修复 WinGet/App Installer。
- macOS 固定 Applications 中的应用名、Bundle ID、Developer Team ID、主 Mach-O slice、codesign 和 Gatekeeper 结果；CC Switch 验证 minisign，ChatGPT 验证 Sparkle Ed25519；不清除 quarantine。
- 任一验证缺失、身份变化或版本合同变化时，默认停止并给出原因。

## 支持的客户端

| 客户端 | Windows 分发策略 | macOS 分发策略 |
| --- | --- | --- |
| WorkBuddy | 官方更新接口 + Authenticode / 最终 EXE 复检 | 官方架构 ZIP + Apple codesign/Gatekeeper + Bundle/Team/版本/架构复检 |
| Hermes Agent | 官方 bootstrap，桌面与 runtime 状态分离 | Apple Silicon DMG bootstrap；Intel 明确不支持 |
| CC Switch | 官方 `latest.json` + minisign + MSI | 官方签名 `tar.gz` + minisign + `.app` 复检 |
| Claude Desktop | Anthropic 官方完整 MSIX（x64/ARM64）直连优先；明确可用性失败时使用香港签名清单中的同版本 MSIX；管理员方式机器级部署，安装阶段完全本地执行并复检 Claude Package 身份与版本 | 官方 Universal DMG 优先；Intel/Apple Silicon 在明确可用性失败时使用各自签名清单并继续验证同一 Universal 应用 |
| ChatGPT | 固定微软轻量安装器；明确的微软网络/分发失败时使用对应架构官方 MSIX 与离线许可证；安装后复检 OpenAI Package Identity/Family/Publisher/架构 | OpenAI 官方 Intel / Apple Silicon appcast 与 ZIP 直连优先；元数据或完整包网络不可达时使用固定签名清单回退；始终验证 Sparkle Ed25519 与 Apple 身份 |

“支持”描述的是已编码的安全合同，不代表某个平台已绕过所有发布验证 Gate。点击操作仅在对应信任条目和平台证据都闭合后才会变为可用。

Windows 上的 Claude 直接使用 Anthropic 官方部署文档公开的完整 MSIX。`easy agent` 先下载并验证 AppX 签名、Publisher、Identity、架构、版本和稳定文件身份，再通过本地固定的管理员部署命令写入系统；不会启动 Claude Setup，也不会在安装阶段再次下载约 253 MB 的组件。Cowork 仍可能要求启用 Windows 虚拟机平台并重启。此链路只解决安装包获取和本地安装，不代理 Claude 登录或运行流量；中国大陆也不在 Anthropic 当前官方支持地区列表中。

## 开发与验证

```bash
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --target x86_64-apple-darwin
cargo check --all-targets --target aarch64-apple-darwin
```

在 macOS 上复现 Windows 双架构验证构建需先安装 `cargo-xwin` 与 Homebrew LLVM，然后运行 `./packaging/build-windows-from-macos.sh all`。

品牌资源包含原始 PNG、运行时 PNG、Windows `.ico` 和 macOS `.icns`。`cargo test --test branding_contract` 会检查窗口、Windows 资源脚本与 macOS Bundle 的名称/图标一致性。图标来源和维护说明见 [assets/branding](assets/branding/README.md)。

## 文档、贡献与传播

- [完整安装指南](docs/installation.md)：正式发行版、本地构建、验证 DMG 和校验方法。
- [GitHub 首页设计记录](docs/github-homepage-design.md)：对 Ollama、Tauri、LocalSend、RustDesk 的主页模式调研与本仓库取舍。
- [实现与验证状态](docs/implementation-status.md)：已完成能力、可复现实证与未关闭 Gate。
- [macOS 功能链路审计](evidence/macos-functional-parity-audit-2026-08-08.md)：Windows/macOS 阶段对照、双架构完整包、激活/回滚和当前支持矩阵。
- [Claude 接入与镜像交叉审计](evidence/claude-integration-audit-2026-08-12.md)：官方分发合同、当前私有部署、客户端落点、Cowork 边界和公网硬门。
- [easy agent macOS 品牌构建证据](evidence/easy-agent-branding-macos-proof-2026-08-04.md)：新图标、Universal DMG、签名、挂载和 Intel 启动验证。
- [维护手册](docs/maintenance.md)：更新信任根、官方来源和平台证据时必须遵守的规则。
- [参与贡献](CONTRIBUTING.md)：测试要求、文档规范与安全变更流程。

欢迎提交可复现的问题、平台验证证据和文档改进。涉及下载源、签名、Bundle/Package 身份、架构或安装行为的变更必须附带官方依据与可复核证据；请勿在 Issue 中粘贴令牌、账号信息或完整临时下载 URL。

## 许可

本项目使用 [MIT License](LICENSE)。第三方客户端及其商标、安装包和服务条款均归各自权利人所有。仓库不提交第三方安装包；部署产品专用受验证回退服务时，运营者仍需自行确认对应的软件分发与服务条款。
