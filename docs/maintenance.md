# 维护手册

## 维护目标

本项目不承诺厂商接口永不变化。维护目标是：版本与短期下载地址可动态变化，信任根和安装行为只能通过代码审查后的本地发布更新改变；任何无法证明的变化都失败关闭。

## 日常检查

1. 运行 `cargo test --all-targets` 和 `cargo clippy --all-targets -- -D warnings`。
2. 启动开发版，确认五个适配器的当前官方版本解析结果。
3. 对解析失败的产品只更新其独立 adapter fixture，不修改共享安全边界来“兼容所有情况”。
4. 检查 `config/trust-registry.toml` 中目标平台是否仍保持正确 enabled/disabled 状态。

## 厂商变化处理

- 版本字段或页面结构变化：更新对应 `src/adapters/<product>.rs` 和 fixture，保留旧格式失败测试。
- 下载 host/path 变化：必须取得官方一手证据；新增按 host 的精确路径规则，不用全局 `/` 放宽。
- 签名证书、MSIX Publisher/Family、macOS Team ID、CC Switch minisign key 或 ChatGPT Sparkle `SUPublicEDKey` 变化：视为信任根轮换，必须独立安全复核并重新完成目标平台 proof。
- 包类型变化：不在运行时自动切换 EXE/MSI/MSIX/DMG；先更新任务证据、验证策略和安装后检测。
- ChatGPT Windows：主路径固定下载 OpenAI 官方文档列出的微软轻量安装器 `9PLM9XGG6VKS`。每次操作重新下载，验证 Microsoft Authenticode、`MSStoreTag001` 签名配置中的 Product ID、PFN、`WindowsUpdate`、`isHarbor=true` 与 `autoUpdate=false` 后启动；不再探测、修复或升级 WinGet/App Installer。
- ChatGPT 完整包兜底：微软安装器因网络、更新服务、分发服务不可用或返回 Windows Installer `1612/0x64C`（安装源不可用）时，下载固定架构的 `ChatGPT-x64.msix` / `ChatGPT-arm64.msix` 与 `ChatGPT-License.xml`。必须验证 Store 签名、Identity、Publisher、架构、版本、无额外框架依赖，以及许可证 Product ID、PFM、`Full/Offline`、`LeaseRequired=False`，再通过 UAC 管理员部署。用户取消、UAC 拒绝、组织策略、安全验证、身份、架构或许可证合同错误不得触发兜底。
- Claude Windows/macOS：Windows 官方入口固定为 Anthropic 公开的 x64/ARM64 完整 MSIX 重定向，macOS 固定为 Universal DMG 重定向。Windows 直连或香港回退候选都必须验证 SHA-256（回退）、AppX 签名、Publisher、Identity、架构和版本，再由本地固定 PowerShell 通过原生 `runas` 执行 `Add-AppxProvisionedPackage -Online -SkipLicense -Regions all`；不得启动 Setup 或执行服务器返回的参数。安装后继续复检 Claude Package Identity/Family/Publisher/架构/版本；macOS 继续验证 Bundle/Team/架构/版本。只有地区不可用、403/451、408/429、连接/超时/响应体中断或服务端错误可进入受验证回退；安全白名单、未知重定向、清单/签名或平台身份错误必须硬失败。元数据连接/超时/响应中断最多重试三次，证书错误和 HTTP 合同错误不得重试。
- 系统命令定位：不得恢复裸 `powershell.exe` 或 `msiexec.exe` 调用。PowerShell/msiexec 必须继续由 Windows 系统目录 API 解析；ChatGPT 离线部署与 Claude 机器级 MSIX 部署的权限提升必须使用固定本地脚本和原生 `runas`，不得执行服务器返回的命令。
- Windows 后台进程：所有检测、校验、下载和安装命令必须通过共享的无控制台窗口启动边界设置 `CREATE_NO_WINDOW`，不得在单个调用点自行遗漏或恢复裸 `Command::spawn/status/output`。该设置只隐藏命令行控制台，不得用于规避安装程序正常的 GUI、UAC 或用户确认。
- Windows 本机检测：完整刷新固定分为两个相互隔离的有界探针。AppX 探针只查询 Claude/ChatGPT 的固定 Identity、Family 和 `Main` 包；注册表探针只读取 WorkBuddy/Hermes/CC Switch 候选卸载项及 Hermes 固定目录。每个系统 PowerShell 最多运行 20 秒，必须使用 UTF-8 JSON 输出并保留 AppX/注册表真实错误；禁止恢复五产品各自枚举全部 AppX、无超时 `.output()` 或 `SilentlyContinue` 后把错误当成未安装。联网版本解析仍按产品并行，不能覆盖已经得到的本机安装状态。
- Windows 卸载项检测：版本变化必须由本地可测试规则吸收，不得用不受约束的前缀匹配。WorkBuddy 只接受基础名称或纯数字点分版本后缀，并同时要求腾讯 Publisher；变更名称或 Publisher 属于检测身份合同变化，需要 fixture/live 只读证据。
- Windows EXE bootstrap：目标设备架构与安装器外壳 PE machine 是不同信任事实。默认两者必须一致；只有本地 trust entry 可固定不同的 `windows_exe_machine`，且跨架构 bootstrap 必须同时固定单文件名 `postinstall_executable`。不得在 verifier、orchestrator 或 adapter 中全局允许 x86 EXE 运行于 x64 目标。
- WorkBuddy 更新包：当前 Windows x64 官方通道固定为 x86 NSIS bootstrap，安装后固定检查 `WorkBuddy.exe`。若 bootstrap machine、最终主程序文件名、Signer、ProductName 或注册项路径形态变化，必须重新取得官方包只读 proof、更新测试并完成干净机更新验证，不能静默兼容。
- 直接包 postcheck：安装器退出 0 后允许注册表/AppX 状态短暂延迟，当前上限约 90 秒。错误身份或架构立即失败；未登记、版本未知、最终主程序尚未出现或仍为旧版本只在时间窗内重试，超时必须是 `ResultUnknown`。已存在但不是可解析目标架构 PE 的固定主程序属于确定性失败，不得伪造成功。
- 操作日志：Windows 默认位置为 `%LOCALAPPDATA%\easy agent\logs\operations.jsonl`。只记录状态变化/终态，重复下载进度不落盘；完整 URL、用户目录和临时目录必须脱敏；1 MiB 时只保留一个 previous 文件。日志写入失败不能改变安装结果，但必须回到批次摘要。
- 并发边界：每个产品必须拥有独立状态、取消令牌、进度和错误；下载与校验可以并行。厂商 EXE 与 macOS 应用复制不占全局许可，可以直接并行；Windows MSI 只在 MSI 通道内 FIFO，MSIX 与 Microsoft Store 只在 AppX 通道内 FIFO，两条通道互相独立。系统安装及紧随其后的 postcheck 必须共同持有对应通道许可；排队取消不得启动安装器或写入系统。
- 本地威胁边界：私有暂存、`O_NOFOLLOW`/reparse 检查和稳定文件绑定用于阻止下载内容、链接与路径竞态逃逸；已经能以同一登录用户并发执行任意本地代码的进程不属于本工具可提供的隔离边界，应由操作系统账户隔离和终端安全负责。下载重试建立文件句柄后不得再按路径重新打开 `.part`。面向用户的安装包副本只能在完整平台验证后写入系统“下载”目录，复制时重新计算长度和 SHA-256，并在复制前后再次绑定私有源文件；执行器不得改为运行该公开副本。同名不同内容必须使用摘要后缀，不能覆盖用户文件。
- ChatGPT 部署：轻量安装器或完整包部署退出码 0 都不是最终成功，仍必须通过固定 Package Family/Identity/Publisher/架构/版本 postcheck。已进入系统部署后超时不得杀死进程，只能报告“结果待复检”。
- macOS 架构：安装助手自身是 Universal，但下游包选择必须使用物理硬件判断，不能直接用当前进程 slice；Apple Silicon 即使在 Rosetta 下启动也应选择 ARM64 包。Hermes Intel 必须保持 unsupported。
- macOS 包合同：WorkBuddy 使用更新接口的架构 ZIP；CC Switch 使用 `latest.json` 的 minisign tar.gz；Claude 使用 Universal DMG 稳定重定向；ChatGPT 分别读取 `appcast.xml` 和 `appcast-x64.xml`，要求最低 macOS 字段为数字点分版本，并把官方最新版本声明与当前系统版本比较。信任注册表保存当前已验证的支持基线（现为 macOS `13.0`），不能再把上游最低版本写死为一个永不变化的常量。完整包长度、架构文件名和 64-byte Ed25519 签名仍必须固定验证。ChatGPT 只有在官方元数据请求或完整包下载发生明确连接/超时/响应中断/403/429/451/服务端失败时才能进入固定香港回退；回退必须与用户已确认的版本、架构、包型、最低系统版本、大小和 OpenAI Sparkle 签名完全一致，清单还要通过 minisign、最大陈旧期、不可变路径和 SHA-256，镜像同步端还必须确认 appcast 与 ZIP 内 `LSMinimumSystemVersion` 一致，再由客户端继续验证 Apple Team/Bundle、版本、架构、codesign 与 Gatekeeper。合同/白名单/证书/签名/大小错误不得回退，也不得猜网页链接或使用第三方通用镜像。
- Anthropic 现已在 macOS 企业部署文档中推荐 Universal PKG。当前项目仍采用面向普通用户、无需整体 root 运行的 DMG 应用包策略；除非单独实现并验证 PKG 签名、安装收据、按需提权、失败恢复和复检链，否则不要把 PKG URL直接塞进现有 DMG 适配器。
- WorkBuddy macOS 摘要策略：厂商接口当前给出的 Intel/Apple Silicon SHA-256 均与同 URL 下载的完整 ZIP 不一致，所以这两个固定条目使用 `platform_signature_only`。远端摘要仍必须解析、比较并记录警告；下载完成后仍以实际 SHA-256、规范路径和长度形成稳定文件身份，并在 Apple 平台验证前后重复核对。只有 WorkBuddy、macOS、ZIP、`direct_app_bundle`、Bundle ID `com.workbuddy.workbuddy`、Team ID `FN2V63AD2J` 的组合可使用该策略，其他条目仍按默认摘要策略失败关闭。
- WorkBuddy macOS 运行日志：厂商应用首次运行会在固定 `tencent-docs-ai-engine/bin/darwin-{x64|arm64}/editor_sdk.log` 路径新增日志，破坏资源封印。只允许已安装状态扫描对这一条普通、不可执行、大小受限的日志做专用识别，并要求 codesign 严格失败输出中没有其他新增或修改，随后以 `--ignore-resources` 重新验证根代码签名。候选包、安装暂存、最终激活和其他产品仍必须通过完整深度 codesign；最终激活继续通过 Gatekeeper，不得推广该例外。
- macOS 安装：只检查用户/系统 Applications；首次安装优先使用可写的系统 `/Applications`，无权限时才使用 `~/Applications`，已有可信应用保持原位置。归档路径、展开大小和链接先后双重校验，DMG 只读挂载；固定应用名、Bundle ID、Developer Team ID、主 executable slice、updater 签名、完整代码签名和 Gatekeeper 全部通过后才能复制，最终应用还必须成功注册 LaunchServices。下载前检查目标目录权限和运行中主进程，不得清除 quarantine。
- macOS 更新：同名但错误 Bundle ID、同时存在两份应用、不可写的系统 Applications 或无法确认签名时都失败关闭。替换前保留同卷备份；最终复验失败必须回滚，回滚失败必须保留备份位置并报告，不能自动清理旧版。
- macOS 系统命令：元数据命令默认 30 秒，codesign/Gatekeeper 120 秒，LaunchServices 60 秒，DMG 5 分钟，解压与复制 15 分钟。超时必须终止独立进程组并 `wait` 回收，不能只杀父进程或留下孤立 `spctl`/`codesign`。日常状态扫描只做固定 Bundle/Team/版本/架构与根签名检查；深度 codesign 和 Gatekeeper 保留在候选包与最终激活边界，避免刷新状态重复执行完整安装级检查。

