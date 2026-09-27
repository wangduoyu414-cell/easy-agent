# Clash Verge Rev Windows x64 信任链取证（2026-09-26）

## 结论

Clash Verge Rev 的 Windows x64 官方分发链已在本机完成下载与签名校验，信任条目以
`enabled = true` 写入 `config/trust-registry.toml`；arm64 与 macOS 条目保持
`enabled = false`，等待真机取证。

## 官方入口与清单合同

- 官方仓库：`clash-verge-rev/clash-verge-rev`（GitHub），发布由 github-actions 机器人完成。
- 更新清单（固定 URL，不随版本变化）：
  `https://github.com/clash-verge-rev/clash-verge-rev/releases/download/updater/update.json`
  该地址来自官方 `src-tauri/tauri.conf.json` 的 `plugins.updater.endpoints`。
- 清单结构：`name`（形如 `v2.5.6`）、`pub_date`、`platforms.{key}.{signature,url}`；
  **没有独立的 `version` 字段**，版本号由 `name` 去掉 `v` 前缀得到，且必须与下载 URL 中
  `/download/v{version}/` 一致（适配器已按此 fail-closed 校验）。
- 平台键：`windows-x86_64`（备用 `windows-x86_64-nsis`）、`windows-aarch64`、
  `darwin-x86_64`、`darwin-aarch64`。Windows 产物为 NSIS `*-setup.exe`，macOS 清单指向
  `*.app.tar.gz`（DMG 也随 Release 发布，但不在清单内）。
- 注意：逐版本的 `latest.json` 使用 `api.github.com` 资产 URL（无扩展名、需跳转），
  不作为解析依据；`updater/update.json` 提供稳定的 `github.com` 直链。

## 签名信任根

- 算法：minisign（Ed25519），与 CC Switch 相同，复用应用内 `minisign-verify` 链。
- 公钥来源：`src-tauri/tauri.conf.json` → `plugins.updater.pubkey`（base64 编码的
  minisign 公钥文档），key id `D28C2F0BBEF9BDDF`，已钉入 trust-registry。
- 签名随清单内嵌（`platforms.*.signature`，base64），也可从 Release 资产
  `*.sig` 单独取得，两者内容一致。

## 本机验证记录

- 样本：`Clash.Verge_2.5.6_x64-setup.exe`，59,572,259 字节，
  SHA-256 `6cecd32b684b22e05f8dcdd4fef772fb7becf199f9d069a40401ec5c0bbe1c`。
- 用与应用相同的 `minisign-verify` 0.2.5 代码路径验证上述公钥对安装包的签名：**VALID**。
- PE 机架构：安装包头部 Machine = `0x014c`（i386）。NSIS 引导程序是 32 位 PE，
  即使安装 64 位载荷——因此条目写作 `windows_exe_machine = "x86"`，并按交叉架构规则
  补齐 `postinstall_executable = "clash-verge.exe"`。

## 安装后身份（来自官方 NSIS 模板）

官方 `src-tauri/packages/windows/installer.nsi`（Tauri 占位符已按其配置代入）：

- 注册表项：`HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Clash Verge`
  （perMachine 安装）。
- `DisplayName` = `Clash Verge`，`Publisher` = `Clash Verge Rev`
  （来自 `bundle.publisher`），`DisplayVersion` = 版本号，
  `InstallLocation` = `$INSTDIR`（默认 `%ProgramFiles%\Clash Verge`）。
- 主程序：`clash-verge.exe`（Cargo 包名 `clash-verge`，未设置 `mainBinaryName`）。
- 本机检测据此要求 DisplayName 与 Publisher 同时匹配。

## 尚未完成

- 干净机器的真实安装 + 安装后复检 + 独立启动（规范要求，当前仅有下载/签名/模板合同证据）。
- Windows ARM64：官方已发布 `arm64-setup.exe`，需真机验证后启用。
- macOS：`tauri.macos.conf.json` 显示 bundle id `io.github.clash-verge-rev.clash-verge-rev`、
  最低系统 11.0；Team ID 与公证状态未取证，两条 macOS 条目保持禁用。

## 更新策略决定（2026-09-26，用户批准）

CVR 的 NSIS 模板为 perMachine 安装，注册表写在 HKLM。按本机检测规则 HKLM 安装
`management_known = false`，默认拒绝覆盖更新。已按 CC Switch MSI 的先例为该身份开启
`allow_trusted_update_when_management_unknown`：信任校验限定为
`clash_verge_rev` + 仅 EXE + `package_identity = "Clash Verge"` + 必须钉住 minisign 公钥，
注册表匹配要求 DisplayName 与 Publisher（`Clash Verge Rev`）同时命中，满足才允许覆盖。
否则组织管理的同名软件有被误覆盖的风险。
