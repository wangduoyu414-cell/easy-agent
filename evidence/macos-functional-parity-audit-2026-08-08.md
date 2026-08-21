# macOS 功能链路与 Windows 对照审计

日期：2026-08-08

## 结论

- 本轮严格暂停 `packaging/build-macos.sh`，没有生成或覆盖任何 `.app`、DMG 或 `dist/` 发布文件。
- 审计开始时“macOS 安装功能全部不可用”的直接原因已确认：嵌入式信任注册表当时把所有 macOS 条目都设置为 `enabled = false`，解析器会在联网前拒绝生成安装计划；这不是按钮事件、下载线程或 macOS 系统本身的问题。
- 通用 macOS 直接应用包链路已经达到 Windows 直接包链路的同等级阶段：精确检测、官方元数据解析、私有暂存下载、摘要或 updater 签名、平台签名与身份、架构、版本、执行前二次绑定、安装前门禁、同卷暂存、原子替换、失败清理、旧版回滚和安装后复检均有生产代码与自动测试。
- WorkBuddy、CC Switch 与 ChatGPT 的 x64/ARM64 官方完整包已经通过当前生产验证路径，因此三者对应的六个 macOS 条目已启用。
- WorkBuddy 厂商接口给出的两个 SHA-256 都与实际 CDN 文件不一致。经产品级安全复核后，错误摘要改为记录警告；实际下载文件仍由本地 SHA-256 稳定绑定，并强制通过固定 Apple Bundle ID、Team ID、版本、架构、codesign 与 Gatekeeper。例外被代码限制在这两个 WorkBuddy macOS ZIP 条目，不能推广到其他产品。
- Claude 官方稳定端点在当前地区仍可能返回 HTTP 403，但后续已经接入固定、签名、限时且按架构隔离的香港清单，下载与验证链已启用；Hermes 仍因 vendor bootstrap 最终行为未建模而失败关闭。
- 本轮没有覆盖真实第三方安装位置。WorkBuddy、CC Switch 与 ChatGPT 的 x64/ARM64 六个最新包都在随机临时目录完成首次安装、原位更新、强制最终失败回滚和新装失败清理；`/Applications` 与 `~/Applications` 中的现有应用未被修改。

## Windows 与 macOS 功能阶段对照

| 阶段 | Windows | macOS 当前实现 | 对照结果 |
| --- | --- | --- | --- |
| 平台与架构 | Windows x64/ARM64；包架构和目标架构分离 | Intel/Apple Silicon；使用 `hw.optional.arm64` 识别物理硬件，避免 Rosetta 误选 Intel 包 | 等价 |
| 支持策略 | 每产品/系统/架构独立信任条目 | 同样按产品/系统/架构独立建模；新增 `macos_install_strategy` | 等价 |
| 已安装检测 | AppX identity/family/publisher 或精确卸载项/最终 EXE | `~/Applications` 与 `/Applications` 精确应用名、Bundle ID、Team ID、版本、Mach-O 与根 codesign；深度 codesign/Gatekeeper 保留在安装边界 | 等价，刷新更快 |
| 旧架构迁移 | 现有 x64/ARM64 身份可由目标架构包更新，最终复检目标架构 | 现有可信 x64 应用可在 Apple Silicon 上迁移为 ARM64；新包和最终应用仍严格要求目标 slice | 已补齐 |
| 官方版本解析 | 固定元数据入口、host/path、包型与身份合同 | WorkBuddy API、Hermes 官网、CC Switch 清单、Claude redirect、ChatGPT appcast | 等价 |
| 网络 | HTTPS、逐跳重定向校验、超时、大小限制 | 同一核心实现；系统代理读取；ChatGPT 官方网络明确不可用时可进入签名、限时、不可变路径的固定回退 | 已补齐 |
| 下载 | 随机私有目录、`.part`、取消、2 GiB 上限、稳定文件身份 | 复用同一核心实现 | 同一代码 |
| updater/文件绑定 | CC Switch minisign；其余按摘要或平台身份 | CC Switch minisign；ChatGPT Sparkle Ed25519；WorkBuddy 以实际 SHA-256 稳定绑定并强制 Apple 平台身份 | macOS 更完整 |
| 平台签名与身份 | Authenticode/AppX、Publisher、Identity、Family、PE/AppX 架构 | Apple codesign、固定 Team ID/Bundle ID、Gatekeeper、Mach-O slice | 平台等价 |
| 安装前策略 | 已装更高版、受管理、管理未知、版本未知时失败关闭 | 同一核心版本策略；另加应用运行状态和安装目录可写性门禁 | 等价并补强 |
| 执行前二次绑定 | 再核对路径、长度、摘要、签名后启动结构化命令 | 再核对路径、长度和实际 SHA-256；按条目复核远端摘要、minisign/Sparkle 与 Apple 身份后进入内部复制器 | 等价 |
| 包展开 | Windows 安装器或 MSIX 部署 | DMG 只读挂载；ZIP/tar.gz 展开前后检查条目、大小、路径、重复项、符号链接和特殊文件 | 平台等价 |
| 激活 | MSI/EXE/MSIX 由系统或厂商安装器提交 | 目标卷私有 stage、`ditto` 复制复验、同卷 rename 原子激活 | 等价 |
| 失败恢复 | 安装器退出码与系统部署结果 | 新装最终复验失败会删除无效应用；更新失败恢复旧版，恢复失败保留备份路径 | 已覆盖 |
| 运行中应用 | ChatGPT MSIX 使用 `ForceTargetApplicationShutdown`；其他厂商安装器自行处理 | 下载前和最终替换前按精确主 executable 路径检查，要求用户先完全退出 | 已补齐 |
| 安装后复检 | identity/publisher/family/架构/版本，有界等待和 `ResultUnknown` | Bundle ID/Team ID/架构/版本，同一有界等待和 `ResultUnknown` | 同一编排 |
| UI 与日志 | 可安装/更新/禁用/不支持/失败原因、独立任务与脱敏日志 | 下载/校验可并行，系统安装和 postcheck 通过 FIFO 通道排队；各产品独立取消、错误和刷新 | 已补齐 |

