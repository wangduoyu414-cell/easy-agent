# 实现与验证状态

更新时间：2026-09-06

本页只记录当前有效状态。历史问题、原始版本观察和逐步测试过程保留在 [`evidence/`](../evidence/) 中，不再与当前结论混写。

## 当前结论

- `easy agent` 已实现 Windows x64/ARM64 与 macOS Intel/Apple Silicon 平台模型，以及下载、验证、安装、取消、复检、回滚和脱敏日志链路。
- Windows x64 已在干净 Windows 11 虚拟机中通过五款客户端的真实首次安装、安装后复检和独立启动；Windows 10 已通过 CC Switch 的真实安装与复检。
- Claude Windows 完整 MSIX 采用两阶段部署：管理员上下文完成机器级预配，随后在原登录用户上下文注册或更新同一已验证包，避免机器预配成功但当前用户仍保留旧版本。
- ChatGPT Windows 会从固定 OpenAI MSIX 的响应元数据读取最新版本，并同时校验 Package Identity、目标架构、四段 AppX 版本和包大小；本机版本相同或更高时不再显示可点击的更新按钮。元数据暂时不可用时仍保留微软安装器安装能力。
- 下载进度按约 1 MiB 增量合并，并保证起点和完成点仍会通知，避免小块网络读取触发数千次界面刷新和日志/测试输出。
- Windows ARM64 当前只启用 Claude 和 ChatGPT。EXE 构建、PE 架构、图标和版本资源已通过检查，但仍缺少 ARM64 真机安装矩阵。
- macOS Intel/Apple Silicon 当前启用 WorkBuddy、CC Switch、Claude 和 ChatGPT 的直接应用包链。Hermes Intel 明确不支持；Hermes Apple Silicon 只识别 vendor bootstrap，当前禁用。
- Hermes 新增 Windows/macOS 共用的[只读安装诊断](hermes-diagnostics.md)，观察布局、声明版本、Git HEAD 和安装标记一致性。该工具尚不用于 GUI 安装成功判定，不验证运行健康或源码完整性。
- Mac 另有最终桌面 Bundle/ARM64/codesign 诊断，以及绑定实际 Python 进程、工作目录、端口和版本的后端存活探针。它们仍是独立诊断入口，不证明厂商来源、模型访问或整体安装成功；Windows 对应系统检查待实现。
- Hermes Mac 正式扫描与详情已接入只读观察。官方正式版固定源码已在 M1 上完成锁定依赖、ARM64 桌面构建、真实后端健康和同机前后端认证联调；这是自建测试样本，官方 DMG 一键安装仍未闭合。详见[固定源码与真实运行验证](../evidence/hermes-fixed-source-runtime-2026-09-06.md)。
- Apple M1 上已通过原生测试、双架构编译、Universal 打包和 `easy agent` 界面启动；WorkBuddy 两种 Mac 架构已完成当前官方包身份迁移修复和临时安装、重复替换、失败回滚验证。
- CC Switch 3.20.1、ChatGPT 26.901.41600、Claude 1.46388.4 在 M1 上的当前 ARM64 制品已通过临时首次安装、同版本重复替换、失败回滚和清理；尚不能据此宣称旧版升级或客户端独立启动通过。
- 已发布的 Release `v0.1.0-preview.3` 包含 Claude 当前用户更新、PowerShell 回执和下载进度合并修复。Apple Silicon 构建配置、WorkBuddy 身份修复和 Hermes 诊断纳入 `v0.1.0-preview.4`，对应标签工作流重新构建全部安装包。
- 当前 Windows EXE 未做 Authenticode 签名，macOS DMG 未做 Developer ID 签名和 Apple 公证，因此仍属于验证产物。

## 平台支持矩阵

