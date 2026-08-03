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
- 签名证书、MSIX Publisher/Family、macOS Team ID 或 CC Switch updater key 变化：视为信任根轮换，必须独立安全复核并重新完成目标平台 clean-machine proof。
- 包类型变化：不在运行时自动切换 EXE/MSI/MSIX/DMG；先更新任务证据、验证策略和安装后检测。
- ChatGPT Windows：只维护固定官方清单 `persistent.oaistatic.com/codex-app-prod/windows-store-update.json` 和由本地代码构造的 `/codex-app-prod/releases/{四段版本}/ChatGPT-{x64|arm64}.msix` 合同。日常版本变化不改仓库；schema、清单路径、release host/path、`OpenAI.Codex` Identity、Package Family、Publisher 或架构变化属于兼容性/信任根事件，必须失败关闭、更新 fixture/proof 并独立复核。
- ChatGPT 完整 MSIX：官方清单当前不提供独立 SHA，可信根由固定 HTTPS host/path、有效 AppX 签名、精确 Publisher/Identity/Family/架构/包内版本、执行前二次文件绑定和安装后复检共同组成。不得因缺 SHA 放宽任何身份检查，也不得回退 Store、WinGet、引导器或第三方源。
- 系统命令定位：不得恢复裸 `powershell.exe` 或 `msiexec.exe` 调用。PowerShell/msiexec 必须继续由 Windows 系统目录 API 解析。任何新外部系统工具都必须先定义同等级别的本地信任定位方式；ChatGPT 路径不得重新引入 WinGet 或 Desktop App Installer。
- Windows 后台进程：所有检测、校验、下载和安装命令必须通过共享的无控制台窗口启动边界设置 `CREATE_NO_WINDOW`，不得在单个调用点自行遗漏或恢复裸 `Command::spawn/status/output`。该设置只隐藏命令行控制台，不得用于规避安装程序正常的 GUI、UAC 或用户确认。
- Windows 卸载项检测：版本变化必须由本地可测试规则吸收，不得用不受约束的前缀匹配。WorkBuddy 只接受基础名称或纯数字点分版本后缀，并同时要求腾讯 Publisher；变更名称或 Publisher 属于检测身份合同变化，需要 fixture/live 只读证据。
- Windows EXE bootstrap：目标设备架构与安装器外壳 PE machine 是不同信任事实。默认两者必须一致；只有本地 trust entry 可固定不同的 `windows_exe_machine`，且跨架构 bootstrap 必须同时固定单文件名 `postinstall_executable`。不得在 verifier、orchestrator 或 adapter 中全局允许 x86 EXE 运行于 x64 目标。
- WorkBuddy 更新包：当前 Windows x64 官方通道固定为 x86 NSIS bootstrap，安装后固定检查 `WorkBuddy.exe`。若 bootstrap machine、最终主程序文件名、Signer、ProductName 或注册项路径形态变化，必须重新取得官方包只读 proof、更新测试并完成干净机更新验证，不能静默兼容。
- 直接包 postcheck：安装器退出 0 后允许注册表/AppX 状态短暂延迟，当前上限约 90 秒。错误身份或架构立即失败；未登记、版本未知、最终主程序尚未出现或仍为旧版本只在时间窗内重试，超时必须是 `ResultUnknown`。已存在但不是可解析目标架构 PE 的固定主程序属于确定性失败，不得伪造成功。
- 操作日志：Windows 默认位置为 `%LOCALAPPDATA%\AI Client Installer\logs\operations.jsonl`。只记录状态变化/终态，重复下载进度不落盘；完整 URL、用户目录和临时目录必须脱敏；1 MiB 时只保留一个 previous 文件。日志写入失败不能改变安装结果，但必须回到批次摘要。
- ChatGPT AppX 部署：只执行官方清单对应架构的单个完整 MSIX。包内版本必须与清单候选完全一致，安装前拒绝降级；`Add-AppxPackage` 退出 0 后仍必须通过固定 Package Family/Identity/Publisher/架构/版本 postcheck。未取得专门证据前不要启用 `ForceUpdateFromAnyVersion`。
- macOS 架构：安装助手自身是 Universal，但下游包选择必须使用物理硬件判断，不能直接用当前进程 slice；Apple Silicon 即使在 Rosetta 下启动也应选择 ARM64 包。Hermes Intel 必须保持 unsupported。
- macOS 包合同：WorkBuddy 使用更新接口的架构 ZIP 与 SHA-256；CC Switch 使用 `latest.json` 的 minisign tar.gz；Claude 使用 Universal DMG 稳定重定向；ChatGPT 分别读取 `appcast.xml` 和 `appcast-x64.xml`，只接受与 appcast 架构/版本一致的完整 ZIP。不得退回网页猜链接或第三方镜像。
- macOS 安装：只检查用户/系统 Applications；默认新装到 `~/Applications`。归档路径、展开大小和链接先后双重校验，DMG 只读挂载；固定应用名、Bundle ID、Developer Team ID、主 executable slice、完整代码签名和 Gatekeeper 全部通过后才能复制。不得清除 quarantine。
- macOS 更新：同名但错误 Bundle ID、同时存在两份应用、不可写的系统 Applications 或无法确认签名时都失败关闭。替换前保留同卷备份；最终复验失败必须回滚，回滚失败必须保留备份位置并报告，不能自动清理旧版。