## 产品支持矩阵

| 产品 | Intel Mac | Apple Silicon | 当前状态与原因 |
| --- | --- | --- | --- |
| CC Switch | 已启用 | 已启用 | 官方 Universal tar.gz、两个清单架构项、minisign、Bundle、Team、版本、双 slice、codesign、Gatekeeper、临时首次安装和更新激活均通过 |
| ChatGPT | 已启用 | 已启用 | 官方 appcast 优先；网络不可达时使用固定签名清单回退；Sparkle Ed25519、SHA-256、Bundle、Team、版本、目标 slice、codesign、Gatekeeper 与临时安装/更新/回滚均通过；最低系统固定为 appcast 声明的 macOS 12 |
| WorkBuddy | 已启用 | 已启用 | 厂商摘要错误仅记录；实际文件 SHA-256 稳定绑定，固定 Bundle、Team、版本、目标 slice、codesign 与 Gatekeeper 均通过。专用策略不能用于其他身份 |
| Claude Desktop | 已启用 | 已启用 | 官方 stable redirect 优先；明确可用性失败时使用固定香港签名清单，仍验证 SHA-256、Bundle、Team、版本、目标 slice、codesign 与 Gatekeeper |
| Hermes Agent | 厂商明确不支持 | 禁用 | DMG 内是 `com.nousresearch.hermes.setup` / `0.0.1` vendor bootstrap，不是官网 `0.20.0` 最终桌面应用；复制 bootstrap 不能等价于完成安装 |

## 当前官方完整包 proof

### WorkBuddy 5.3.8.34705286

- Intel API 声明 SHA-256：`81971beb350c7062355fcaa6e553a26faf0da7e5013cf1039f9d27d70ce5de3d`。
- Intel 完整 ZIP 实际 SHA-256：`39ab7d0f2fbf6189d82759db451d9d68cd3f0b64ea19a7df4e0b722f0b7f9688`，`remote_digest_matches=false`。
- Apple Silicon API 声明 SHA-256：`583ee29d9f037523200eb0d6b59f199119922b9a11101e8174ae1963a4ce4974`。
- Apple Silicon 完整 ZIP 实际 SHA-256：`a6af3b9747586725e5a1e89ca205f7ff5e768a80d5396aaf7cc3e8e0c96c10fc`，`remote_digest_matches=false`。
- 两个实际 SHA-256 都会与规范路径和长度一起形成 `StableFileIdentity`，安装交接前、Apple 平台验证后都会重新计算；文件发生任何变化仍立即失败。
- Bundle ID：`com.workbuddy.workbuddy`。
- Team ID：`FN2V63AD2J`。
- Intel 包通过 x64 slice、版本、codesign 与 Gatekeeper 验证；Apple Silicon 包通过 ARM64 slice、版本、codesign 与 Gatekeeper 验证。
- WorkBuddy 的候选版本含内部构建号，应用 Bundle 只登记三段发行版本；proof 与生产 postcheck 均按该产品固定的三段版本规则比较。
- `platform_signature_only` 在注册表加载时被严格限制为这两个固定 WorkBuddy macOS ZIP 身份；错误产品、Windows、其他包型、Bundle ID 或 Team ID 均不能启用该例外。

