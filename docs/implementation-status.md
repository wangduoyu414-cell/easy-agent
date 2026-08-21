# 实现与验证状态

更新时间：2026-08-17

## 已实现

- Rust + egui 原生桌面程序，Windows 使用静态 CRT，发布目录只需一个 EXE。
- Windows x64/ARM64 与 macOS Intel/Apple Silicon 平台模型。
- Windows 已安装检测：完整刷新不再为五个产品同时启动五套相同 PowerShell。Claude/ChatGPT 使用一个 20 秒有界的精确 AppX Main-package 探针，WorkBuddy/Hermes/CC Switch 使用另一个独立的注册表/固定目录探针；AppX 变慢或失败不会再拖住三个注册表产品。AppX family/name 与 HKCU/HKLM Uninstall 仍执行固定身份核对，不调用 `Win32_Product`。
- Windows 本机安装状态与联网获取最新版本已分开呈现：本机确认未安装后显示“未安装 · 正在获取最新版本”，PowerShell/AppX/注册表失败显示“本机安装状态检测失败”并禁用操作，不再把检测失败伪装成未安装。单产品安装前复检也会拒绝在检测失败时继续猜测执行。
- WorkBuddy 卸载项由 Rust 固定规则核对：只接受 `WorkBuddy` 或纯数字点分版本后缀，并要求 Publisher 精确为腾讯；PowerShell 只读取候选注册项，不负责放宽身份判断。
- WorkBuddy Windows x64 的官方更新包是 x86 NSIS bootstrap。信任注册表只对该产品/平台固定 `windows_exe_machine = x86`，并固定安装后主程序 `WorkBuddy.exe`；其他 Windows EXE 仍默认要求 PE machine 与目标架构一致。
- WorkBuddy 的卸载注册项与主程序只登记三段发行版本，例如 `5.3.8`；官方更新 API 使用带内部构建号的 `5.3.8.34705286`。UI、安装前重复判断和安装后复检统一按前三段比较，避免成功安装后继续误报更新或等待超时。
- 五类真实在线解析器：WorkBuddy 结构化更新接口、Hermes 官方首页、CC Switch 签名更新清单、Claude 官方完整 MSIX/Universal DMG 重定向，以及 ChatGPT Windows 固定微软安装器/离线包合同与 macOS Sparkle appcast。
- Claude Windows x64/ARM64 与 macOS Intel/Apple Silicon 均已实现固定受验证回退：只对明确的地区/网络/服务可用性失败触发，镜像清单需 minisign、独立 URL 白名单、SHA-256、大小和同步时效全部通过；合同、安全白名单、证书或平台身份错误禁止回退。四类硬件的干净机安装闭环仍单独待验证。
- 东京/香港现有同步链仍在 Windows schema 2 清单中同时记录 Setup 与完全同版本完整 MSIX，macOS 继续使用 schema 1 Universal DMG；2026-08-15 起 Windows 客户端只读取并安装清单中的 MSIX，不再下载或运行 Setup。香港节点仍每 30 分钟单实例同步，客户端对瞬时连接/超时/响应中断增加最多三次元数据重试，证书和合同错误保持失败关闭。
- 产品扫描已改为互相独立并发执行；某个官方端点变慢或失败时，只禁用对应产品，不再让其他已完成解析的安装按钮继续变灰。安装批次结束时若仍有扫描在进行，会在当前扫描收口后自动复检。
- Claude Windows 使用 Anthropic 官方完整 MSIX，官方直连不可用时才读取香港签名清单中的同版本 MSIX。程序验证 SHA-256（回退）、AppX 签名、Publisher、Identity、架构和版本后，通过本地固定管理员部署命令执行机器级安装；不再启动 Setup，因此安装阶段不会重新联网下载约 253 MB。最终仍以 Claude Package 身份、架构和版本复检判定成功；Cowork 的 Windows 虚拟机平台与重启要求单独提示。
- ChatGPT Windows x64/ARM64 已改为 OpenAI 官方 Windows 部署说明中的双路径：优先下载并启动微软轻量安装器；网络、Windows Update、微软分发服务明确不可用或返回 `1612/0x64C` 安装源缺失时，自动下载固定架构完整 MSIX 与离线许可证并请求管理员部署。已删除 WinGet/App Installer 探测、自愈和组件升级，避免共享组件占用把 ChatGPT 安装拖入无关修复流程。
- 微软安装器每次重新下载并验证 Microsoft Authenticode，以及签名证书扩展里的 `MSStoreTag001` 产品配置：Product ID=`9PLM9XGG6VKS`、PFN=`OpenAI.Codex_2p2nqsd0c76g0`、`installerType=WindowsUpdate`、`isHarbor=true`、`autoUpdate=false`。完整包兜底则验证 Store 签名、Identity、Publisher、版本、架构、无额外框架依赖，以及许可证 Product ID/PFM、`Full/Offline` 和无需租约。
- 版本化嵌入式 `trust-registry`，远端响应只能在固定边界内提供易变版本和地址。
- HTTPS、官方/镜像独立逐跳 host/path 校验、最多五次重定向、4 MiB 元数据限制、2 GiB 安装包限制。
- 每次随机私有暂存目录、`.part` 写入、同会话最多三次受对象绑定的 Range 续传、最终长度校验，并在打开部分文件时拒绝符号链接/Windows reparse point。
- 直接安装包在完成摘要、签名、身份、版本和架构验证后，会额外复制到 Windows Known Folder Downloads 或 macOS 当前用户 `Downloads`。复制过程重新核对字节数和 SHA-256，冲突时使用摘要文件名且不覆盖；真正安装继续使用私有暂存文件。ChatGPT Windows 的微软安装器与完整包只在私有暂存中执行，不长期缓存动态引导器或 600+ MB 离线包。
- SHA-256、CC Switch minisign 流式验证接口、Windows Authenticode/AppX 验证、EXE PE 架构和 MSIX manifest 身份读取。
- 安装启动前再次核对稳定路径、长度、摘要和平台签名；MSI/EXE/MSIX 使用结构化进程参数，不拼接 shell 命令。
- PowerShell 与 `msiexec` 通过 Windows 系统目录 API 解析为受信任绝对路径，避免当前目录或普通 `PATH` 劫持。
- Windows 正式版使用 GUI subsystem，所有本地检测、签名校验、PowerShell、MSI/MSIX/EXE 安装子进程统一设置 `CREATE_NO_WINDOW`；安装程序自身的图形界面或 UAC 提示仍按其正常行为显示，但不再创建黑色命令行窗口。
- ChatGPT 安装后要求 Package Identity=`OpenAI.Codex`、Family=`OpenAI.Codex_2p2nqsd0c76g0`、Publisher、架构和版本全部匹配；超时但系统部署仍可能继续时报告“结果待复检”，不会强杀系统安装或假报成功。
- UI 安装/更新前确认、每产品独立任务、并行下载/校验、按系统引擎隔离安装、取消、postcheck、单项失败隔离与摘要已接到真实执行路径。厂商 EXE 与 macOS 应用复制可直接并行；MSI 只与 MSI 排队，MSIX/Store 只在 AppX 通道内排队，MSI 与 Store 可并行。等待状态只在真实发生同通道冲突时显示，排队中仍可取消。成功后只刷新对应产品，失败、取消或结果未知会保留该产品的具体错误。
- ChatGPT 状态在尚未安装前只显示当前版本/状态；操作时依次显示下载微软安装器、等待微软窗口、必要时准备完整安装包、管理员部署和最终复检。无真实百分比的阶段使用移动短条；每个文件只显示自己的真实下载百分比。
- 直接包安装器退出码为 0 后进行最多约 90 秒的有界版本复检；错误 Package Family/架构立即失败，目标尚未登记或仍是旧版本则继续等待，超时收敛为 `ResultUnknown` 并禁止自动假刷新。WorkBuddy 还会从受信任注册项的 InstallLocation、DisplayIcon 或 UninstallString 定位固定的 `WorkBuddy.exe`，最终主程序不是 x64 时硬失败，文件尚未出现时才继续等待。
- 安装批次状态自动追加到 `%LOCALAPPDATA%\easy agent\logs\operations.jsonl`；日志只保留状态切换和最终错误，脱敏 URL/用户目录/临时目录，并在 1 MiB 时轮换一份 previous 文件。日志不可用不会阻断安装，但会在批次摘要中明确提示。
- 已装更高版本、受组织管理、管理状态未知或现有版本未知时在下载前失败关闭；相同版本不重复安装。
- 中文字体、真实检测/解析后台线程和失败关闭界面。
- 客户端下载页已按参考稿重构为纯白居中布局：默认内容区 `800×620`、最小 `740×580`，顶部只显示“常用 AI 客户端 · 一键安装”，可操作产品优先排列，五个客户端使用统一图标容器、版本/状态和右侧操作按钮；安装期间显示实时进度条、明确取消状态并阻止在关键写入阶段直接退出。
- 安装/更新确认采用带轻遮罩的页内紧凑模态卡片，只展示当前状态、目标版本和必要功能限制。官方域、CDN、镜像、包类型、架构和同步时间继续由后台固定校验并写入脱敏操作日志，不再要求普通用户理解下载链路。
- macOS 平台执行链已接入真实代码路径：Universal 进程用 `hw.optional.arm64` 判断物理硬件，避免 Apple Silicon 在 Rosetta 下误下 Intel 包；同时读取 macOS 版本并执行产品最低版本门禁。
- HTTP 客户端启用系统代理发现，Finder 启动的桌面应用无需依赖 shell 代理环境变量；HTTPS、逐跳 host/path 和重定向上限仍不放宽。ChatGPT macOS 在官方元数据或完整 ZIP 下载发生明确网络可用性失败时可进入固定受验证回退；回退包必须与已确认官方候选的版本、架构、包型、大小和 Sparkle 签名完全一致，合同、安全白名单、证书、签名或大小错误不允许回退。
- 应用品牌已统一为 `easy agent`：eframe 窗口、应用 ID、操作日志目录、HTTP User-Agent、Windows EXE 资源和 macOS Bundle/DMG 输出均使用新名称。项目所有者提供的图标已转换为运行时 PNG、Windows ICO 与 macOS ICNS，并由 `branding_contract` 测试锁定一致性。
- macOS 检测只检查 `/Applications` 与 `~/Applications`，要求固定应用名/Bundle ID，读取包内版本、主 Mach-O slice、Team ID 与根代码签名。完整深度 codesign 和 Gatekeeper 保留在下载候选与最终激活边界，日常刷新不再重复执行安装级 Gatekeeper；本机 ChatGPT 状态扫描由约 28 秒降至约 5 秒。
- macOS 安装支持 DMG、ZIP、tar.gz：DMG 只读挂载；归档在展开前限制条目数、总大小、路径穿越、重复路径和逃逸链接，展开后再次验证链接边界；只接受一个固定名称的 `.app`。
- macOS 首次安装优先使用可写的系统 `/Applications`，无权限时才使用 `~/Applications`；现有可信应用仍原位更新，同时发现用户级/系统级两份时拒绝猜测。新 app 先复制到目标卷私有暂存目录并复验，再原子替换并显式注册 LaunchServices；最终复验或注册失败会恢复旧版，恢复失败则保留备份而不是删除。
- macOS 下载前和最终替换前都会检查目标应用的精确主 executable 是否仍在运行，并在下载前检查安装目录可写性。现有可信 Intel 应用可以在 Apple Silicon 上迁移到 ARM64；候选包与最终应用仍必须严格包含目标架构。
- 五产品 macOS 解析合同已编码：WorkBuddy Intel/Apple Silicon 官方 ZIP+SHA-256，Hermes Apple Silicon 官方 DMG，CC Switch 双架构同一 minisign tar.gz，Claude Universal DMG 稳定重定向，ChatGPT Intel/Apple Silicon 官方 Sparkle appcast ZIP。2026-08-14 OpenAI 最新版把最低要求提高到 macOS `13.0`；客户端现读取每个官方候选的数字点分最低版本并与当前系统比较，信任注册表同步维护当前支持基线，避免再把 `12.0` 写死而误判兼容电脑。
- macOS 条目新增显式 `direct_app_bundle` / `vendor_bootstrap` 策略。只有前者可启用；Hermes setup 被标记为 vendor bootstrap，注册表、解析器和执行器都会阻止它误走普通 `.app` 复制链。
- ChatGPT macOS 固定已签名官方应用中的 `SUPublicEDKey`，appcast `edSignature` 必须解码为 64-byte Ed25519 签名；完整 ZIP 在下载后和执行前都进行 Sparkle 验签。香港节点每 30 分钟同步 x64/ARM64 固定包，先核对 appcast、Bundle、版本、最低 macOS、Mach-O 和 OpenAI 签名，并要求 ZIP 内 `LSMinimumSystemVersion` 与 appcast 一致，再发布 minisign 清单、大小、SHA-256、时效和不可变路径；客户端仍会重复验证 OpenAI 与 Apple 身份。
- Intel Mac 只读取证已固定 CC Switch Team ID `R8UR22V2F9`、WorkBuddy Team ID `FN2V63AD2J`、Claude Team ID `Q6L2SF6YDW`、ChatGPT Team ID `2DC432GLL2` 和 Hermes bootstrap Team ID `T2F6S8MF7C`。Hermes 官方 DMG 的真实应用身份是 `Hermes.app` / `com.nousresearch.hermes.setup`，不是此前记录的最终桌面 Bundle ID。
- 只读取证发现 WorkBuddy Intel/Apple Silicon API 声明的 SHA-256 与各自 CDN 完整 ZIP 不一致。macOS 现采用仅限固定 WorkBuddy 身份的产品级策略：厂商摘要仍比较并记录警告，实际下载文件的 SHA-256 会立即绑定并在安装前后平台验证时重复核对；固定 Bundle ID、Team ID、版本、目标架构、codesign 与 Gatekeeper 仍全部硬失败。该策略不能用于其他产品、Windows、其他包型或其他应用身份。
- WorkBuddy 首次启动会在固定编辑器引擎目录新增 `editor_sdk.log`，导致后续严格资源封印检查失败。已安装检测只对该产品、对应架构和这一条固定普通日志路径使用专用复检：日志必须非符号链接、不可执行且大小受限，严格检查输出不得包含任何其他新增/修改；随后仍要通过全部可执行代码签名、固定 Team ID、Bundle ID、版本和架构检查。其他产品和其他资源变化仍失败关闭。
- Claude stable redirect 在大陆直连会跳转到精确的地区不可用页面；客户端将该页面识别为可用性失败，并读取固定香港清单。2026-08-12 四个平台清单验签、客户端解析、下载阶段版本绑定、真实包身份和 macOS 双架构临时安装/更新/失败回滚均已通过；干净机交互验收仍为 validation pending。
- 2026-08-12 真实包闭环复检后，WorkBuddy、CC Switch、Claude 和 ChatGPT 的 macOS x64/ARM64 八个条目均完成“完整下载 → updater/摘要 → Apple 身份 → 临时首次安装 → 原位更新 → 强制最终失败回滚 → 新装失败清理”。Hermes 仍按 vendor bootstrap 原因失败关闭。
- macOS 外部系统命令按类别设置 30 秒至 15 分钟上限；超时会终止独立进程组并回收子进程。ChatGPT Gatekeeper 正常可能超过 30 秒，因此 codesign/Gatekeeper 使用 120 秒而不是统一短超时。
- 自家 macOS 打包脚本生成一个包含 `x86_64` 与 `arm64` slice 的 Universal `.app`/DMG，构建时写入 Cargo 版本，执行 codesign、notarytool、staple、Gatekeeper 和 SHA-256。

