# 五款 AI 客户端官方分发与安装调研

> 2026-08-11 更新：2026-08-02 的 ChatGPT Windows 直接 MSIX 结论已被新运行证据替代。当前 OpenAI 清单对应 x64/ARM64 对象均为 HTTP 404，旧包又声明 `appLicensing`；当前实现使用固定 Store ID 的后台 WinGet 安装来取得微软授权，不打开 Store UI，并禁止无授权本地包兜底。见 `evidence/chatgpt-windows-store-recovery-2026-08-11.md`。

> 2026-08-04 macOS 更新：安装助手确定为一个 Universal DMG，内部同时包含 `x86_64` 与 `arm64`，运行时按物理硬件选择厂商包。WorkBuddy 采用各架构官方 ZIP+摘要；CC Switch 采用官方 minisign Universal tar.gz；Claude 采用官方 Universal DMG；ChatGPT 采用 OpenAI 官方 Intel/Apple Silicon appcast ZIP，并按 OpenAI 当前文档执行 macOS 14 下限；Hermes 官方明确不支持 Intel Mac。Mac 执行核心已实现，Intel 只读取证已固定已观察 Team ID，但所有可安装条目仍因 WorkBuddy 摘要不一致、Claude stable redirect challenge、Apple Silicon/干净机 Gate 与安装复检缺口保持 disabled。

调研日期：2026-07-31
适用目标：Windows 10/11 x64、Windows 10/11 ARM64、macOS 12+ Intel/Apple Silicon
调研性质：任务设计证据；版本、文件大小和短期 CDN 地址均为当日快照，不是应写死的产品常量。

## 1. 结论

V1 可做，但不能把五款软件当成同一种“官网下载 EXE/DMG”处理。最稳妥的产品形态是一个固定五产品、失败关闭的安装助手：每个产品一个适配器，共用环境探测、下载、签名校验、安装执行、安装后复检和日志能力。

最关键的两个结论是：

1. “Claude Code”应定义为 **Claude Desktop 中的 Code 客户端**，不是 Claude Code CLI。Anthropic 当前已经提供 macOS Universal、Windows x64 和 Windows ARM64 的桌面客户端，桌面应用内置 Code 页；CLI 不属于本 V1。
2. ChatGPT Windows 必须作为特殊适配器。OpenAI 公开文档仍以 Microsoft Store 为正式分发入口；要满足“不要 Store 引导器、拿完整安装包”，只能在用户机器上实时解析 Microsoft 官方目录和短期 CDN，再安装完整 MSIX。该路径必须先通过干净系统实测，不能仅凭“能下载 MSIX”就宣称可用。

“长期可用”不应承诺永远免维护。合理定义是：保留稳定的产品身份和信任根，运行时解析易变的版本与地址，把变化隔离在五个适配器中；官方接口或签名身份改变时明确停止并发布新版安装助手，而不是回退第三方源。

## 2. 产品定义与范围

| UI 名称 | V1 的准确产品定义 | 明确排除 |
|---|---|---|
| WorkBuddy | 腾讯 WorkBuddy 桌面客户端，中国站正式分发 | 第三方镜像、非正式渠道、Windows ARM64 猜测性兼容 |
| Hermes | Nous Research Hermes Desktop 官方引导安装器；安装后区分桌面壳与 Hermes runtime 就绪状态 | 社区脚本镜像、由本工具接管 Hermes 下游依赖链 |
| CC Switch | `farion1231/cc-switch` 正式签名发布；Windows MSI、macOS 官方更新产物 | Portable ZIP、Homebrew 安装的接管与全盘扫描 |
| Claude Code | Claude Desktop 的 Code 页；Windows 使用官方 MSIX，macOS 使用官方 Universal DMG | Claude Code CLI、Node/npm、`curl | bash`、`irm | iex` |
| GPT / ChatGPT | OpenAI 当前统一桌面应用（Chat、Work、Codex）；内部产品 ID 使用 `chatgpt` | ChatGPT Classic、第三方“GPT Desktop”、Microsoft Store 引导器 |

## 3. 当前官方分发快照

