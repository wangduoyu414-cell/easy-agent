# macOS Intel 只读取证与信任根更新

日期：2026-08-04

## 范围与结论

- 环境：macOS 26.4.1，Intel `x86_64`，仓库 revision `52edae5`。
- 本轮只下载、挂载、展开和检查官方制品；没有安装、更新或启动任何第三方客户端。
- 所有第三方包均位于仓库外的私有临时目录，未写入 `dist/` 或 Git。
- 新增 `examples/macos_artifact_proof.rs`，复用生产代码的归档边界、minisign、codesign、Gatekeeper、Bundle 和 Mach-O 检查，且不执行安装。
- 已固定 CC Switch、Claude、ChatGPT、WorkBuddy 和 Hermes bootstrap 的已观察应用身份；所有 macOS 安装条目继续保持 `enabled = false`。
- 发现两个硬阻塞：WorkBuddy 官方 API SHA-256 与 CDN 文件不一致；Claude 稳定重定向当前被 Cloudflare 自动化挑战阻断。

## 安装助手自身

- 仓库既有 `dist/SHA256SUMS-macos-universal.txt` 校验通过；既有 DMG 与 app 的 ad-hoc codesign 校验通过，主程序同时包含 `x86_64` 与 `arm64` slice。
- 当前工作树的 x86_64 与 ARM64 Release 均重新构建成功，并在仓库外组装版本 `0.1.0` Universal app/DMG；未覆盖 `dist/`。
- 当前工作树临时 DMG 为 9,862,865 bytes，SHA-256 `6a868064610d26d592bf091512bf396364b31742027ab8cee8e6081756cc2cf3`；Universal 主程序 SHA-256 为 `f063469e59aaba4e431e6762f09d3248f9ac58a87e25f38c31f17037e1e1c776`。
- 临时 app 与 DMG 的 ad-hoc codesign、DMG 只读挂载、挂载后 app 复验和 `lipo -verify_arch x86_64 arm64` 均通过；Bundle ID 为 `local.aiclientinstaller.app`。
- Intel 主机直接启动当前工作树 DMG 内程序并持续运行 30 秒，随后由验证流程以 SIGTERM 终止（状态 143）；未触发任何厂商安装动作。
- `spctl` 对当前临时 app 返回状态 3 / `rejected`，符合 ad-hoc、未公证验证包的预期；这不是正式 Gatekeeper 通过证据。
- 临时构建目录和第三方只读取证目录均在最终检查后清理，不进入仓库或发布制品。

## CC Switch 3.19.1

- 官方入口：`https://dl.ccswitch.io/latest.json`。
- 制品：`CC-Switch-v3.19.1-macOS.tar.gz`，27,717,609 bytes。
- 本地 SHA-256：`aa172d981c1cdca58d143ce36232ad26956a4e9ee77e3ed4ae2349ceeb2a074b`。
- x64 与 ARM64 清单项指向同一制品，minisign 文本分别按清单解码后均验证通过。
- 应用名：`CC Switch.app`。
- Bundle ID：`com.ccswitch.desktop`。
- Team ID：`R8UR22V2F9`。
- 版本：`3.19.1`。
- 主程序同时包含 `x86_64` 与 `arm64`；两种期望架构的生产 verifier 均通过。
- codesign 与 Gatekeeper 通过；本机已有 `/Applications/CC Switch.app` 也显示 `Notarized Developer ID`。
- 嵌入式信任注册表更新后，生产 `detect_product` 路径返回 `installed=true`、版本 `3.19.1` 和精确 Bundle/Team identity。
- 仍缺首次安装、旧版更新、回滚、权限和 Apple Silicon 真机矩阵，因此保持禁用。

## WorkBuddy 5.3.8 Intel / Apple Silicon

- 官方 API：`https://www.workbuddy.cn/v2/update?platform=workbuddy-darwin-x64`。
- API 版本：`5.3.8.34705286`。
- API 声明 SHA-256：`81971beb350c7062355fcaa6e553a26faf0da7e5013cf1039f9d27d70ce5de3d`。
- 下载文件：414,884,017 bytes；本地 SHA-256 为 `39ab7d0f2fbf6189d82759db451d9d68cd3f0b64ea19a7df4e0b722f0b7f9688`，与 API 不一致。
- 本地 MD5 `ac43c8b2d657b0e5011f0c997df04ca7` 与 CDN `x-cos-meta-md5` 完全一致，ZIP 完整性检查无错误，因此不是截断下载。
- 应用名：`WorkBuddy.app`。
- Bundle ID：`com.workbuddy.workbuddy`。
- Team ID：`FN2V63AD2J`。
- 应用版本：`5.3.8`；Intel slice、codesign 与 Gatekeeper 通过。
- 结论：平台身份可固定，但摘要合同未闭合；不能忽略 API SHA，也不能启用安装。
- Apple Silicon API 声明 SHA-256：`583ee29d9f037523200eb0d6b59f199119922b9a11101e8174ae1963a4ce4974`。
- Apple Silicon 下载文件：402,827,548 bytes；本地 SHA-256 为 `a6af3b9747586725e5a1e89ca205f7ff5e768a80d5396aaf7cc3e8e0c96c10fc`，同样与 API 不一致。
- Apple Silicon 本地 MD5 `9325dd58b258dd36550897293f9eb184` 与 CDN `x-cos-meta-md5` 一致，ZIP 完整性无错误；ARM64 slice、Bundle、Team ID、codesign 与 Gatekeeper 通过。
- 两种架构均证明身份正确但摘要合同失败；仍需两类 Mac 真机安装矩阵。

