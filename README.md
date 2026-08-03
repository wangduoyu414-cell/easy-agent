# AI 客户端安装助手

一个固定管理五款 AI 桌面客户端的轻量本地安装助手：WorkBuddy、HermesAgent、CC Switch、Claude Desktop（含 Code 页）和 ChatGPT。

当前实现已经具备 Windows x64 原生界面、本机安装检测、在线官方版本解析、嵌入式信任注册表、受控重定向、私有临时下载、摘要/签名验证、执行前制品二次绑定和结构化安装命令。五款产品都使用固定官方入口解析直接安装包；ChatGPT Windows 读取 OpenAI 官方更新清单并部署完整 MSIX。macOS 共用同一套产品模型，由一个 Universal 应用同时包含 Intel 与 Apple Silicon slice，运行时再按真实硬件选择各厂商包。

## 当前交付状态

- Windows x64：可构建单文件便携 EXE；默认窗口为紧凑的 `800×610`，五个产品无需滚动即可完整显示，每项只有一个“安装/更新”主动作。确认步骤使用页内模态卡片，不再打开独立小窗口；检测、校验、PowerShell 和安装程序等后台子进程统一以无控制台窗口方式启动，不再闪现黑色命令行窗口。ChatGPT 的同一按钮会解析 OpenAI 官方清单、下载完整 MSIX、校验 AppX 签名与固定身份、关闭占用中的目标应用并执行本地部署，最终以 Package Identity/Family/Publisher/架构/版本复检为准。
- WorkBuddy Windows 检测接受官方注册项的纯数字版本后缀（例如 `WorkBuddy 5.1.7`），并同时固定腾讯 Publisher。官方 x64 通道当前使用 x86 NSIS 安装引导器；该机器码只在 WorkBuddy x64 的本地信任条目中显式允许，其他 EXE 不受影响。厂商注册表只登记三段版本（如 `5.3.8`），官网接口还带内部构建号（如 `5.3.8.34705286`），程序按前三段判断同一发行版。安装器退出后还必须定位 `WorkBuddy.exe`、确认其为 x64 且版本达到目标。
- 每次安装批次自动写入脱敏操作日志 `%LOCALAPPDATA%\AI Client Installer\logs\operations.jsonl`；只记录状态变化和最终错误，重复下载进度不会持续写盘，日志达到 1 MiB 后保留一份上一轮文件并轮换。
- Windows ARM64：Rust target 已安装并实际尝试 release 构建；当前主机缺 Visual Studio C++ ARM64/clang-cl 组件，且无 ARM64 真机，暂不作为已交付制品。
- macOS Universal：已在 GitHub 的 Intel macOS Runner 实际生成一个同时包含 `x86_64` 与 `arm64` slice 的 ad-hoc 签名验证 DMG，位于 `dist/AI-Client-Installer-macos-universal.dmg`。应用/DMG codesign 校验和本地 SHA-256 复核通过；未使用 Apple Developer ID、未公证，因此不是无 Gatekeeper 警告的正式发布包。安装链已实现硬件/系统检测、Applications 精确检测、DMG/ZIP/tar.gz 安全展开、身份/架构验证和失败回滚。当前所有可安装 Mac 厂商条目仍保持 `disabled`，等待实包 Team ID 固定和两类 Mac 真机安装复检；Hermes Intel 明确为 `unsupported`。
- 真机 Gate：本轮没有执行 WorkBuddy 或 ChatGPT 的真实安装/更新。WorkBuddy 单动作更新以及 ChatGPT Windows x64/ARM64 的完整 MSIX 首次安装、旧版更新和“无 Store/WinGet/引导器/登录窗口”监控仍需可丢弃快照验证；正式状态为 `Implementation complete; validation pending`，不是最终签名发行版。

详见 [实现与验证状态](docs/implementation-status.md)、[V1 执行任务卡](tasks/AI-CLIENT-INSTALLER-V1.md)、[ChatGPT 直装任务卡](tasks/AI-CLIENT-INSTALLER-CHATGPT-DIRECT.md) 和 [官方分发调研](research/official-distribution-and-installation-research-2026-07-31.md)。

## 本地开发

```powershell
cargo check --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo test --test resolver_fixtures
cargo test --test security_boundaries
cargo check --all-targets --target x86_64-apple-darwin
cargo check --all-targets --target aarch64-apple-darwin
```

构建 Windows x64 便携包：

```powershell
.\packaging\build-windows.ps1 -Architecture x64
```

输出写入 `dist/`，不包含任何第三方客户端安装包。

在具备 Xcode、Developer ID Application 和 notarytool profile 的 Mac 上构建一个双架构 DMG：

```bash
APPLE_SIGN_IDENTITY='Developer ID Application: ...' \
APPLE_NOTARY_PROFILE='ai-client-installer' \
./packaging/build-macos.sh
```

无 Apple 凭据时可在 Mac 或私有 CI 中设置 `ALLOW_UNSIGNED_MACOS_BUILD=1` 生成 ad-hoc 签名的验证 DMG；该产物未公证，会被 Gatekeeper 视为测试包，不能冒充正式发行版。仓库提供手动触发的 `.github/workflows/build-macos-validation.yml`。

## 设计边界

- 固定五产品，不做通用插件、远程规则平台或后台服务。
- 运行时只解析版本和短期官方地址，不能扩大内嵌 host、包类型、签名主体、公钥或应用身份。
- ChatGPT 只从固定的 OpenAI 官方更新清单读取动态四段版本；下载地址由本地代码按固定 `persistent.oaistatic.com/codex-app-prod/releases/{version}/ChatGPT-{arch}.msix` 合同构造。包内 `OpenAI.Codex` Identity、Family、Publisher、架构和版本在执行前及安装后再次验证；官方合同变化时失败关闭，不猜测备用源。
- Windows 系统工具不依赖当前目录或普通 `PATH` 搜索：PowerShell 与 `msiexec` 从 Windows 系统目录解析为绝对路径。ChatGPT 不定位或调用 WinGet、Desktop App Installer、Store URI 或引导器。
- 不执行服务器返回的 PowerShell/bash；PowerShell 仅用于本地编译的固定 Windows 检测、签名和 MSIX 安装策略，文件路径通过环境变量或结构化参数传递。
- macOS 只调用绝对路径的系统工具；归档先验证路径、展开上限和链接边界，再检查应用签名。默认安装到 `~/Applications` 以避免无意义的管理员提示；已存在于 `/Applications` 的可信应用只在目录可写时原位更新，不伪造“完全静默提权”。
- 本地操作日志不得保存完整临时下载 URL、用户主目录或临时目录；真实用户日志不进入仓库或发布包。
- 不镜像、不提交、不长期缓存第三方安装包。
