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
- ChatGPT Windows：只有完整 Microsoft 目录/FE3 依赖闭包、摘要、签名、授权和无 Store UI 直接安装再次通过，才允许启用。

## 启用一个 trust entry

启用前必须同时具备：

- 官方入口和每跳 host/path 合同；
- 正确架构与包类型；
- 固定产品身份；
- Authenticode/AppX Publisher/Package Family、minisign key 或 macOS Team ID 等平台信任根；
- 下载、验证、交互安装/取消、安装后版本与身份复检的干净机证据；
- 已装更高版本、受管或管理状态未知时的失败关闭证据。

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

## 仓库卫生

- 不提交五款第三方安装包、`.part`、临时 CDN URL、真实用户日志、证书私钥或 notary 凭据。
- `dist/` 只保留本项目生成的发行制品、checksum 和 manifest。
- 发布前确认父级 Obsidian 知识库仍忽略该独立嵌套仓库，避免污染用户已有工作区。