| 平台 | WorkBuddy | Hermes | CC Switch | Claude | ChatGPT | 整机验证状态 |
| --- | --- | --- | --- | --- | --- | --- |
| Windows x64 | 启用 | 启用 | 启用 | 启用 | 启用 | 干净 Windows 11 五款真实首次安装、复检和启动通过 |
| Windows ARM64 | 禁用 | 禁用 | 禁用 | 启用 | 启用 | 构建与静态制品检查通过；真机待验证 |
| macOS Intel | 启用 | 不支持 | 启用 | 启用 | 启用 | 直接应用包链与 Intel 验证制品启动通过；正式公证待完成 |
| macOS Apple Silicon | 启用 | bootstrap 禁用 | 启用 | 启用 | 启用 | M1 原生构建与安装助手界面通过；客户端完整使用矩阵未完成 |

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
- Hermes Windows 官方 bootstrap 会自行安装或配置 Node.js、Python、Git、uv、ripgrep、ffmpeg 等组件。干净环境无需预装这些工具，但建议约 5 GB 可用内存、数 GB 磁盘空间和约 30 分钟安装时间。
- Hermes bootstrap 的厂商取消结果和部分可选组件提示不够可靠；`easy agent` 最终仍以固定安装身份、版本和本地运行复检为准。
- CC Switch 采用用户目录安装时，另一 Windows 用户可能读取到机器级卸载记录但无法访问原用户目录中的程序。该跨用户检测边界尚未调整。
- Windows x64 尚未覆盖五款产品的全部旧版更新、所有 UAC/断网/系统服务异常和账号业务场景。
- Windows ARM64 仍缺少真机验证。Apple Silicon 已有 M1 本地验证，但系统 Applications 首次安装、旧版升级、客户端独立启动及账号业务尚未覆盖完整矩阵。
- WorkBuddy 5.5.3.37748631 两种 Mac 官方包现使用 `com.tencent.workbuddy.mac`，已固定新身份。旧 ID 仅限检测已安装的同 Team 应用，下载包与安装后复检必须匹配新 ID；旧签名包到新包的实际升级尚缺真实样本。

## macOS 当前边界

- 历史验证覆盖过 WorkBuddy、CC Switch、Claude 和 ChatGPT 的 Intel/Apple Silicon 下载、身份验证和临时激活链，不保证厂商后续包身份保持不变。首次结果见 [Apple Silicon 原生开发验证](../evidence/apple-silicon-native-validation-2026-09-06.md)，后续修复及四平台最新检查见 [安装来源审计](../evidence/distribution-source-audit-2026-09-06.md)。
- WorkBuddy 官方 API 提供的 macOS SHA-256 与实际 CDN 文件不一致。只有 WorkBuddy/macOS 使用专用策略：记录厂商摘要异常，并继续强制 Apple 签名、固定 Bundle ID、Team ID、版本、目标架构和稳定文件绑定；该策略不能复用于其他产品。
- Claude 和 ChatGPT 的受验证回退只在明确网络或地区可用性失败时进入，且必须与官方候选的版本、架构、包型和签名完全一致。
- Hermes Apple Silicon DMG 是厂商 bootstrap，默认追踪 `main`，setup 版本、官网版本和最终桌面/runtime 版本不同；最终应用安装与复检尚未实现，因此保持禁用。Windows bootstrap 源码同样存在可变分支与下游镜像回退，历史安装通过不能证明当前来源可重复。

## 发布与验证 Gate

| Gate | 状态 |
| --- | --- |
| Windows x64 干净机真实首次安装 | 已完成 |
| Windows 10 基础兼容与 CC Switch 真实安装 | 已完成 |
| Windows ARM64 真机安装矩阵 | 待完成 |
| Apple Silicon 真机启动与使用 | M1 安装助手原生启动通过；客户端完整使用矩阵待完成 |
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

- [Apple Silicon 原生开发验证与当前制品复验](../evidence/apple-silicon-native-validation-2026-09-06.md)
- [多环境与真实安装测试](../evidence/multi-environment-test-2026-08-21.md)
- [macOS 功能链路审计](../evidence/macos-functional-parity-audit-2026-08-08.md)
- [Claude 四平台接入审计](../evidence/claude-integration-audit-2026-08-12.md)
- [Windows ChatGPT 与 Claude 安装链证据](../evidence/windows-chatgpt-claude-install-chain-2026-08-15.md)
- [Windows 干净机检测证据](../evidence/windows-clean-detection-2026-08-17.md)
