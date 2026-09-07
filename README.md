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
  <a href="#下载与安装">下载与安装</a> ·
  <a href="#工作方式">工作方式</a> ·
  <a href="#平台状态">平台状态</a> ·
  <a href="#安全边界">安全边界</a> ·
  <a href="docs/installation.md">完整安装指南</a> ·
  <a href="CONTRIBUTING.md">参与贡献</a>
</p>

> [!IMPORTANT]
> `easy agent` 目前是测试预览版，不是已签名的正式发行版。Windows EXE 尚未做 Authenticode 签名，macOS DMG 尚未经过 Apple 公证。请勿关闭 Gatekeeper、删除 quarantine 或忽略系统签名警告来绕过验证。

## 下载与安装

当前版本为 [`v0.1.0-preview.4`](https://github.com/wangduoyu414-cell/easy-agent/releases/tag/v0.1.0-preview.4)：

| 系统 | 下载 | 已验证范围与限制 |
| --- | --- | --- |
| Windows x64 | [下载 EXE](https://github.com/wangduoyu414-cell/easy-agent/releases/download/v0.1.0-preview.4/easy-agent-windows-x64.exe) | 干净 Windows 11 虚拟机中五款客户端的真实首次安装、复检和独立启动均已通过；Claude 当前用户更新修复已完成真实 Windows 复检，EXE 未签名。 |
| Windows ARM64 | [下载 EXE](https://github.com/wangduoyu414-cell/easy-agent/releases/download/v0.1.0-preview.4/easy-agent-windows-arm64.exe) | 当前只启用 Claude 与 ChatGPT；构建和 PE 资源检查通过，仍需 ARM64 真机验收，EXE 未签名。 |
| macOS Intel / Apple Silicon | [下载验证 DMG](https://github.com/wangduoyu414-cell/easy-agent/releases/download/v0.1.0-preview.4/easy-agent-macos-universal-UNNOTARIZED-VALIDATION.dmg) | Universal 验证包兼容 Intel/Apple Silicon；M1 原生构建与安装助手运行已验证，未公证，仅用于受控验证。 |
| 完整性校验 | [下载 SHA-256](https://github.com/wangduoyu414-cell/easy-agent/releases/download/v0.1.0-preview.4/SHA256SUMS.txt) | 运行前核对文件摘要。 |

完整步骤、环境要求和排错见 [安装指南](docs/installation.md)。所有版本见 [GitHub Releases](https://github.com/wangduoyu414-cell/easy-agent/releases)。

> [!NOTE]
> 主分支已新增 ChatGPT Windows 最新版本识别：读取并校验 OpenAI 固定 MSIX 的版本、产品身份、架构和大小元数据；本机已是最新版时不再显示可点击的更新按钮。该修复晚于 `v0.1.0-preview.4` 制品，需后续 Release 重新构建后才会进入公开 EXE。

## 项目简介

`easy agent` 只管理 WorkBuddy、Hermes Agent、CC Switch、Claude Desktop 和 ChatGPT。它不是通用软件管家，也不执行网页返回的脚本；官方入口、包类型、签名主体、Bundle/Package 身份和架构规则均固定在应用内，证据不足时停止安装。

| 能力 | 说明 |
| --- | --- |
| 本机状态识别 | 检测平台、安装状态和已安装版本，并分别显示安装、更新或不可用原因。 |
| 可信下载与验证 | 官方入口优先；产品专用回退只有在明确网络或地区不可用时才允许，并继续验证签名、摘要、版本和身份。 |
| 安全执行 | 私有暂存、执行前二次绑定、结构化安装命令、安装后身份/架构/版本复检。 |
| 多任务与可解释失败 | 产品任务相互隔离；下载、取消、验证失败、结果未知等状态不会被伪装成成功。 |

## 工作方式

```text
检测平台与现有安装
        ↓
解析内置可信分发合同
        ↓
私有暂存下载 + 受控重定向
        ↓
摘要 / updater 签名 / 平台签名 / 身份 / 架构验证
        ↓
执行前二次绑定
        ↓
Windows：结构化安装命令     macOS：只读挂载或安全展开 → 原子替换 .app
        ↓
安装后精确复检
```

## 平台状态

| 平台 | 当前启用范围 | 当前结论 |
| --- | --- | --- |
| Windows x64 | WorkBuddy、Hermes、CC Switch、Claude、ChatGPT | 干净 Windows 11 中五款真实首次安装、复检和启动通过；Windows 10 中 CC Switch 真实安装通过。更新、账号业务和少数故障场景尚未全部覆盖。 |
| Windows ARM64 | Claude、ChatGPT | 单文件 EXE、PE 架构、图标和版本资源已验证；WorkBuddy、Hermes、CC Switch 仍禁用，整机使用需 ARM64 真机验证。 |
| macOS Intel | WorkBuddy、CC Switch、Claude、ChatGPT；Hermes 不支持 | 四款直接应用包的下载、Apple 身份、临时安装、更新和回滚链已验证；验证 DMG 未公证。 |
| macOS Apple Silicon | WorkBuddy、CC Switch、Claude、ChatGPT；Hermes bootstrap 禁用 | ARM64/Universal 包身份和架构链已验证；仍需 Apple Silicon 真机启动与使用验收。 |

已知使用限制：

- 2026-09-06 已修复 WorkBuddy 两种 Mac 架构的 Bundle ID 迁移，并验证当前官方包的临时安装与回滚。Hermes Mac 安装器默认追踪 main，缺少固定稳定版本与最终应用复检，继续禁用。Windows 当前包检查与关联影响见 [安装来源审计](evidence/distribution-source-audit-2026-09-06.md)。
- Hermes 官方正式版固定源码已在 M1 上构建并完成真实桌面/后端联调；该自建测试样本不替代官方签名安装器的验证，见[真实运行证据](evidence/hermes-fixed-source-runtime-2026-09-06.md)。
- 图形界面需要 OpenGL 2.0 或更高版本；缺少可用图形驱动的旧电脑或受限虚拟机可能无法启动。
- ChatGPT Windows 会从固定 OpenAI MSIX 的受校验响应元数据读取最新版本；本机版本相同或更高时显示“已是最新版本”并禁用更新按钮，网络暂时无法确认时才显示“检查更新”。
- Hermes Windows 安装器会自动准备 Node、Python、Git 等依赖，无需预装；干净环境建议约 5 GB 可用内存、数 GB 磁盘空间，并预留约 30 分钟。

详细证据和未关闭项目见 [实现与验证状态](docs/implementation-status.md) 与 [多环境测试报告](evidence/multi-environment-test-2026-08-21.md)。

## 安全边界

- 只管理五款固定客户端；远端响应不能新增主机、包类型、签名主体或产品身份。
- 不执行服务器返回的 PowerShell、Shell 或安装参数；平台命令由本地编译代码构造。
- 下载在私有临时目录完成并限制重定向、文件名和大小；安装始终使用已绑定的私有副本。
- Windows 验证 Authenticode、AppX/MSIX 身份和 PE 架构；macOS 验证 Bundle ID、Developer Team ID、Mach-O 架构、codesign 和 Gatekeeper。
- 任一摘要、签名、身份、架构、版本合同或最终复检缺失时默认停止。

## 支持的客户端

| 客户端 | Windows | macOS |
| --- | --- | --- |
| WorkBuddy | x64 官方更新接口、腾讯签名和最终 EXE 复检 | Intel/Apple Silicon 官方 ZIP 与 Apple 身份复检 |
| Hermes Agent | x64 官方 bootstrap，桌面与 runtime 状态分离 | Intel 不支持；Apple Silicon bootstrap 已识别但当前禁用 |
| CC Switch | x64 官方签名更新清单、minisign 和 MSI | Intel/Apple Silicon 签名归档、minisign 和 `.app` 复检 |
| Claude Desktop | x64/ARM64 官方完整 MSIX；明确不可用时使用同版本签名回退 | Intel/Apple Silicon Universal DMG；明确不可用时使用同版本签名回退 |
| ChatGPT | x64/ARM64 固定微软安装器；明确分发失败时使用官方完整 MSIX 与离线许可证 | Intel/Apple Silicon 官方 Sparkle appcast 与 ZIP；明确不可用时使用签名回退 |

“支持”表示对应安全合同已启用，不代表正式发布签名、真机或账号业务测试已经全部完成。

## 开发与验证

```bash
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

本地运行、Windows/macOS 构建和制品校验命令见 [安装指南](docs/installation.md)。品牌资源说明见 [assets/branding](assets/branding/README.md)。

## 文档

- [安装指南](docs/installation.md)：下载、环境要求、构建、校验和排错。
- [实现与验证状态](docs/implementation-status.md)：当前支持矩阵、真实测试结果和剩余 Gate。
- [维护手册](docs/maintenance.md)：信任根、官方来源、回退服务和发布维护规则。
- [GitHub 首页设计记录](docs/github-homepage-design.md)：主页结构的调研与取舍。
- [参与贡献](CONTRIBUTING.md)：测试要求、文档规范与安全变更流程。

## 许可

本项目使用 [MIT License](LICENSE)。第三方客户端及其商标、安装包和服务条款归各自权利人所有；仓库不提交第三方安装包。