### CC Switch 3.19.2

- x64 与 ARM64 清单项解析为同一 Universal tar.gz。
- 文件大小：27,837,697 bytes。
- SHA-256：`fa609030111417e5d9af0e89097a12139307a51ac30d92bbd74f4a6f7e61e824`。
- 两种期望架构均得到 `updater_signature_verified=true`。
- Bundle ID：`com.ccswitch.desktop`。
- Team ID：`R8UR22V2F9`。
- 生产 verifier 对 x64 与 ARM64 均通过。
- 当前主机 `/Applications/CC Switch.app`：版本 `3.19.1`，codesign 与 Gatekeeper 通过。
- 最新完整包在随机临时目录分别以 x64/ARM64 执行“首次安装 → 原位更新 → 强制最终失败恢复旧版 → 新装失败删除无效应用”，全部通过；没有修改系统 Applications。

### ChatGPT 26.803.41515

- Intel ZIP：539,372,355 bytes。
- Intel SHA-256：`87239a3dd12e2761de515ee78a0b73c02ac907c34c1f01a68dd6f093cf433fb1`。
- Apple Silicon ZIP：551,752,702 bytes。
- Apple Silicon SHA-256：`8abd46bf063bc27cbadcbc2863007ac44365b13a23ede17ceaee81fb9eeaeb9a`。
- 两个完整 ZIP 均使用从已签名官方应用 `Info.plist` 固定的 `SUPublicEDKey` 验证 appcast `edSignature`，结果均为 `updater_signature_verified=true`。
- 固定 Sparkle 公钥：`mNfr1v9t63BfgDtlw4C8lRvSY6uMggIXABDOCi3tS6k=`。
- Bundle ID：`com.openai.codex`。
- Team ID：`2DC432GLL2`。
- x64 与 ARM64 生产 verifier 均通过版本、目标 slice、codesign 和 Gatekeeper。
- 当前主机 `/Applications/ChatGPT.app`：版本 `26.727.51351`，codesign 与 Gatekeeper 通过；本轮未更新。
- 香港固定回退每 30 分钟同步两个架构；连续定时复检可在约 1 秒内复用同一不可变文件。清单由 minisign 保护，并固定生成时间、最近成功上游检查、大小、SHA-256、架构和精确文件路径；客户端仍重复验证 OpenAI Sparkle 与 Apple 身份。
- x64 与 ARM64 最新 ZIP 均在随机临时目录通过首次安装、原位更新、更新失败回滚和新装失败清理。Intel 主机不能真实启动 ARM64 应用，因此 ARM64 启动仍属于 Apple Silicon 真机边界。

### 外部状态复核

- WorkBuddy 两个远端摘要仍错误；这是持续监控的厂商元数据缺陷，但不再是固定 Apple 身份包的安装阻塞。若下载地址、Bundle ID、Team ID、包型或签名状态变化，条目仍会失败关闭。
- Claude live stable redirect 在当前网络仍可能返回 `HttpStatus(403)`；客户端会把已识别的地区/网络失败转入固定受验证回退，安全或合同错误不会回退。
- Hermes 官网当前版本：`0.20.0`；DMG setup Bundle 版本仍为 `0.0.1`，需要专用 bootstrap 执行、下游信任边界和最终桌面/runtime postcheck，不能复用直接应用包策略。

## 本轮修复