## 2026-08-02 本机观察

环境：Windows 11 Pro build 26100，x64。

| 产品 | 检测结果 | 官方解析结果 | 当前动作 |
|---|---|---|---|
| Hermes Agent | 未检测到注册安装 | `0.19.1` | 可安装；运行官方签名 bootstrap，完成后复检桌面身份 |
| Claude Desktop / Code | 已安装 `1.24012.1.0` | `1.24012.9` | 当日历史观察；当前设计已改为直接验证并机器级部署完整 MSIX，完成后复检 Package Family/Identity/架构/版本 |
| ChatGPT | 已安装 `26.721.11231.0`，identity `OpenAI.Codex`，family `OpenAI.Codex_2p2nqsd0c76g0`，Publisher `CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B`，x64 | Store 当前生产版本 | “更新”可执行；后台获取微软授权并安装，结束后精确复检 |
| WorkBuddy | 已安装 `5.1.7`，注册项 `WorkBuddy 5.1.7`，Publisher 为腾讯，`WorkBuddy.exe` 为 x64，HKCU | `5.3.8.34705286` | “更新”可执行；验证官方 x86 bootstrap，安装后要求最终 `WorkBuddy.exe` 为 x64 且达到目标版本 |
| CC Switch | 已安装 `3.17.0` | `3.19.1` | 可更新；验证官方 Tauri minisign、MSI 产品身份与架构 |

