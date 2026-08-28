# 实现与验证状态

更新时间：2026-08-28

本页只记录当前有效状态。历史问题、原始版本观察和逐步测试过程保留在 [`evidence/`](../evidence/) 中，不再与当前结论混写。

## 当前结论

- `easy agent` 已实现 Windows x64/ARM64 与 macOS Intel/Apple Silicon 平台模型，以及下载、验证、安装、取消、复检、回滚和脱敏日志链路。
- Windows x64 已在干净 Windows 11 虚拟机中通过五款客户端的真实首次安装、安装后复检和独立启动；Windows 10 已通过 CC Switch 的真实安装与复检。
- Windows ARM64 当前只启用 Claude 和 ChatGPT。EXE 构建、PE 架构、图标和版本资源已通过检查，但仍缺少 ARM64 真机安装矩阵。
- macOS Intel/Apple Silicon 当前启用 WorkBuddy、CC Switch、Claude 和 ChatGPT 的直接应用包链。Hermes Intel 明确不支持；Hermes Apple Silicon 只识别 vendor bootstrap，当前禁用。
- Release `v0.1.0-preview.2` 与当前主分支的运行代码、信任配置和打包逻辑一致；后续差异只涉及仓库清理和文档维护。
- 当前 Windows EXE 未做 Authenticode 签名，macOS DMG 未做 Developer ID 签名和 Apple 公证，因此仍属于验证产物。

## 平台支持矩阵

| 平台 | WorkBuddy | Hermes | CC Switch | Claude | ChatGPT | 整机验证状态 |
| --- | --- | --- | --- | --- | --- | --- |
| Windows x64 | 启用 | 启用 | 启用 | 启用 | 启用 | 干净 Windows 11 五款真实首次安装、复检和启动通过 |
| Windows ARM64 | 禁用 | 禁用 | 禁用 | 启用 | 启用 | 构建与静态制品检查通过；真机待验证 |
| macOS Intel | 启用 | 不支持 | 启用 | 启用 | 启用 | 直接应用包链与 Intel 验证制品启动通过；正式公证待完成 |
| macOS Apple Silicon | 启用 | bootstrap 禁用 | 启用 | 启用 | 启用 | 包身份/架构链通过；原生真机启动与使用待验证 |

支持状态的权威代码来源是 [`config/trust-registry.toml`](../config/trust-registry.toml)。远端元数据只能在该文件定义的边界内提供版本和地址，不能扩大信任范围。

## Windows x64 真实安装结果

以下结果均经过 `easy agent` 的生产 `resolve_install_plan` 与 `run_install_plan` 路径，不是手工绕过程序安装：

| 产品 | 结果 | 已安装身份/版本 | 独立启动结果 |
| --- | --- | --- | --- |
| WorkBuddy | 成功 | `WorkBuddy 5.3.14`，腾讯 Publisher，x64 | 登录窗口正常显示 |
| CC Switch | 成功 | `CC Switch 3.20.0` | 应用正常启动；Windows 10/11 均完成安装验证 |
| Claude | 成功 | `Claude_1.34493.1.0_x64__pzs8sxrjxfjjc` | 首次使用窗口正常显示 |
| ChatGPT | 成功 | `OpenAI.Codex_26.818.5229.0_x64__2p2nqsd0c76g0` | 登录窗口正常显示 |
| Hermes | 成功 | `com.nousresearch.hermes`，`0.20.5`，x64 | 桌面页面正常，Python 后端 `/api/health` 返回 200 |

未提供模型服务账号/API Key 或第三方消息凭据，因此真实模型回复和消息投递仍未验证；这不影响安装、版本检测、本地后端健康和页面渲染结论。

## 已知限制

- GUI 依赖 OpenGL 2.0 或更高版本。缺少有效图形驱动的旧设备或受限虚拟机可能在启动时退出。
- ChatGPT Windows 的微软分发链没有提供稳定、可直接比较的公开“最新版本号”。程序能精确识别本机 Package 身份和版本，但已安装最新版时仍可能保留“更新”按钮，让微软执行可用版本检查。
- Hermes Windows 官方 bootstrap 会自行安装或配置 Node.js、Python、Git、uv、ripgrep、ffmpeg 等组件。干净环境无需预装这些工具，但建议约 5 GB 可用内存、数 GB 磁盘空间和约 30 分钟安装时间。
- Hermes bootstrap 的厂商取消结果和部分可选组件提示不够可靠；`easy agent` 最终仍以固定安装身份、版本和本地运行复检为准。
- CC Switch 采用用户目录安装时，另一 Windows 用户可能读取到机器级卸载记录但无法访问原用户目录中的程序。该跨用户检测边界尚未调整。
- Windows x64 尚未覆盖五款产品的全部旧版更新、所有 UAC/断网/系统服务异常和账号业务场景。
- Windows ARM64 与 Apple Silicon macOS 缺少对应真机；虚拟机或交叉构建结果不能代替真机验收。

## macOS 当前边界

- WorkBuddy、CC Switch、Claude 和 ChatGPT 的 Intel/Apple Silicon 包已完成完整下载、签名/身份/架构验证、临时首次安装、原位更新、失败回滚和失败新装清理。
- WorkBuddy 官方 API 提供的 macOS SHA-256 与实际 CDN 文件不一致。只有 WorkBuddy/macOS 使用专用策略：记录厂商摘要异常，并继续强制 Apple 签名、固定 Bundle ID、Team ID、版本、目标架构和稳定文件绑定；该策略不能复用于其他产品。
- Claude 和 ChatGPT 的受验证回退只在明确网络或地区可用性失败时进入，且必须与官方候选的版本、架构、包型和签名完全一致。
- Hermes Apple Silicon DMG 是厂商 bootstrap，不是已建模的最终直接 `.app` 包，因此保持禁用。

## 发布与验证 Gate

| Gate | 状态 |
| --- | --- |
| Windows x64 干净机真实首次安装 | 已完成 |
| Windows 10 基础兼容与 CC Switch 真实安装 | 已完成 |
| Windows ARM64 真机安装矩阵 | 待完成 |
| Apple Silicon 真机启动与使用 | 待完成 |
| Windows Authenticode 发布签名 | 待提供证书 |
| macOS Developer ID、公证与 stapling | 待提供 Apple 凭据 |
| 五款客户端全部更新/异常/账号业务矩阵 | 部分完成 |

## 当前质量检查

当前主分支已通过：

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets`
- GitHub Actions Windows、macOS 验证构建与预览发布工作流语法检查
- README 本地链接、外部链接和 Release 下载链接检查

代码或配置发生变化后，这些结果必须重新执行，不能沿用旧提交的检查结论。

## 权威证据

- [多环境与真实安装测试](../evidence/multi-environment-test-2026-08-21.md)
- [macOS 功能链路审计](../evidence/macos-functional-parity-audit-2026-08-08.md)
- [Claude 四平台接入审计](../evidence/claude-integration-audit-2026-08-12.md)
- [Windows ChatGPT 与 Claude 安装链证据](../evidence/windows-chatgpt-claude-install-chain-2026-08-15.md)
- [Windows 干净机检测证据](../evidence/windows-clean-detection-2026-08-17.md)
