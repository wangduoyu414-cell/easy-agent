# 实现与验证状态

更新时间：2026-08-03

## 已实现

- Rust + egui 原生桌面程序，Windows 使用静态 CRT，发布目录只需一个 EXE。
- Windows x64/ARM64 与 macOS Intel/Apple Silicon 平台模型。
- Windows 已安装检测：精确 AppX family/name 和 HKCU/HKLM Uninstall 注册信息，不调用 `Win32_Product`。
- WorkBuddy 卸载项由 Rust 固定规则核对：只接受 `WorkBuddy` 或纯数字点分版本后缀，并要求 Publisher 精确为腾讯；PowerShell 只读取候选注册项，不负责放宽身份判断。
- WorkBuddy Windows x64 的官方更新包是 x86 NSIS bootstrap。信任注册表只对该产品/平台固定 `windows_exe_machine = x86`，并固定安装后主程序 `WorkBuddy.exe`；其他 Windows EXE 仍默认要求 PE machine 与目标架构一致。
- WorkBuddy 的卸载注册项与主程序只登记三段发行版本，例如 `5.3.8`；官方更新 API 使用带内部构建号的 `5.3.8.34705286`。UI、安装前重复判断和安装后复检统一按前三段比较，避免成功安装后继续误报更新或等待超时。
- 五类真实在线解析器：WorkBuddy 结构化更新接口、Hermes 官方首页、CC Switch 签名更新清单、Claude 官方稳定 MSIX 重定向、ChatGPT 官方 Windows 更新清单。
- ChatGPT Windows x64 已接入与其余产品相同的直接包单主动作：固定 OpenAI 官方清单 `persistent.oaistatic.com/codex-app-prod/windows-store-update.json`，读取动态四段版本并由本地代码构造对应架构的完整 `ChatGPT-{arch}.msix` 地址。
- ChatGPT 路径不调用 Microsoft Store、WinGet、Desktop App Installer、`get.microsoft.com` 引导器、FE3 私有协议或账户登录；schema、Identity、URL、签名、Publisher、Package Family、版本或架构失败均硬失败且不切换备用通道。
- 版本化嵌入式 `trust-registry`，远端响应只能在固定边界内提供易变版本和地址。
- HTTPS、逐跳 host/path 校验、最多五次重定向、4 MiB 元数据限制、2 GiB 安装包限制。
- 每次随机私有暂存目录、`.part` 写入、拒绝路径逃逸/符号链接/Windows reparse point。
- SHA-256、CC Switch minisign 流式验证接口、Windows Authenticode/AppX 验证、EXE PE 架构和 MSIX manifest 身份读取。
- 安装启动前再次核对稳定路径、长度、摘要和平台签名；MSI/EXE/MSIX 使用结构化进程参数，不拼接 shell 命令。
- PowerShell 与 `msiexec` 通过 Windows 系统目录 API 解析为受信任绝对路径，避免当前目录或普通 `PATH` 劫持。
- Windows 正式版使用 GUI subsystem，所有本地检测、签名校验、PowerShell、MSI/MSIX/EXE 安装子进程统一设置 `CREATE_NO_WINDOW`；安装程序自身的图形界面或 UAC 提示仍按其正常行为显示，但不再创建黑色命令行窗口。
- ChatGPT 下载完成后要求 AppX 签名有效，包内 Identity=`OpenAI.Codex`、Publisher、架构和四段版本与本地固定合同/官方清单一致；安装后再次核对 Package Family、Identity、Publisher、架构和版本。清单未提供独立 SHA 时不放宽任何 AppX 信任根。
- UI 安装/更新前确认、顺序批次、下载取消、验证、安装、postcheck、单项失败隔离、批次摘要与完成后自动刷新已接到真实执行路径。
- 直接包安装器退出码为 0 后进行最多约 90 秒的有界版本复检；错误 Package Family/架构立即失败，目标尚未登记或仍是旧版本则继续等待，超时收敛为 `ResultUnknown` 并禁止自动假刷新。WorkBuddy 还会从受信任注册项的 InstallLocation、DisplayIcon 或 UninstallString 定位固定的 `WorkBuddy.exe`，最终主程序不是 x64 时硬失败，文件尚未出现时才继续等待。
- 安装批次状态自动追加到 `%LOCALAPPDATA%\AI Client Installer\logs\operations.jsonl`；日志只保留状态切换和最终错误，脱敏 URL/用户目录/临时目录，并在 1 MiB 时轮换一份 previous 文件。日志不可用不会阻断安装，但会在批次摘要中明确提示。
- 已装更高版本、受组织管理、管理状态未知或现有版本未知时在下载前失败关闭；相同版本不重复安装。
- 中文字体、真实检测/解析后台线程和失败关闭界面。
- 客户端下载页已按参考稿重构为纯白居中布局：默认内容区 `800×610`、最小 `720×560`，标题/副标题、五个客户端单行列表、官方产品图标、紧凑版本状态和右侧“安装/更新”按钮；默认尺寸下五项完整显示且无多余滚动条，底部只保留当前平台与刷新入口。
- 安装/更新确认已改为带轻遮罩的页内紧凑模态卡片，展示产品、架构与官方来源，保留明确的“返回/开始”二次确认，不再出现独立子窗口。
- macOS 平台执行链已接入真实代码路径：Universal 进程用 `hw.optional.arm64` 判断物理硬件，避免 Apple Silicon 在 Rosetta 下误下 Intel 包；同时读取 macOS 版本并执行产品最低版本门禁。
- macOS 检测只检查 `/Applications` 与 `~/Applications`，要求固定应用名/Bundle ID，读取包内版本和主 Mach-O slice，并执行 `codesign --verify --deep --strict` 与 `spctl --assess --type execute`。
- macOS 安装支持 DMG、ZIP、tar.gz：DMG 只读挂载；归档在展开前限制条目数、总大小、路径穿越、重复路径和逃逸链接，展开后再次验证链接边界；只接受一个固定名称的 `.app`。
- macOS 默认安装到 `~/Applications`；若已存在一份可信应用则原位更新，同时发现用户级/系统级两份时拒绝猜测。新 app 先复制到目标卷私有暂存目录并复验，再原子替换；最终复验失败会恢复旧版，恢复失败则保留备份而不是删除。
- 五产品 macOS 解析合同已编码：WorkBuddy Intel/Apple Silicon 官方 ZIP+SHA-256，Hermes Apple Silicon 官方 DMG，CC Switch 双架构同一 minisign tar.gz，Claude Universal DMG 稳定重定向，ChatGPT Intel/Apple Silicon 官方 Sparkle appcast ZIP。OpenAI 官方支持下限按 macOS 14 执行，不采纳 appcast 中更宽松的 12.0 作为用户支持承诺。
- 自家 macOS 打包脚本生成一个包含 `x86_64` 与 `arm64` slice 的 Universal `.app`/DMG，构建时写入 Cargo 版本，执行 codesign、notarytool、staple、Gatekeeper 和 SHA-256。

