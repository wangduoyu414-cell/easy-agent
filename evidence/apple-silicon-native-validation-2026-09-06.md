# Apple Silicon 原生开发验证

验证时间：2026-09-05 至 2026-09-06（Asia/Shanghai）。

基线：`9757d66`（`v0.1.0-preview.3`），本次开发分支为 `codex/apple-silicon-validation`。以下是本地验证结果，不代表远端 CI 已运行或正式 Release 已发布。

> 本文保留首次验证快照（含旧 DMG 摘要和 WorkBuddy 初次失败）。随后已修复 WorkBuddy 双架构身份迁移并重新构建，当前结论与制品摘要见 [安装来源审计](distribution-source-audit-2026-09-06.md)。

## 环境与变更

- Apple M1，16 GiB 内存，macOS 26.6.2（25G83）。
- Rust 1.95.0，host 为 `aarch64-apple-darwin`；Apple Command Line Tools，SDK 26.5。
- 源码在 SMB 网络盘，Cargo 缓存、打包中间文件和输出使用本机磁盘。
- macOS 验证工作流新增 ARM64 `macos-15`，与 `macos-15-intel` 分别执行测试和 Universal 构建，缓存、制品名称按 runner 架构隔离。
- 打包脚本从 `Info.plist` 读取 `LSMinimumSystemVersion`，设置 `MACOSX_DEPLOYMENT_TARGET`；Cargo 测试、Clippy 和发行构建使用 `--locked`。

## 开发与制品检查

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo test --locked --all-targets` | 100 项通过，0 失败，7 项在线/真实制品测试默认忽略 |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | 通过 |
| `cargo check --locked --all-targets --target x86_64-apple-darwin` | 通过 |
| `cargo check --locked --all-targets --target aarch64-apple-darwin` | 通过 |
| `ALLOW_UNSIGNED_MACOS_BUILD=1 bash packaging/build-macos.sh` | 通过，生成约 12 MiB 的验证 DMG |
| `lipo -info` | 同时含 `x86_64` 和 `arm64` |
| `otool -l` | 两个切片的 `LC_BUILD_VERSION` 均为 `minos 12.0`，SDK 26.5 |
| `.app` 深度严格 codesign 与 DMG codesign | 通过，均为 ad-hoc 签名 |
| `spctl --assess --type execute` | 返回 3，`rejected`，符合未公证验证包的预期 |
| Shell 语法、工作流 YAML 解析、`git diff --check` | 通过 |

DMG：`easy-agent-macos-universal-UNNOTARIZED-VALIDATION.dmg`。

SHA-256：`15a82766cec682d4e1724512d9c7c9382eacbc6300043ba9c8d75fab099ddde3`。

未进行 Developer ID 签名、公证或正式发布。没有清除 quarantine、关闭 Gatekeeper 或改写系统安全策略。该机器和 SDK 上的构建成功也不证明 macOS 12 真机兼容性。

## M1 界面验收

从本地生成的 `.app` 启动，进程采样记录 `Code Type: ARM64`。窗口、图标和中文正常显示；底部识别为 `macOS 26.6.2 · Apple Silicon`。

联网扫描完成后，Claude 与 CC Switch 显示可安装，ChatGPT 识别现有系统 Applications 中的版本并显示可更新；WorkBuddy 显示检测失败，Hermes 显示暂不可用。没有点击客户端安装/更新按钮，也没有替换用户现有客户端。

## ARM64 当前制品与临时激活

使用已有的 `platform::macos::tests::live_artifact_closes_download_install_update_and_rollback_loop`，显式设置产品和 `EASY_AGENT_MACOS_PROOF_ARCHITECTURE=arm64`。测试复用生产解析、下载、签名/身份/架构验证和激活代码，但目标为自动清理的临时目录。

| 产品 | 当前结果 |
| --- | --- |
| CC Switch 3.20.1 | 首次安装、同版本重复替换、最终验证失败回滚、失败新装清理通过 |
| ChatGPT 26.901.41600 | 首次安装、同版本重复替换、最终验证失败回滚、失败新装清理通过 |
| Claude 1.46388.4 | 首次安装、同版本重复替换、最终验证失败回滚、失败新装清理通过 |
| WorkBuddy 5.5.3.37748631 | 官方 ARM64 ZIP 的 Bundle ID 已变化，生产 verifier 在身份校验处拒绝，未进入安装 |

已通过制品身份：

- CC Switch：Bundle `com.ccswitch.desktop`，Team `R8UR22V2F9`，SHA-256 `c860cade24f1afb8db93fb4c4edd6b56627c2db7b79b82e4fef4da8505293583`。
- ChatGPT：Bundle `com.openai.codex`，Team `2DC432GLL2`，SHA-256 `789062d54b39770d770035758373963a562997b277fc5a957ddf2cd1aaf76913`。
- Claude：Bundle `com.anthropic.claudefordesktop`，Team `Q6L2SF6YDW`，SHA-256 `c5451dba21b8bf4232f8feffbff946dc7be4d6a64ee22d3190954e16f62444c9`。

“同版本重复替换”不能作为旧版升级到新版的验收证据；临时安装也不能替代系统 Applications 安装、客户端独立启动、账号业务和取消等完整使用矩阵。

## WorkBuddy 身份迁移缺口

固定官方入口 `https://www.workbuddy.cn/v2/update?platform=workbuddy-darwin-arm64` 返回 5.5.3.37748631，以及既有允许范围 `download.codebuddy.cn/workbuddy/saas/darwin-arm64/` 下的 ZIP。

生产验证失败：`Bundle ID mismatch: expected com.workbuddy.workbuddy, got com.tencent.workbuddy.mac`。

本机已安装 WorkBuddy 5.4.7 的只读检查也观察到新 Bundle ID，签名显示 `Tencent Technology (Shanghai) Company Limited (FN2V63AD2J)`。这只支持当前已安装副本的签名主体观察，不能替代新下载包的完整签名、公证和安装验收。

本次没有修改信任配置或放宽身份校验。修复应作为独立身份迁移处理：核验官方 Intel/ARM64 当前包的 Bundle/Team/签名/摘要；明确旧 Bundle 安装的检测与更新规则；复核仅限 WorkBuddy 的摘要例外；补齐拒绝其他身份的测试及双架构临时激活证据。

## 参考

- [GitHub 标准 runner 架构](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
- [Rust Apple Darwin deployment target](https://doc.rust-lang.org/rustc/platform-support/apple-darwin.html)
- [WorkBuddy 官方 Mac 安装说明](https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Mac-Guide)

原始构建日志、进程采样和截图保存在本机缓存目录，第三方制品由临时目录自动清理，不提交到仓库。