这些版本是当日在线观察，不是代码常量。ChatGPT 固定 Store Product ID、Package Identity/Family/Publisher 和安装器/离线包信任合同，实际版本由微软分发服务或已验证的官方完整包决定。

## WorkBuddy 当前失败与只读 proof

- 用户在 2026-08-02 03:45 点击“更新”后，操作日志明确记录：下载完成、进入验证、随后因 `unsupported PE machine 0x014c` 失败。流程没有到达 `AwaitingUserInstall`/`Installing`，因此没有启动安装器。
- 当前官方更新接口返回 `5.3.8.34705286` 的 `win32-x64-user` EXE，407,285,928 bytes，SHA-256 `C111BC3F54A0E53FA04924313AE660125EEBFFAFCD5AC7722DA7C3C03402CB7A`。只读检查确认 PE machine 为 x86、NSIS 标记存在、Authenticode 有效、Signer 为腾讯、ProductName 为 `WorkBuddy`。
- 本机现有 `WorkBuddy.exe` 为 x64、`Uninstall WorkBuddy.exe` 为 x86，证明厂商将 x86 安装/卸载外壳与 x64 主程序分开。当前官方包已通过修改后的同一套 Rust 验签/身份/bootstrap machine 规则；本机注册检测也通过最终 `WorkBuddy.exe` x64 proof。
- 本轮没有运行该 407 MB 安装包。真实单动作更新仍需 Windows x64 可丢弃快照关闭 Gate。

