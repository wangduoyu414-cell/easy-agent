# 安装 easy agent

`easy agent` 是桌面应用的安全安装助手，不是远程脚本下载器。安装或构建前，请先阅读 [README](../README.md) 中的当前验证状态；当前并没有可宣称为生产级的已签名用户发行版。

## 选择正确的安装路径

| 目标 | 推荐路径 | 何时使用 |
| --- | --- | --- |
| 日常使用 | 将来的 GitHub Release 已签名制品 | 仅当 Release 说明同时给出版本、SHA-256 和签名/公证状态时。 |
| 评估当前代码 | `cargo run --release` | 开发者在受控本机直接运行。 |
| 验证 Windows 包 | `packaging/build-windows.ps1` | 需要一个带 Windows 资源图标的便携 EXE。 |
| 验证 macOS Universal 包 | `packaging/build-macos.sh` | 需要 Intel + Apple Silicon Universal `.app` / DMG。 |

不要使用未知镜像、第三方“加速下载”、`curl | sh`、关闭 Gatekeeper 或删除 quarantine。应用界面只显示版本与状态，实际入口、下载节点、镜像时效和签名身份由内置策略固定校验并写入脱敏日志，不需要用户自行判断渠道。安全验证失败时，停止并检查证据，而不是寻找绕过方式。

Claude Windows 当前直接使用 Anthropic 官方完整 MSIX。`easy agent` 会先完成摘要、AppX 签名、Publisher、Package Identity、架构和版本验证，再请求管理员权限执行本地机器级部署；不会启动 Claude Setup，也不会在安装阶段再次下载约 253 MB 的组件。部署结束后仍复检固定 Package 身份、架构和版本，不会只凭退出码报告成功。Cowork 可能还要求启用 Windows 虚拟机平台并重启。Claude 登录和实际使用仍取决于本机网络与 Anthropic 的服务地区要求。

可以连续操作多个客户端：不同厂商的 EXE 安装器和 macOS 应用复制可以直接同时进行；只有两个任务同时使用同一种 Windows 系统安装引擎时才短暂排队（MSI 只等 MSI，MSIX/Store 只等 MSIX/Store）。MSI 与 Store 互不阻塞。每个产品都可以单独取消，某一项失败不会中断其他项。

Windows 刷新状态时，本机安装检测和联网获取最新版本是两个独立阶段。全新电脑正常会先显示“未安装 · 正在获取最新版本”，随后变为“可安装”；官网较慢不会再伪装成本机仍在检测。若系统 PowerShell、AppX 或卸载注册表确实不可读，界面会明确显示“本机安装状态检测失败”并禁用安装，刷新后仍失败时应检查 Windows AppX 服务或终端安全策略，而不是把它当成未安装继续覆盖。

直接下载安装的 EXE、MSI、MSIX、DMG、ZIP 和 tar.gz 在验证通过后都会额外保存一份到当前用户的系统“下载”目录，文件名包含产品和版本；安装本身仍使用防篡改的私有暂存副本。若下载目录已有不同内容的同名文件，easy agent 会添加 SHA-256 摘要后缀，不会覆盖原文件。Windows ChatGPT 默认下载并启动微软轻量安装器；网络/分发服务不可用或返回 `1612/0x64C` 安装源缺失时，自动下载 OpenAI 完整 MSIX 和离线许可证并请求管理员部署，临时文件不作为用户安装包长期保存。

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

在 macOS 上交叉构建同一组验证 EXE：

```bash
brew install llvm
cargo install cargo-xwin --locked
./packaging/build-windows-from-macos.sh all
```

该脚本使用 MSVC ABI/Windows SDK 导入库，并分别生成 x64 与 ARM64 单文件 EXE；最终仍需在对应 Windows 真机上运行和做 Authenticode 签名。

## 构建 macOS Universal DMG

### 内部/CI 验证包

```bash
ALLOW_UNSIGNED_MACOS_BUILD=1 ./packaging/build-macos.sh
```

脚本会构建 `x86_64-apple-darwin` 与 `aarch64-apple-darwin`、合并为 Universal 可执行文件、复制 `easy agent.icns`、执行 ad-hoc codesign，并在 DMG 中同时放入 `easy agent.app` 与指向系统 Applications 的拖拽入口，然后输出：

```text
dist/easy agent.app
dist/easy-agent-macos-universal-UNNOTARIZED-VALIDATION.dmg
dist/SHA256SUMS-macos-universal-UNNOTARIZED-VALIDATION.txt
```

ad-hoc 产物没有 Developer ID 与 notarization，Gatekeeper 拒绝是正确结果；文件名强制带有 `UNNOTARIZED-VALIDATION`，请不要改名或将其作为面向终端用户的下载包。只有 Developer ID 签名、公证、staple 和 Gatekeeper 检查全部通过的构建才能生成正式文件名 `easy-agent-macos-universal.dmg`。

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

- Claude 固定回退、双架构真实包下载、Apple 身份校验和 Intel 临时目录安装/更新/失败回滚已接通；Apple Silicon 原生启动仍需对应真机验收；
- Hermes Intel 由厂商明确不支持；Apple Silicon DMG 是尚未建模最终桌面/runtime 状态的 vendor bootstrap；
- 生产级 `easy agent` DMG 仍需 Developer ID 与 notarization 凭据。

WorkBuddy、CC Switch、Claude 与 ChatGPT 的 Intel/Apple Silicon 直接应用包条目已经启用。WorkBuddy 官方摘要已确认错误，因此仅该产品的 macOS 条目改用 Apple 平台签名与固定应用身份；Claude 和 ChatGPT 的固定回退仍要求签名清单、摘要、版本和平台身份全部通过。它们会在目标应用运行、安装目录不可写、签名/身份/版本变化时失败关闭。请参考 [macOS 功能链路审计](../evidence/macos-functional-parity-audit-2026-08-08.md)、[实现与验证状态](implementation-status.md) 和 [维护手册](maintenance.md)。