| 产品 | 当日推荐版本/标识 | Windows | macOS | 版本发现可靠度 |
|---|---|---|---|---|
| WorkBuddy | `5.3.5.34189228` | x64 用户级 EXE；未发现官方 ARM64 包 | Intel 与 Apple Silicon 分包；更新接口返回 ZIP，网站下载使用 DMG | 中：结构化接口，但未公开为稳定 API，Windows 无官方哈希 |
| Hermes | 官网显示 `0.19.1` | 单一 `Hermes-Setup.exe` 引导器，官方称支持 x86_64/aarch64 | 单一 `Hermes-Setup.dmg` 引导器，具体产物架构需真机确认 | 中低：官网 HTML/资源链接，未发现签名版本清单 |
| CC Switch | GitHub latest `v3.19.0` | x64/ARM64 MSI | 同一 macOS 签名 tar.gz（Universal 行为需真机确认），Release 另有 DMG | 高：结构化 `latest.json` + minisign 签名 |
| Claude Desktop | Windows MSIX `1.24012.9` 快照 | x64/ARM64 官方 MSIX 稳定重定向端点 | Universal DMG 稳定重定向端点 | 高：官方稳定端点 + 平台签名；版本从重定向/包元数据读取 |
| ChatGPT | Windows Store 包 `26.727.4816.0` 快照 | x64/ARM64 完整 MSIX 存在于 Microsoft 官方目录 | OpenAI 官方 DMG；macOS 14+，Intel/Apple Silicon | Windows 中：官方目录但解析协议复杂；macOS 中高 |

说明：CC Switch 仓库 `main` 的配置版本在调研时已是 `3.19.1`，但公开 latest release 仍是 `3.19.0`。因此不得用源码主分支版本判断用户应安装的最新版。

## 4. WorkBuddy

### 4.1 权威来源