## ChatGPT 当前链路修正

- 2026-08-11 在线清单返回 build `26.803.10989.0`，但 OpenAI x64/ARM64 直连地址从本机与东京节点均返回 HTTP 404；旧版本 `26.727.6591.0` 仍返回 206，证明问题是发布合同存在不可避免的“清单先到、包后到”窗口。
- 对仍可用的官方 x64 MSIX 远程读取 `AppxManifest.xml`，确认 Identity/Publisher/架构正确，同时声明受限能力 `appLicensing`。旧电脑的本地 Add/Update 成功不能证明全新电脑已获得所需微软授权。
- 当前实现使用微软轻量安装器主路径；它自身支持 Windows Update 与 Package Retrieval Service。被代码明确分类为网络/微软分发服务不可用的退出结果，以及实测出现的 Windows Installer `1612/0x64C`，会进入 OpenAI 官方完整 MSIX + 离线许可证兜底；取消、UAC、安全、身份、架构、策略和许可证错误全部停止。
- 2026-08-15 Claude Windows 官方 x64/ARM64 MSIX 重定向在本机无代理直连均返回可用制品，Setup 与 macOS DMG 入口仍会受地区限制。客户端已切换为 MSIX 直连优先、香港同版本签名回退，并删除运行时 Setup 依赖。四平台在线解析/回退匹配测试在加入元数据重试后通过；Windows 真实管理员部署与最终 Cowork 行为仍需干净机验收。
- 2026-08-14 OpenAI macOS 最新版 `26.810.41047` 将最低要求从 `12.0` 提高到 `13.0`。客户端与香港同步工具已移除固定 `12.0` 判断：官方候选、签名回退清单和 ZIP 内 `LSMinimumSystemVersion` 必须一致，客户端再与当前系统版本比较。香港 x64/ARM64 清单均已同步为 `26.810.41047` / `13.0`，定时任务恢复成功；官方解析、双架构安装计划和官方/备用精确匹配三项在线测试均通过。
- 2026-08-12 已从 macOS 重新交叉构建 Windows x64/ARM64 GUI EXE，PE machine 分别为 `0x8664`/`0xAA64`，两者均包含 ICON、GROUP_ICON、VERSIONINFO，SHA-256 校验通过；未签名测试包的 Windows 真机启动/安装仍待用户验证。
- 2026-08-15 在 Claude 完整 MSIX 管理员部署、ChatGPT `1612/0x64C` 自动离线兜底和元数据瞬时失败重试全部接入后，当前工作树重新生成 Windows x64/ARM64 GUI EXE：x64 为 9,610,240 bytes、SHA-256 `660cb99f995a7740093f30d73a37b6d600d2c588efec1252d1c81cb4e2eb16eb`；ARM64 为 8,554,496 bytes、SHA-256 `c8737be5b5e5b7c8e9d66fdfd9371b19af351a843606f7d296d201bc76df2605`。PE machine、GUI subsystem、ICON、GROUP_ICON、VERSIONINFO 与校验文件均复检通过；Windows 干净机管理员安装仍待真机验收。
- 2026-08-17 修复 x64 全新电脑首次状态扫描后重新生成 Windows x64 GUI EXE：9,630,208 bytes，SHA-256 `2199cefb886183041139d3388eaa80591b3eab9650800916c4beef811a9d569b`。当前产物为 PE32+ AMD64、GUI subsystem，并包含 ICON、GROUP_ICON 和 VERSIONINFO；格式、测试、Clippy、Windows x64/ARM64 目标检查与构建脚本均通过。实际 x64 全新电脑的 AppX/注册表超时、明确失败状态和联网解析分离仍需该产物真机验收。
- 历史 Windows x64 未签名测试产物仍以旧品牌文件名 `dist/AI-Client-Installer-windows-x64.exe` 留作当时证据，10,597,888 bytes，SHA-256 `d7f7e6e3fac236ec67d0600a19f1f5cd014b42c9f746c61a394e1cabe9afde17`。当前构建脚本已改为输出带 `easy agent` 图标和资源的 `dist/easy-agent-windows-x64.exe`。