## 启用一个 trust entry

启用前必须同时具备：

- 官方入口和每跳 host/path 合同；
- 正确架构与包类型；
- 固定产品身份；
- Authenticode/AppX Publisher/Package Family、minisign key 或 macOS Team ID 等平台信任根；
- 下载、验证、交互安装/取消、安装后版本与身份复检的干净机证据；
- 已装更高版本、受管或管理状态未知时的失败关闭证据。

ChatGPT Windows 条目还必须同时具备：

- 固定 Store ID、微软安装器签名与产品绑定、Package Identity/Family/Publisher 与支持架构；
- 固定 x64/ARM64 MSIX 和离线许可证合同；
- x64/ARM64 干净机首次安装、旧版更新、主路径网络/服务失败自动兜底、UAC 取消、安全失败不兜底和 postcheck 证据。缺少真机证据时只能标记 validation pending。

macOS 条目还必须同时具备：

- 显式 `macos_install_strategy`；当前只有 `direct_app_bundle` 可启用，`vendor_bootstrap` 必须有独立执行器、下游信任边界和最终状态 postcheck；
- `remote_digest_policy` 默认保持 `enforce_if_present`。只有上文固定的 WorkBuddy macOS ZIP 身份可以使用 `platform_signature_only`，不得复制到其他产品、平台、Bundle ID 或 Team ID；
- 当前官方完整包在对应 Mac 上的 `CFBundleIdentifier`、`CFBundleExecutable`、版本、Developer Team ID 和架构 slice 证据；
- 配置了 updater 公钥时，完整文件的 minisign 或 Sparkle Ed25519 成功证据；
- `codesign --verify --deep --strict` 与 `spctl --assess --type execute` 成功证据；
- 首次激活、更新替换、首次失败清理、最终复验失败回滚、运行中应用和权限失败的自动或临时目录 proof；
- Intel 与 Apple Silicon 候选包分别通过生产 verifier。真实干净机 UI 矩阵未完成时，README/Release 必须继续标记 validation pending，不得宣称生产就绪。

