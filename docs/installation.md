# 安装 easy agent

`easy agent` 是桌面应用的安全安装助手，不是远程脚本下载器。安装或构建前，请先阅读 [README](../README.md) 中的当前验证状态；当前并没有可宣称为生产级的已签名用户发行版。

## 选择正确的安装路径

| 目标 | 推荐路径 | 何时使用 |
| --- | --- | --- |
| 日常使用 | 将来的 GitHub Release 已签名制品 | 仅当 Release 说明同时给出版本、SHA-256 和签名/公证状态时。 |
| 评估当前代码 | `cargo run --release` | 开发者在受控本机直接运行。 |
| 验证 Windows 包 | `packaging/build-windows.ps1` | 需要一个带 Windows 资源图标的便携 EXE。 |
| 验证 macOS Universal 包 | `packaging/build-macos.sh` | 需要 Intel + Apple Silicon Universal `.app` / DMG。 |

不要使用未知镜像、第三方“加速下载”、`curl | sh`、关闭 Gatekeeper 或删除 quarantine。安全验证失败时，停止并检查证据，而不是寻找绕过方式。

## 从将来的 GitHub Release 安装

正式发布时，请只在 [Releases](https://github.com/wangduoyu414-cell/easy-agent/releases) 页面选择与系统匹配的文件：

| 系统 | 预期文件 | 安装方式 |
| --- | --- | --- |
| Windows x64 | `easy-agent-windows-x64.exe` | 下载后核对 SHA-256，直接运行。 |
| Windows ARM64 | `easy-agent-windows-arm64.exe` | 下载后核对 SHA-256，直接运行。 |
| macOS Intel / Apple Silicon | `easy-agent-macos-universal.dmg` | 下载后核对 SHA-256，挂载并将 `easy agent.app` 移到 Applications。 |

Windows PowerShell 校验示例：

```powershell
Get-FileHash -Algorithm SHA256 .\easy-agent-windows-x64.exe
```

macOS 校验示例：

```bash
shasum -a 256 easy-agent-macos-universal.dmg
codesign --verify --deep --strict --verbose=2 "/Applications/easy agent.app"
spctl --assess --type execute --verbose=2 "/Applications/easy agent.app"
```

只有 Release 页面明确声明签名和公证均已完成时，才应期望 Gatekeeper 通过。不要把开发验证 DMG 当成正式发布物。

## 本地运行（Windows 和 macOS）

前提：Rust stable；Windows 还需要 Visual Studio C++ Build Tools，macOS 还需要 Xcode Command Line Tools。

```bash
git clone https://github.com/wangduoyu414-cell/easy-agent.git
cd easy-agent
cargo run --release
```

该方式直接启动当前主机架构的开发应用，不创建 `.app` 或便携 EXE。它适合代码审查、界面验证和受控测试。

## 构建 Windows 便携 EXE

在 Windows PowerShell 中执行：

```powershell
.\packaging\build-windows.ps1 -Architecture x64
```

可替换为 `-Architecture arm64` 生成 ARM64 版本。脚本会运行格式、静态检查、测试和 release 构建，然后输出：

```text
dist/easy-agent-windows-x64.exe
dist/SHA256SUMS-windows-x64.txt
dist/SHA256SUMS.txt
dist/release-manifest.json
```

该 EXE 包含 `easy agent` 的 Windows 图标和版本资源，但除非已使用合法 Authenticode 证书签名，否则仍只是验证产物。

## 构建 macOS Universal DMG

### 内部/CI 验证包

```bash
ALLOW_UNSIGNED_MACOS_BUILD=1 ./packaging/build-macos.sh
```

脚本会构建 `x86_64-apple-darwin` 与 `aarch64-apple-darwin`、合并为 Universal 可执行文件、复制 `easy agent.icns`、执行 ad-hoc codesign，并输出：

```text
dist/easy agent.app
dist/easy-agent-macos-universal.dmg
dist/SHA256SUMS-macos-universal.txt
```

ad-hoc 产物没有 Developer ID 与 notarization，Gatekeeper 拒绝是正确结果；请不要将其作为面向终端用户的下载包。

如需避免改写仓库内 `dist/`，可指定临时输出目录：

```bash
EASY_AGENT_DIST_DIR="$(mktemp -d /tmp/easy-agent-build.XXXXXX)" \
ALLOW_UNSIGNED_MACOS_BUILD=1 \
./packaging/build-macos.sh
```

### 正式签名与公证包

只有发布主体提供 Developer ID Application 身份和 `notarytool` keychain profile 后才执行：

```bash
APPLE_SIGN_IDENTITY='Developer ID Application: Your Organization (TEAMID)' \
APPLE_NOTARY_PROFILE='easy-agent-notary' \
./packaging/build-macos.sh
```

脚本会提交公证、staple、验证并通过 Gatekeeper 检查。签名身份、notary profile 和证书私钥均不得写入仓库、Issue、日志或聊天记录。

## 客户端操作为何可能显示“验证待完成”

应用自身可以正常运行不代表五款第三方客户端都已获准安装。`easy agent` 会在信任条目尚未闭合时禁用对应按钮。当前 macOS 的主要 Gate 是：

- Claude 稳定下载端点在本机网络出现 Cloudflare challenge；
- Hermes Intel 由厂商明确不支持；Apple Silicon DMG 是尚未建模最终桌面/runtime 状态的 vendor bootstrap；
- 生产级 `easy agent` DMG 仍需 Developer ID 与 notarization 凭据。

WorkBuddy、CC Switch 与 ChatGPT 的 Intel/Apple Silicon 直接应用包条目已经启用。WorkBuddy 官方摘要已确认错误，因此仅该产品的 macOS 条目改用 Apple 平台签名与固定应用身份；下载文件本身仍会计算实际 SHA-256，并在平台验证前后、进入安装交接前重复核对。它们仍会在目标应用运行、安装目录不可写、签名/身份/版本变化时失败关闭。请参考 [macOS 功能链路审计](../evidence/macos-functional-parity-audit-2026-08-08.md)、[实现与验证状态](implementation-status.md) 和 [维护手册](maintenance.md)。