## 验证待完成

- Windows x64 干净机：优先对 WorkBuddy 补齐单动作更新、bootstrap 退出结果、最终 `WorkBuddy.exe` x64 与注册版本 proof；ChatGPT 补齐微软安装器首次安装/更新、服务不可用自动兜底、UAC 取消与最终身份复检。
- Windows ARM64：release cross-build、PE/资源与 SHA-256 校验已通过；仍需 Windows ARM64 真机启动、安装矩阵和 Authenticode 发布签名。
- macOS：2026-08-12 在 Intel Mac 上重新下载并验证 WorkBuddy、CC Switch、Claude 与 ChatGPT 的 x64、ARM64/Universal 八个真实包，固定 Bundle ID、Team ID、版本、目标 slice、codesign 与 Gatekeeper 均通过；WorkBuddy 的厂商摘要仍按已记录的专用平台签名策略处理。四款产品的两种架构候选均完成临时首次安装、原位更新、失败回滚和失败新装清理。Intel 主机不能替代 Apple Silicon 原生启动验收；Hermes bootstrap 最终状态仍待厂商侧条件闭合。
- macOS 自家制品：2026-08-12 当前工作树重建 Universal 验证 DMG，应用包含 x86_64/arm64 两个 slice、新图标与 Applications 拖拽入口；55 个核心测试、品牌合同、解析/安全边界、ad-hoc codesign、只读挂载、SHA-256 和 Intel 实际启动均通过。尚未使用 Apple Developer ID/notary，也未在 Apple Silicon 真机启动，因此该文件强制保留 `UNNOTARIZED-VALIDATION` 名称，不能作为正式外发包。
- macOS 自家制品：2026-08-15 基于同一当前工作树再次重建 Universal DMG，12,609,009 bytes，SHA-256 `48edf84e3103e5bfd4aba60434f22cc32c498f63e989a43eeb03897dcff58898`；DMG 校验、只读挂载、x86_64/arm64、ad-hoc codesign 与从新挂载 DMG 实际启动均通过。Developer ID/notary 仍未提供，因此文件继续保留 `UNNOTARIZED-VALIDATION` 名称，其他电脑首次打开需要按未公证应用方式确认，不能称为正式公证发布包。
- ChatGPT Windows：x64/ARM64 干净机真实首次安装与旧版更新、运行中应用关闭行为、微软安装器网络/服务失败分类、完整包 UAC 部署、取消/安全错误不兜底和最终身份/版本 Gate。
- 正式发布签名：Windows Authenticode 证书与 Apple Developer ID/notary 凭据未提供。

因此当前状态是：`easy agent Windows x64/ARM64 build complete; macOS direct-app parity implemented for WorkBuddy, CC Switch, Claude and ChatGPT on Intel/Apple Silicon; Hermes remains product-specifically fail-closed; release signing and clean-machine release validation pending`。当前 EXE 与 Mac DMG 都仍是开发验证产物；尚未生成 Apple Developer ID 签名并公证的正式 Mac DMG。