## Claude Desktop Universal 1.24012.9

- 已知官方制品：`downloads.claude.ai/releases/darwin/universal/1.24012.9/...dmg`，333,746,503 bytes。
- SHA-256：`27251d96083806857310524d4fcdc63e3ddf2bf34bca85410ebd477f7da0f923`。
- MD5 `486af4c0e0d8649032f7926e6d2bcc82` 与对象 ETag 一致。
- 应用名：`Claude.app`。
- Bundle ID：`com.anthropic.claudefordesktop`。
- Team ID：`Q6L2SF6YDW`。
- 版本：`1.24012.9`。
- x64 与 ARM64 slice、codesign 与 Gatekeeper 均通过。
- `https://claude.ai/api/desktop/darwin/universal/dmg/latest/redirect` 在本机网络返回 HTTP 403，响应明确包含 `cf-mitigated: challenge`；普通、项目和浏览器 User-Agent 均不能作为无状态客户端解决该挑战。
- 结论：身份已固定，但稳定入口可达性和两类 Mac 安装矩阵未闭合，保持禁用。

## ChatGPT Intel 26.727.51351

- 官方 x64 appcast 当前版本：`26.727.51351`，内部 build `6119`。
- 本机已有 `/Applications/ChatGPT.app`，与 appcast 最新版一致；本轮未执行安装或更新。
- 应用名：`ChatGPT.app`。
- Bundle ID：`com.openai.codex`。
- Team ID：`2DC432GLL2`。
- 主程序架构：`x86_64`。
- 深度 codesign 与 Gatekeeper 通过，来源为 `Notarized Developer ID`。
- 嵌入式信任注册表更新后，生产 `detect_product` 路径返回 `installed=true`、版本 `26.727.51351` 和精确 Bundle/Team identity。
- Apple Silicon appcast 同版本完整 ZIP 为 559,285,243 bytes，SHA-256 `8f3fc87e634332fddc711e5221eb2af554f5f6ecb04e6a69b3d10e01f4f196c8`；ZIP 完整性、ARM64 slice、Bundle ID、Team ID、codesign 与 Gatekeeper 均通过。
- 两种架构的制品身份已固定；干净安装、旧版更新和回滚仍需独立证明，因此保持禁用。

## Hermes Apple Silicon bootstrap

- 官方制品：`https://hermes-assets.nousresearch.com/Hermes-Setup.dmg`，6,752,854 bytes。
- SHA-256：`b61e047efe3059faf1c55fec3252e661f2d2a993a7a3eebf5cc6a9aa5c1790f5`。
- MD5 `44c1f1848ca0c2118aafde6ca49a92c6` 与 ETag 一致。
- 应用名：`Hermes.app`。
- 实际 Bundle ID：`com.nousresearch.hermes.setup`。
- Team ID：`T2F6S8MF7C`。
- 版本：`0.0.1`；主程序为 ARM64，codesign 与 Gatekeeper 通过。
- 原注册表中的 `com.nousresearch.hermes` 与官方 bootstrap 不一致，已修正为 setup identity。
- 该包是 bootstrap；最终桌面应用、runtime、下游下载边界和 Apple Silicon 真机安装结果仍未证明，因此保持禁用。Intel Mac 继续明确 `unsupported`。

## 本轮自动检查

- `cargo fmt --all -- --check`。
- `cargo test --all-targets`：54 项通过，2 项网络型测试默认忽略。
- `cargo clippy --all-targets --all-features -- -D warnings`。
- `cargo check --all-targets --target x86_64-apple-darwin`。
- `cargo check --all-targets --target aarch64-apple-darwin`。
- live macOS metadata：WorkBuddy、CC Switch、Hermes 与 ChatGPT 通过；Claude stable redirect 因 HTTP 403 challenge 失败。

## 尚未关闭的 Gate

- 不在用户当前主机执行五款产品的首次安装或更新；需要可回滚干净环境。
- WorkBuddy 必须先解释或修复官方摘要不一致，不能改为忽略 SHA。
- Claude 需要厂商提供无状态客户端可用的稳定官方入口，或确认当前 challenge 是区域/网络暂态并取得可复现证据。
- 需要 Apple Silicon 真机验证 WorkBuddy、ChatGPT、Hermes 和全部安装/回滚矩阵；当前 ARM64 制品只在 Intel Mac 上做了静态架构与签名检查。
- 安装助手正式发布仍缺 Developer ID Application 与 notarization 凭据。
