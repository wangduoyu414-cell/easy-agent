# macOS Universal 实现与验证证据

日期：2026-08-03

## 结论

- 自家安装助手可以只交付一个 macOS Universal DMG；同一可执行文件包含 `x86_64` 与 `arm64`。
- 运行时使用 `hw.optional.arm64` 判断物理硬件，避免 Apple Silicon 在 Rosetta 下误选 Intel 厂商包。
- Mac 检测、验证、安装、回滚和 postcheck 已接入生产代码；厂商条目仍失败关闭，未在真实 Mac 固定 Team ID 和完成安装矩阵前不启用。
- Hermes 是例外：同一个安装助手能在 Intel Mac 运行，但 Hermes 官方客户端在 Intel Mac 上显示不支持。

## 当前官方动态合同快照

| 产品 | Intel | Apple Silicon | 运行时入口/包型 |
|---|---|---|---|
| WorkBuddy | 独立 ZIP | 独立 ZIP | `www.workbuddy.cn/v2/update?platform=workbuddy-darwin-{x64|arm64}`；响应含 SHA-256 |
| Hermes | 厂商不支持 | DMG bootstrap | 官网解析 `hermes-assets.nousresearch.com/Hermes-Setup.dmg` |
| CC Switch | 同一 Universal tar.gz | 同一 Universal tar.gz | `dl.ccswitch.io/latest.json`；每个平台项带 minisign |
| Claude Desktop | Universal DMG | Universal DMG | `claude.ai/api/desktop/darwin/universal/dmg/latest/redirect` |
| ChatGPT | 架构 ZIP | 架构 ZIP | `appcast-x64.xml` / `appcast.xml`；OpenAI 文档要求 macOS 14 |

2026-08-03 在线读取到 WorkBuddy `5.3.8.34705286`、CC Switch `3.19.1`、Hermes `0.19.1`、ChatGPT `26.727.51351`。这些值仅是快照；代码只固定入口、host/path、包型与身份边界。

## 已执行检查

```text
cargo test --all-targets
  82 passed, 5 ignored after macOS tests were added

cargo test --test resolver_fixtures live_macos_metadata_contracts_parse_from_pinned_official_entries -- --ignored
  passed against current WorkBuddy, CC Switch, Hermes and ChatGPT official metadata

cargo clippy --all-targets --all-features -- -D warnings
  passed on Windows x64

cargo check/clippy --target x86_64-apple-darwin
  passed

cargo check/clippy --target aarch64-apple-darwin
  passed
```

覆盖的自动检查包括：Mac appcast/manifest 架构映射、最低系统门禁、Hermes Intel unsupported、Mach-O thin/universal 解析、归档路径/链接逃逸拒绝、Team ID 解析、旧版回滚，以及 Windows 全量回归。

## Universal DMG 实际构建结果

- 私有 GitHub Actions run：`30815608883`，Runner：`macos-15-intel`。
- 测试、Clippy、`x86_64-apple-darwin`/`aarch64-apple-darwin` release 编译、`lipo` 合并、应用与 DMG ad-hoc codesign 校验、制品上传均通过。
- `lipo -info` 返回 `x86_64 arm64`。
- 本地落盘：`dist/AI-Client-Installer-macos-universal.dmg`，9,997,973 bytes。
- SHA-256：`3b19873c73339222709055d6e157f7b4a6bbc2d5838e58df2e5360a3302a2963`，下载后重新计算并与 CI 文件一致。
- 此包未使用 Apple Developer ID、未 notarize/staple，是内部验证包，不是无 Gatekeeper 警告的正式发行包。

## 真实 Mac 待验项

- Intel 与 Apple Silicon 各一台：自家 Universal app 实际启动；补 Developer ID 签名、公证、staple 与 Gatekeeper 验证。
- 每个厂商当前包的最终应用名、Bundle ID、Developer Team ID、主 executable slice 与版本。
- 首次安装、旧版更新、用户/系统 Applications、双安装冲突、应用运行中、权限不足、网络/磁盘失败和回滚。
- Hermes Apple Silicon bootstrap 完成后，桌面应用与 runtime 分别达到什么状态。
- Claude 稳定重定向在真实 Mac 网络环境的可达性；本次 Windows 只读请求遇到 Cloudflare 403，不能据此宣称 Mac 运行时已通过。

正式状态：`Universal validation DMG built; vendor installation validation and release signing pending`。

## Windows 伴随回归产物

当前源码的 Windows x64 release build 已成功生成：

- `target/x86_64-pc-windows-msvc/release/ai-client-installer.exe`
- 8,938,496 bytes
- SHA-256 `59619e04862ae09ca4be1e464039d09d0a269593b3614735734a98a77d2e5a3d`

复制到标准 `dist/AI-Client-Installer-windows-x64.exe` 时，旧 dist 程序仍在运行并锁定自身文件，因此没有强制终止进程，也没有伪造 dist/checksum 已刷新。关闭该程序后重新运行 `packaging/build-windows.ps1 -Architecture x64` 即可更新标准发布路径。