## 2026-08-02 本机观察

环境：Windows 11 Pro build 26100，x64。

| 产品 | 检测结果 | 官方解析结果 | 当前动作 |
|---|---|---|---|
| Hermes Agent | 未检测到注册安装 | `0.19.1` | 可安装；运行官方签名 bootstrap，完成后复检桌面身份 |
| Claude Desktop / Code | 已安装 `1.24012.1.0` | `1.24012.9` | 可更新；直接下载官方 MSIX，验证 Publisher/Family 后执行 Add-AppxPackage |
| ChatGPT | 已安装 `26.721.11231.0`，identity `OpenAI.Codex`，family `OpenAI.Codex_2p2nqsd0c76g0`，Publisher `CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B`，x64 | `26.727.6591.0` | “更新”可执行；直连完整 x64 MSIX 并在结束后精确复检 |
| WorkBuddy | 已安装 `5.1.7`，注册项 `WorkBuddy 5.1.7`，Publisher 为腾讯，`WorkBuddy.exe` 为 x64，HKCU | `5.3.8.34705286` | “更新”可执行；验证官方 x86 bootstrap，安装后要求最终 `WorkBuddy.exe` 为 x64 且达到目标版本 |
| CC Switch | 已安装 `3.17.0` | `3.19.1` | 可更新；验证官方 Tauri minisign、MSI 产品身份与架构 |

这些版本是当日在线观察，不是代码常量。ChatGPT 的最新版本来自 OpenAI 固定官方清单；本地只固定清单位置、release URL 合同和包身份信任根。

## WorkBuddy 当前失败与只读 proof

- 用户在 2026-08-02 03:45 点击“更新”后，操作日志明确记录：下载完成、进入验证、随后因 `unsupported PE machine 0x014c` 失败。流程没有到达 `AwaitingUserInstall`/`Installing`，因此没有启动安装器。
- 当前官方更新接口返回 `5.3.8.34705286` 的 `win32-x64-user` EXE，407,285,928 bytes，SHA-256 `C111BC3F54A0E53FA04924313AE660125EEBFFAFCD5AC7722DA7C3C03402CB7A`。只读检查确认 PE machine 为 x86、NSIS 标记存在、Authenticode 有效、Signer 为腾讯、ProductName 为 `WorkBuddy`。
- 本机现有 `WorkBuddy.exe` 为 x64、`Uninstall WorkBuddy.exe` 为 x86，证明厂商将 x86 安装/卸载外壳与 x64 主程序分开。当前官方包已通过修改后的同一套 Rust 验签/身份/bootstrap machine 规则；本机注册检测也通过最终 `WorkBuddy.exe` x64 proof。
- 本轮没有运行该 407 MB 安装包。真实单动作更新仍需 Windows x64 可丢弃快照关闭 Gate。