- [WorkBuddy 官网](https://www.workbuddy.cn/)
- [Windows 安装指南](https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Win-Guide)
- [macOS 安装指南](https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Mac-Guide)
- [FAQ](https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/FAQ)
- [更新日志](https://www.workbuddy.cn/docs/workbuddy/Changelog)
- 公开更新接口：`https://www.workbuddy.cn/v2/update?platform=<platform>`

### 4.2 当日接口证据

- Windows x64：`platform=workbuddy-win32-x64-user`，版本 `5.3.5.34189228`，完整 EXE 约 546 MB。
- macOS Apple Silicon：`platform=workbuddy-darwin-arm64`，接口返回 ZIP 与 SHA-256。
- macOS Intel：`platform=workbuddy-darwin-x64`，接口返回 ZIP 与 SHA-256。
- `workbuddy-win32-arm64-user`、`workbuddy-win32-arm64` 均返回 invalid platform；未发现官方 Windows ARM64 包。

官网页面会把 macOS 更新接口中的 `.zip` 下载路径映射为 `.dmg`。ZIP 有接口哈希，DMG 未在同一响应中给出哈希，因此不能拿 ZIP 哈希校验 DMG；而且同一更新通道返回的 hash 只能补充完整性，macOS 真实性仍必须由固定 Team ID 的代码签名与 Gatekeeper/notarization 建立。

### 4.3 本地静态检查

当日仅下载 Windows EXE 到临时目录做静态检查，未执行安装：

- 产品名：`WorkBuddy`
- 公司：`Tencent Technology (Shenzhen) Company Limited`
- Authenticode：有效
- 签名主体：`Tencent Technology (Shenzhen) Company Limited`
- 当日文件 SHA-256：`3064D6E873BD74169E62EA2E480382C120125E6B8F99155649EC2389C3CBFAFF`

该哈希仅证明当日取样，不得写死为长期信任根。长期校验以官方元数据、包签名和允许的签名主体共同决定。

### 4.4 V1 适配策略

- 版本解析：使用官方更新接口；字段缺失、格式改变或最终域名离开官方白名单时失败关闭。
- Windows：仅原生支持 x64。ARM64 不自动使用 x64 模拟包，除非后续官方文档明确支持并完成真机验证。
- macOS：适配任务先比较“接口 ZIP + 哈希”和“官网 DMG + app 签名”两条官方路径，选择能同时建立来源、完整性和可安装性证据的最小方案。
- 检测：Windows 优先检查 HKCU/HKLM Uninstall 记录、标准安装目录和签名后的主程序；macOS 只检查 `/Applications` 与 `~/Applications` 中的 WorkBuddy.app，并读取 bundle/version/signing 信息。
- 安装：Windows 官方安装器包含许可、安装目录、开始菜单与桌面快捷方式步骤，当前没有可靠静默参数证据；V1 启动官方交互安装器并等待，不能臆造 silent flags。
- 安装后：重新读取注册信息/应用包版本；安装器退出码为 0 但检测不到目标版本时仍判失败。

## 5. Hermes

### 5.1 权威来源

- [Hermes 官网](https://hermes-agent.nousresearch.com/)
- [安装文档](https://hermes-agent.nousresearch.com/docs/getting-started/installation)
- [平台支持](https://hermes-agent.nousresearch.com/docs/getting-started/platform-support)
- [Windows Native 指南](https://hermes-agent.nousresearch.com/docs/user-guide/windows-native)
- [Hermes Agent 官方仓库](https://github.com/NousResearch/hermes-agent)

当日官网资源：

- Windows：`https://hermes-assets.nousresearch.com/Hermes-Setup.exe?build=cc4cab2f592e`
- macOS：`https://hermes-assets.nousresearch.com/Hermes-Setup.dmg?build=cc4cab2f592e`

### 5.2 分发性质

Windows EXE 约 7.6 MB、macOS DMG 约 6.8 MB，明显是引导器而不是完整离线客户端。官方文档说明，桌面安装器会安装桌面应用并在首次启动/安装阶段配置 Hermes runtime；Windows 还可能配置 Python、Node、PortableGit、ripgrep 等依赖。

因此本工具能保证的是“从官方源下载并启动已验证的 Hermes 引导器”，不能把引导器随后下载的每个依赖都伪装成由本工具完整审计或离线托管。

### 5.3 本地静态检查

当日仅下载 Windows 引导器做静态检查，未执行安装：

- 产品：`Hermes`
- 公司：`Nous Research`
- 文件版本：`0.0.1`（引导器自身版本，不等于 Hermes Agent 发布版本）
- Authenticode：有效
- 签名主体：`Nous Research Inc.`
- 当日 SHA-256：`505DFB4C2C1052B055E3FC694A76CB7CE093A64962C7713AA294F5549C6734F5`

### 5.4 V1 适配策略

- 版本解析：从官方首页读取当前推荐版本与带 build 标识的下载链接；不得使用仓库 `package.json` 的版本作为用户最新版。
- 架构：官方平台表声明 Windows 10/11 支持 x86_64 与 aarch64；单一引导器如何选取架构必须在两种 Windows 干净环境中验证。macOS 安装器的 Intel/Apple Silicon 行为同样需要真机证据。
- 检测：桌面应用与 runtime 分开报告。桌面安装成功不等于 runtime 已完成首次配置。
- Windows 检测：优先 Uninstall 记录、发布者和主程序签名；安装目录可配置，不能只硬编码一个路径。runtime 默认根目录可检查 `%LOCALAPPDATA%\hermes`。
- macOS bootstrap 检测：官方 DMG 内应用名为 `Hermes.app`，bundle ID 为 `com.nousresearch.hermes.setup`，Team ID 为 `T2F6S8MF7C`；runtime 默认检查 `~/.hermes`，但不把存在目录等同于健康可用。最终桌面/runtime 状态仍需 Apple Silicon 真机安装证明。
- 安装后结果：至少区分“桌面已安装”“runtime 待首次启动配置”“runtime 就绪”“安装失败”。
- 长期风险：官网未发现带哈希/签名的结构化 latest manifest；解析器必须有 HTML fixture 测试，页面结构变化即停止，不应猜 URL。

## 6. CC Switch

### 6.1 权威来源

- [官方仓库](https://github.com/farion1231/cc-switch)
- [官方 Releases](https://github.com/farion1231/cc-switch/releases/latest)
- [官方更新清单](https://dl.ccswitch.io/latest.json)
- [Tauri 配置中的产品身份和更新公钥](https://github.com/farion1231/cc-switch/blob/main/src-tauri/tauri.conf.json)

### 6.2 当日结构化清单

`latest.json` 当日给出：

- version：`3.19.0`
- Windows x64：`CC-Switch-v3.19.0-Windows.msi`
- Windows ARM64：`CC-Switch-v3.19.0-Windows-arm64.msi`
- macOS x64/ARM64：指向同一个 `CC-Switch-v3.19.0-macOS.tar.gz`
- 每个平台均带 minisign/Tauri updater 签名

应用配置内嵌更新公钥，并把 `https://dl.ccswitch.io/latest.json` 设为主端点、GitHub Releases latest.json 设为回退。发布说明明确：镜像本身不作为信任根，文件仍由内置公钥验签。

### 6.3 V1 适配策略

- 版本解析：优先官方 `dl.ccswitch.io/latest.json`，只有该清单不可用时才使用项目配置声明的 GitHub official fallback；两个来源都必须通过相同更新公钥校验产物签名。
- Windows：使用清单中的 x64/ARM64 MSI，不管理 Portable ZIP；通过 Uninstall/MSI 注册信息检测，禁止使用会触发 MSI 修复副作用的 `Win32_Product` 查询。
- macOS：V1 可优先使用清单已签名的 tar.gz，或在适配 proof 中确认 DMG 能获得同等级信任证据后使用 DMG。安装后用 bundle ID `com.ccswitch.desktop`、版本和代码签名复检。
- 版本比较：采用发布清单版本，不读取 `main` 分支配置版本。

## 7. Claude Desktop（含 Claude Code）

### 7.1 产品语义已确认

- [Claude Code Desktop 快速开始](https://code.claude.com/docs/en/desktop-quickstart)
- [Claude 下载页](https://claude.com/download)
- [Windows 企业部署](https://support.claude.com/en/articles/12622703-deploy-claude-desktop-for-windows)

Anthropic 当前桌面应用包含 Chat、Cowork、Code 三个页签。Code 页即 Claude Code 图形客户端，不需要单独安装 Node.js 或 CLI。Windows 本地 Code 会话需要 Git；Claude Code 功能还受用户订阅权限约束。安装助手只负责应用安装和前置条件提示，不安装 Git、不购买订阅、不自动启用 Virtual Machine Platform。

### 7.2 官方稳定端点

- macOS Universal DMG：`https://claude.ai/api/desktop/darwin/universal/dmg/latest/redirect`
- Windows x64 Setup：`https://claude.ai/api/desktop/win32/x64/setup/latest/redirect`
- Windows ARM64 Setup：`https://claude.ai/api/desktop/win32/arm64/setup/latest/redirect`
- Windows x64 MSIX：`https://claude.ai/api/desktop/win32/x64/msix/latest/redirect`
- Windows ARM64 MSIX：`https://claude.ai/api/desktop/win32/arm64/msix/latest/redirect`

当日 MSIX 重定向版本为 `1.24012.9`；x64 约 258 MB、ARM64 约 254 MB。官方部署文档明确支持 `Add-AppxPackage` 单用户安装。

### 7.3 V1 适配策略

- Windows 选择直接 MSIX，而不是 Setup 引导器，符合“完整可安装包”要求；按 x64/ARM64 选择，校验 MSIX 签名、包身份、版本和最终官方资产域。
- Windows 检测使用官方文档给出的 `Get-AppxPackage -Name Claude` 语义，并核对 package family/publisher，不靠快捷方式。
- macOS 使用 Universal DMG；检测标准位置中的 `Claude.app`，bundle identity 为 `com.anthropic.claudefordesktop`、Team ID 为 `Q6L2SF6YDW`，并核对版本、双架构 slice 与 Gatekeeper。
- 应用约每四小时检查自更新。V1 不关闭、不接管厂商自更新；安装助手再次运行时只比较当前安装版本与当时官方推荐版本。
- 如果 Windows 已由企业 MDM/Provisioned Package 管理，不擅自更换安装所有者或禁用自更新，状态显示“受组织管理”。

## 8. ChatGPT / GPT

### 8.1 权威来源

- [ChatGPT 下载页](https://chatgpt.com/download/)
- [Windows 应用与系统要求](https://help.openai.com/en/articles/9982051-using-the-chatgpt-windows-app)
- [macOS 下载说明](https://help.openai.com/en/articles/9275200-downloading-the-chatgpt-macos-app)
- [macOS 系统要求](https://help.openai.com/en/articles/9395554)
- Microsoft Store Product ID：`9PLM9XGG6VKS`
- Microsoft package family：`OpenAI.Codex_2p2nqsd0c76g0`
- macOS 官方当前 DMG：`https://persistent.oaistatic.com/codex-app-prod/ChatGPT.dmg`

OpenAI 当前说明：新的 ChatGPT 桌面应用合并 Chat、Work、Codex；Windows 最低 Windows 10 build 17763，支持 x64/ARM64；macOS 最低 macOS 14，支持 Intel/Apple Silicon。

### 8.2 Windows 当日目录证据

通过 Microsoft Display Catalog 与 Windows Update FE3 官方分发链，当日读取到：

- x64：`OpenAI.Codex_26.727.4816.0_x64__2p2nqsd0c76g0`
- ARM64：`OpenAI.Codex_26.727.4816.0_arm64__2p2nqsd0c76g0`
- 完整包格式：MSIX
- 大小约 759 MB / 753 MB
- Microsoft 目录提供 SHA-256；最终 CDN 地址带时效，不能持久化

该版本在短时间内会变化，证明“把最终 CDN URL 写死进客户端”不可接受。

### 8.3 V1 特殊适配策略

- 不下载 `get.microsoft.com/installer/download/...` 引导器，也不打开 Microsoft Store UI。
- 运行时以 Product ID、package family 和 Microsoft 类别 ID 为稳定身份，从 Microsoft 官方目录解析最高适用的 x64/ARM64 完整包及依赖；下载地址仅在当前安装会话内使用。
- 同时验证目录摘要、MSIX 签名、package family、publisher、架构和包版本。
- 安装前必须先在干净 Windows x64、Windows ARM64 上证明：无需 Store UI 即可完成包与依赖安装，且不存在授权/entitlement 拒绝。若 proof 失败，ChatGPT Windows 状态必须是“当前无法直接安装”，不能悄悄回退 Store 引导器。
- 检测使用 AppX/MSIX package family；若发现 Classic 与新统一应用并存，分别识别，不自动卸载 Classic。
- macOS 从官方页面/官方资产域解析适用包，下载后读取 DMG 内实际 bundle、架构和版本，不在代码中猜 bundle ID。
- 不缓存、不镜像、不再分发 OpenAI 包；包始终从 OpenAI/Microsoft 官方源直接到用户机器。

## 9. 共同设计难点与处理原则

### 9.1 “最新版”不是同一种协议

五款产品分别使用结构化 API、HTML 页面、签名更新清单、稳定重定向和 Microsoft 目录。统一只应统一结果模型，不应强迫解析实现同构。

建议统一结果字段：

- product id
- raw version 与可比较版本
- OS/arch
- package type
- official source identity
- ephemeral URL
- expected digest/signature policy
- expected package/bundle identity
- installer interaction type
- postcheck policy

### 9.2 稳定身份与易变字段必须分离

可随安装助手版本固化：

- 官方入口与允许的重定向域
- Product ID、package family、Bundle ID、应用 ID
- 官方 updater 公钥
- 允许的签名发布者/Team ID
- 最低系统规则与包类型

必须运行时解析：

- 推荐版本
- 文件大小
- 完整下载地址和查询参数
- 短期 CDN URL
- 当期哈希/签名

实现时必须把这些内容落成版本化、只读的 `trust-registry`，而不是散落在解析代码中。每条启用记录至少包含：初始入口、允许的每个重定向 host、包类型、package family/bundle/product identity、signer/Team/updater public key、验证组合和最低系统规则。远端响应只能填充版本、大小、URL、摘要和当期签名，不能扩大权限。

当前可直接固定的例子包括：

- WorkBuddy Windows signer：`Tencent Technology (Shenzhen) Company Limited`
- Hermes Windows signer：`Nous Research Inc.`
- CC Switch bundle ID：`com.ccswitch.desktop`，以及官方 Tauri updater public key
- Claude macOS bundle identity：`com.anthropic.claudefordesktop`，Team ID：`Q6L2SF6YDW`
- ChatGPT Windows Product ID：`9PLM9XGG6VKS`，package family：`OpenAI.Codex_2p2nqsd0c76g0`

Team ID、MSIX Publisher 或最终 Microsoft CDN host 等值不用通配默认值代替；已取得实包 proof 的值随安装助手版本固定，尚未关闭摘要、入口可达性或 clean-machine 矩阵的产品/平台仍保持 disabled。

### 9.3 版本比较不能只用 SemVer

WorkBuddy 和 AppX/MSIX 版本都不是标准 SemVer。每个适配器负责版本解析；无法可靠比较时只报告“已安装版本”和“官方版本”，状态为未知，不自动覆盖。已安装版本高于当前推荐版本时禁止降级。

### 9.4 安装检测不能靠快捷方式

- Windows：AppX/MSIX package identity、HKCU/HKLM Uninstall、MSI 注册信息、标准安装目录、主程序签名。
- macOS：标准 Applications 目录、Bundle ID、CFBundleVersion、代码签名/Team ID。
- 禁止全盘扫描；CC Switch portable 等非管理来源明确显示为不支持或未知。

### 9.5 下载与完整性

- 每个重定向都验证 scheme/domain，限制跳转次数。
- 下载到每次唯一、仅当前用户可写的随机临时目录的 `.part`；规范化路径必须仍位于本次临时根，拒绝 symlink/junction/reparse point 越界；验证成功后才改为可执行文件名；在启动安装器前再次重开并核对同一路径、长度、摘要与平台签名，避免验证后文件替换。
- 服务器支持 Range 时可在当前会话续传；不支持则重新全量下载，不把续传当成功能硬门。
- 下载前检查 Content-Length、磁盘空间和包魔数；拒绝 HTML/JSON 错误页伪装成安装包。
- 有官方摘要/更新签名时必须验证；没有时必须验证平台代码签名和固定产品身份。
- 验证失败立即删除或隔离临时文件，不能保留一个看似正常的 `.exe/.msi/.msix/.dmg` 名称。
- 不做 TLS 证书 pinning，避免证书轮换造成不必要脆弱；使用系统信任库、官方域名和制品签名形成组合信任。系统代理或 TLS 替换导致校验失败时给出可诊断错误，不绕过校验。

### 9.6 权限与用户交互

- 安装助手默认以普通用户运行，不整体提权。
- 只有具体安装步骤需要时由操作系统显示 UAC/管理员授权。
- 不自动结束正在运行的目标应用；要求用户关闭后重试。
- 供应商安装器需要交互时明确显示“等待厂商安装窗口”，不伪造静默参数。
- 可安全取消的阶段仅限解析/下载/校验；厂商安装器已启动后不承诺强制取消。

### 9.7 安装成功必须复检

“进程退出码为 0”不等于成功。最终成功至少要求：

- 能重新检测到预期产品身份；
- 架构正确；
- 版本达到预期，或厂商安装器已自动升级到更高版本；
- 签名发布者正确；
- 对 Hermes 等二阶段产品，桌面已装和 runtime 就绪分别报告。

macOS 还有一个共同硬门：文件复制到 `/Applications` 不等于可用。所有 macOS 适配器的 succeeded 至少要求 app bundle identity/版本/架构正确、嵌套代码签名完整、Gatekeeper assessment 接受，并且下载、解包和复制过程没有主动清除 quarantine。否则只能报告 `installed_not_launchable` 或失败。

### 9.8 ChatGPT Windows direct-install 的精确 Go/No-Go

ChatGPT Windows 不以“拿到一个 MSIX”作为成功。P0 proof 和后续运行必须同时满足：

1. 主包 package family 精确等于 `OpenAI.Codex_2p2nqsd0c76g0`，Publisher、版本和架构与目标环境匹配。
2. 从主包 AppxManifest 与 Microsoft 更新元数据合并得到依赖集合；每个依赖已安装且达到最小版本，或能从同一 Microsoft 官方元数据链取得并验证。
3. 依赖先安装、主包后安装；全过程不打开 Store UI、不调用 Store URI、不运行 `get.microsoft.com` 引导器。
4. `Add-AppxPackage` 成功后再次通过 package family/version 复检。
5. metadata schema/identity 不一致、依赖不可获得、license/entitlement、Store-only 或组织策略拒绝均是 `unsupported/no-go`，不能进入无限普通重试；只有网络中断、短期 URL 过期、磁盘不足和用户取消属于可恢复错误。

ChatGPT Windows No-Go 只禁用该产品/平台，不阻塞其余四产品或 ChatGPT macOS 的发布。

## 10. 推荐的最小实现技术

推荐使用 Rust 原生核心 + eframe/egui 简单桌面 UI + cargo-packager/平台脚本完成分发：

- 一个代码库可生成 Windows x64/ARM64 便携 EXE 和 macOS Universal app/DMG。
- UI 只需列表、状态、确认、进度、结果和日志，不需要浏览器渲染能力。
- 避免 Electron 体积，也避免让安装助手自身依赖 WebView2 这一额外运行时。
- 安装、签名校验、AppX、注册表、DMG 挂载等平台逻辑可留在清晰的 Windows/macOS 模块中。

Tauri 可作为 foundation proof 失败后的备选，但只有在明确证明目标 Windows 环境无需额外引导安装 WebView2、并满足单文件便携要求时才采用。

## 11. 长期维护策略

V1 不建设远程规则平台。长期维护用以下低成本手段完成：

1. 每个适配器保存官方响应 fixture，解析器做离线契约测试。
2. 发布前运行一次五产品 live resolver smoke test；可选增加不带安装动作的周期巡检，但不得成为远程控制面。
3. 解析字段、包类型、签名主体或 Bundle/Package identity 变化时失败关闭并生成明确日志。
4. 签名主体轮换必须经官方来源和实包双重确认后随安装助手新版发布，不能远程放宽。
   - 变更记录需包含日期、官方来源、样包签名/identity 和一次独立复核。
   - 新旧主体只在当前官方产物仍同时使用时并存；所有受支持当前产物迁移并完成矩阵复验后移除旧主体。
5. 错误界面提供安装助手版本和项目发布页，提示升级安装助手；V1 不做后台自更新。
6. 维护手册记录每个适配器的官方入口、稳定身份、真机矩阵、常见变更和恢复步骤。

## 12. 仍需在执行阶段关闭的证据缺口

- ChatGPT Windows 完整 MSIX 与依赖在干净 x64/ARM64 系统上是否可绕过 Store UI 正常安装。
- WorkBuddy macOS 两架构官方 ZIP 的 API SHA-256 均与 CDN 返回的完整文件不一致；需厂商闭合摘要合同并完成两类 Mac 安装矩阵，不能通过忽略 SHA 处理。
- Hermes 单一 bootstrap 在 Windows ARM64、macOS Apple Silicon 上实际生成的最终桌面/runtime 架构与失败恢复行为；macOS Intel 已按厂商策略关闭为 unsupported。
- 五款 macOS 已观察应用身份已固定；仍需关闭 Claude stable redirect challenge，并完成 Intel/Apple Silicon 首次安装、更新、quarantine、Gatekeeper 与回滚矩阵。
- WorkBuddy、Hermes Windows 安装器是否存在厂商支持的 unattended 参数；未证明前一律按交互安装。
- 自家安装助手的 Windows Authenticode 证书、Apple Developer ID Application/Installer 证书与 notarization 凭据。

前五项是 P0/P3 必须关闭的执行证据。最后一项是已知外部发布前置条件：不阻止 proof 和实现，但缺失时只能生成内部未签名测试包，状态必须是 `Implementation complete; validation pending`，不能把测试包作为最终用户交付物。