1. 启用 WorkBuddy、CC Switch 与 ChatGPT 的 macOS x64/ARM64 条目。
2. 新增默认 `enforce_if_present` 和 WorkBuddy macOS 专用 `platform_signature_only` 摘要策略；后者仍绑定实际文件 SHA-256，并由注册表固定身份限制防止扩散。
3. 新增显式 `MacOsInstallStrategy`：`direct_app_bundle` 与 `vendor_bootstrap`。
4. 注册表禁止启用尚未实现的 vendor bootstrap；执行器也有第二道策略门禁。
5. ChatGPT appcast 的 Sparkle 签名不再丢弃；下载完成和执行前都验证完整文件。
6. `reqwest` 启用系统代理支持，覆盖 Finder 启动的桌面应用网络环境。
7. 现有可信应用允许跨架构迁移，但候选包和最终结果仍必须匹配当前硬件。
8. 下载前与激活前检查目标主进程；运行中应用不会被直接替换。
9. 下载前检查目标目录可写性，避免先下载数百 MiB 后才报告权限失败。
10. UI 直接显示禁用、不支持和在线解析失败的具体原因，按钮悬停也显示完整说明。
11. 新增首次安装成功、更新成功、首次安装最终复验失败清理、更新失败回滚、精确运行进程匹配和跨架构迁移测试。
12. 新增只读 `--download-verify` 与 `--preflight` 证据模式，便于复用生产链验证官方包而不安装。
13. ChatGPT appcast 开始固定完整包 `length`、最低 macOS `12.0` 和 64-byte Ed25519 签名形状；官方域名明确网络不可达时进入独立签名回退，安全/合同错误禁止回退。
14. macOS 外部系统命令增加分类超时、独立进程组终止和回收测试；`spctl`/`codesign` 不再可能无限挂起或留下孤儿进程。
15. 下载包和最终应用保留完整深度 codesign/Gatekeeper，日常状态扫描改为根签名检查；本机 ChatGPT 刷新由约 28 秒降至约 5 秒。
16. 批次只有全部成功才自动刷新；失败、取消和结果未知保留具体错误，不再立即被扫描文字覆盖。

## 仍然保留的边界

- 标准用户更新不可写的 `/Applications` 时会在下载前明确失败；本项目没有实现管理员提权或绕过系统权限。这属于平台权限模型差异，不能静默降级为创建第二份应用。
- Hermes vendor bootstrap 未启用；在最终桌面应用、runtime、下游下载与回滚合同确定前，不执行其远端脚本链。
- WorkBuddy 厂商摘要错误仍会记录并持续监控；专用策略只承认固定 Apple 身份，不能作为其他产品忽略摘要的先例。Claude 固定回退已接通，但仍需要两类 Mac 的真实安装/更新矩阵和服务端持续健康验证。
- `easy agent` 自身正式发行仍需要 Developer ID Application 签名、notarization 和 staple；这与第三方客户端安装链是否可用是两个独立 Gate。
- 本轮没有对真实第三方安装位置执行破坏性首次安装或更新。六个完整包的下载、验签、临时首次安装、更新、失败回滚与清理已闭合；正式发布前仍建议在可回滚 Intel/Apple Silicon 干净机各执行一次真实 UI 安装与启动矩阵。

## 构建门禁

本轮按用户要求保持“功能审计优先、打包暂停”，没有运行 `packaging/build-macos.sh`。当前非打包门禁结果：

- `cargo fmt --all -- --check`：通过。
- `cargo test --all-targets`：84 项通过，6 项环境型 proof 按设计忽略。
- `cargo clippy --all-targets --all-features -- -D warnings`：通过。
- `cargo check --all-targets --target aarch64-apple-darwin`：通过。
- `live_enabled_macos_install_plans_resolve_for_both_architectures`：显式在线运行通过，WorkBuddy、CC Switch、ChatGPT 的 Intel/Apple Silicon 计划均成功解析；本机 ChatGPT 官方域不可达时按设计进入固定受验证回退。
- `live_artifact_closes_download_install_update_and_rollback_loop`：六种产品/架构组合全部显式通过，输出的版本、SHA-256、Bundle ID、Team ID 与上文一致。
- 香港 Nginx 配置检查、healthz、ChatGPT 同步 timer、最近 service 结果、x64/ARM64 Range `206` 与准确 `Content-Range`：全部通过。

以上真实包 proof 下载了完整第三方安装包，但只在随机临时目录激活；没有修改真实 Applications，也没有重新构建 easy agent 的 macOS 安装包。打包仍作为独立后续步骤处理。