## ChatGPT 当前非执行 proof

- Windows AppX 部署日志记录多个本地完整 MSIX 成功 Add/Update，包括 `Codex-Windows-x64.msix`、`ChatGPT-Windows-x64-26.721.3996.0.msix` 和 `ChatGPT-26.721.11231.0-x64.msix`；核心成功路径是本地文件交给 `Add-AppxPackage`，不需要 Store UI 或登录。
- 已安装且签名有效的 OpenAI 客户端内置生产更新清单 `https://persistent.oaistatic.com/codex-app-prod/windows-store-update.json`，并按 `releases/{buildVersion}/ChatGPT-{process.arch}.msix` 构造完整包地址。
- 2026-08-02 只读请求返回 schema `1`、build `26.727.6591.0`、identity `OpenAI.Codex`。对应 x64 与 ARM64 MSIX 均返回 HTTP 200、`application/vnd.ms-appx`；x64 为 759,477,276 bytes，ARM64 为 753,464,470 bytes。
- 本机 `Add-AppxPackage` 同时支持 `Path` 和 `ForceTargetApplicationShutdown`。实现保留系统默认升级/降级规则，没有启用 `ForceUpdateFromAnyVersion`；安装前仍拒绝降级，下载后要求包内版本与清单完全一致。
- 本次开发没有下载完整 700+ MiB 包，也没有执行 ChatGPT 安装/更新；仓库与 `dist/` 不保存 OpenAI 安装包。
- 实际 GUI 联网扫描显示 `已安装 26.721.11231.0 · 可更新至 26.727.6591.0`，仍只有一个“更新”按钮；确认卡显示 `X64 · Msix · persistent.oaistatic.com`。检查后点击“返回”并关闭程序，没有点击“开始”。
- 当前开发检查通过：格式、全部 target 编译、Clippy `-D warnings`、70 项自动测试，另有 3 项环境型 proof 默认忽略；当前主机检测 proof 已显式通过并确认 Hermes `0.19.1`、WorkBuddy `5.3.8` 与最终 x64 主程序。
- 当前 Windows x64 未签名测试产物为 `dist/AI-Client-Installer-windows-x64.exe`，10,597,888 bytes，SHA-256 `d7f7e6e3fac236ec67d0600a19f1f5cd014b42c9f746c61a394e1cabe9afde17`。

## 验证待完成

- Windows x64 干净机：优先对 WorkBuddy 补齐单动作更新、bootstrap 退出结果、最终 `WorkBuddy.exe` x64 与注册版本 proof；其余直接包补齐交互安装/取消和安装后身份/版本矩阵；ChatGPT 补齐完整 MSIX 首次安装、旧版更新和无 Store/WinGet/引导器/登录窗口监控。
- Windows ARM64：release cross-build 已尝试；需补装 Visual Studio C++ ARM64/clang-cl 编译组件后重新构建，并完成签名与 ARM64 真机矩阵。
- macOS：在 Intel Mac 与 Apple Silicon Mac 上提取每款当前官方 app 的精确 Developer Team ID/最终应用名并固定；完成首次安装、旧版更新、双安装冲突、运行中应用、权限不足、断网/磁盘不足、回滚、quarantine 与 Gatekeeper 矩阵。Hermes Intel 已按厂商策略明确不支持；Hermes Apple Silicon 还需证明小型 DMG bootstrap 的最终桌面/runtime 状态。
- macOS 自家制品：私有 GitHub `macos-15-intel` Runner 已实际执行 `packaging/build-macos.sh`，测试、Clippy、两架构 release 编译、Universal 合并、ad-hoc codesign、DMG 生成和上传全部通过。下载后的 DMG 为 9,997,973 bytes，SHA-256 `3b19873c73339222709055d6e157f7b4a6bbc2d5838e58df2e5360a3302a2963`。尚未使用 Apple Developer ID/notary，也未在 Intel/Apple Silicon 实机启动。
- ChatGPT Windows：x64/ARM64 干净机真实首次安装与旧版更新、运行中应用关闭行为、网络/磁盘/AppX 失败分类、Store/WinGet/引导器/登录窗口未启动监控和最终身份/版本 Gate。
- 正式发布签名：Windows Authenticode 证书与 Apple Developer ID/notary 凭据未提供。

因此当前状态是：`Windows x64 implementation complete; macOS Universal validation DMG built, but all installable vendor entries remain validation-gated; clean-machine and release-signing validation pending`。当前 EXE 与 Mac DMG 都是开发验证产物；尚未生成 Apple Developer ID 签名并公证的正式 Mac DMG。