## 启用一个 trust entry

启用前必须同时具备：

- 官方入口和每跳 host/path 合同；
- 正确架构与包类型；
- 固定产品身份；
- Authenticode/AppX Publisher/Package Family、minisign key 或 macOS Team ID 等平台信任根；
- 下载、验证、交互安装/取消、安装后版本与身份复检的干净机证据；
- 已装更高版本、受管或管理状态未知时的失败关闭证据。

ChatGPT Windows 直接条目还必须同时具备：

- 固定清单 exact path、release path prefix、Package Identity/Family/Publisher 与支持架构；
- schema、四段 AppX 版本、identity 和架构映射的成功/失败 fixture；
- 下载包签名、包内 identity/publisher/architecture/version 与候选版本的严格验证；
- x64/ARM64 干净机首次安装、旧版更新、无 Store/WinGet/引导器/登录窗口和 postcheck 证据。缺少真机证据时只能标记 validation pending。

macOS 条目还必须同时具备：

- 当前官方完整包在对应 Mac 上的 `CFBundleIdentifier`、`CFBundleExecutable`、版本、Developer Team ID 和架构 slice 证据；
- `codesign --verify --deep --strict` 与 `spctl --assess --type execute` 成功证据；
- 首次安装、旧版更新、用户/系统 Applications、权限不足、运行中应用和回滚结果；
- Intel 与 Apple Silicon 分别验证；Universal 厂商包也必须在两类机器各自复检。缺任一项时保持 `enabled = false`。

证据完成后才把对应条目的 `enabled` 改为 `true`。不得提供用户侧“跳过验证”开关。

## 发布

Windows：

```powershell
.\packaging\build-windows.ps1 -Architecture x64
.\packaging\build-windows.ps1 -Architecture arm64
```

正式发布还需对 EXE 做 Authenticode 签名并重新生成 `SHA256SUMS.txt` 与 `release-manifest.json`。

macOS：在具备 Xcode Command Line Tools、Developer ID Application 和 notarytool profile 的 macOS 上运行：

```bash
APPLE_SIGN_IDENTITY='Developer ID Application: ...' \
APPLE_NOTARY_PROFILE='ai-client-installer' \
./packaging/build-macos.sh
```

缺签名/notarization、ARM64/macOS 真机或 clean-machine 安装证据时，版本状态只能是 `validation_pending`。

仅用于内部验证的无凭据构建：

```bash
ALLOW_UNSIGNED_MACOS_BUILD=1 ./packaging/build-macos.sh
```

该模式只做 ad-hoc codesign 并生成 Universal DMG/哈希，不执行 notarization 或 Gatekeeper 通过声明；不得放入正式下载页。`.github/workflows/build-macos-validation.yml` 使用同一模式，适合在私有 GitHub macOS runner 上生成短期验证产物。

## 仓库卫生

- 不提交五款第三方安装包、`.part`、临时 CDN URL、真实用户日志、证书私钥或 notary 凭据。
- `dist/` 只保留本项目生成的发行制品、checksum 和 manifest。
- 发布前确认父级 Obsidian 知识库仍忽略该独立嵌套仓库，避免污染用户已有工作区。