条目的官方合同、生产 verifier 和安全激活 proof 完成后才可把对应 `enabled` 改为 `true`；正式发布前还要补齐可回滚真机矩阵。不得提供用户侧“跳过验证”开关。

## macOS 制品只读取证

在真实 Mac 上取得官方包后，可用开发示例复用生产验证链读取 Bundle、Team ID、版本和架构；该示例会执行归档边界、updater 签名、codesign 与 Gatekeeper 检查，但不会复制或启动目标应用，也不会进入最终 DMG：

```bash
MACOS_PROOF_SIGNATURE="$(jq -r '.platforms[\"darwin-x86_64\"].signature' /private/tmp/latest.json | openssl base64 -d -A)" \
cargo run --example macos_artifact_proof -- \
  cc_switch x64 /private/tmp/CC-Switch-macOS.tar.gz
```

也可以让 proof 从当前已启用的官方元数据解析并下载到随机临时目录，完成后自动清理；该模式仍不会安装：

```bash
cargo run --example macos_artifact_proof -- workbuddy x64 --download-verify
cargo run --example macos_artifact_proof -- workbuddy arm64 --download-verify
cargo run --example macos_artifact_proof -- chatgpt x64 --download-verify
cargo run --example macos_artifact_proof -- cc_switch arm64 --download-verify
```

对已安装应用只执行签名/身份检测或安装前运行状态与权限检查：

```bash
cargo run --example macos_artifact_proof -- chatgpt x64 --installed
cargo run --example macos_artifact_proof -- chatgpt x64 --preflight
```

尚未写入注册表的应用名、Bundle ID 或 Team ID 只能作为本次证据的显式预期值传入，不能由远端响应自动扩展：

```bash
MACOS_PROOF_APPLICATION_NAME='WorkBuddy.app' \
MACOS_PROOF_BUNDLE_ID='com.workbuddy.workbuddy' \
cargo run --example macos_artifact_proof -- \
  workbuddy x64 /private/tmp/WorkBuddy.zip
```

探针成功只证明当前制品通过只读平台检查，不等价于首次安装、更新、回滚或双架构真机 Gate。维护探针和证据采集单独下载的第三方包必须留在仓库外的私有临时目录，并在证据记录完成后清理；这不影响桌面程序按用户要求保存已验证副本到系统“下载”目录。

同一示例也可以按嵌入式信任条目只读检查当前 Applications 中的精确安装：

```bash
cargo run --example macos_artifact_proof -- chatgpt x64 --installed
```

## 发布

Windows：

```powershell
.\packaging\build-windows.ps1 -Architecture x64
.\packaging\build-windows.ps1 -Architecture arm64
```

macOS 上的等价内部验证构建：

```bash
brew install llvm
cargo install cargo-xwin --locked
./packaging/build-windows-from-macos.sh all
```

正式发布还需对 EXE 做 Authenticode 签名并重新生成 `SHA256SUMS.txt` 与 `release-manifest.json`。

macOS：在具备 Xcode Command Line Tools、Developer ID Application 和 notarytool profile 的 macOS 上运行：

```bash
APPLE_SIGN_IDENTITY='Developer ID Application: ...' \
APPLE_NOTARY_PROFILE='easy-agent-notary' \
./packaging/build-macos.sh
```

缺签名/notarization、ARM64/macOS 真机或 clean-machine 安装证据时，版本状态只能是 `validation_pending`。正式和内部 DMG 均应包含 `easy agent.app` 与 `/Applications` 拖拽入口；脚本不得通过清除 quarantine 或修改 Gatekeeper 策略来补偿缺失的发布签名。

仅用于内部验证的无凭据构建：

```bash
ALLOW_UNSIGNED_MACOS_BUILD=1 ./packaging/build-macos.sh
```

该模式只做 ad-hoc codesign，并强制生成带 `UNNOTARIZED-VALIDATION` 的 Universal DMG/哈希，不执行 notarization 或 Gatekeeper 通过声明；不得改成正式文件名或放入终端用户下载页。`.github/workflows/build-macos-validation.yml` 使用同一模式，适合在私有 GitHub macOS runner 上生成短期验证产物。

## 仓库卫生

- 不提交五款第三方安装包、`.part`、临时 CDN URL、真实用户日志、证书私钥或 notary 凭据。
- `dist/` 只保留本项目生成的发行制品、checksum 和 manifest。
- 发布前确认父级 Obsidian 知识库仍忽略该独立嵌套仓库，避免污染用户已有工作区。
