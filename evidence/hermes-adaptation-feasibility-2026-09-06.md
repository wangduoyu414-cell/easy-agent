# Hermes 固定版本与 Mac 适配可行性

> 后续开发：已新增两端共用的只读安装观察模块和命令行探针，使用与限制见 [Hermes 诊断说明](../docs/hermes-diagnostics.md)。以下保留分析阶段的结论。

日期：2026-09-06。本轮仅查询官方 Release、获取上游源码及检查本项目实现；没有执行厂商安装脚本、启动安装器、修改用户 Hermes 环境或启用产品。

## 结论

可以继续开发 Mac 适配，无须单纯等待下一次发布。但“向现有 DMG 安装器加一个版本参数”不是已经成立的方案。官方脚本有固定 commit 能力，GUI 的外部调用入口、依赖锁定失败策略和最终应用复检仍需要解决。

Windows 当前实现比 Mac 多了最终安装布局检测，并有历史真机成功证据；并不等于拥有严格固定源码和依赖的安装合同，也不等于每次生产检测都会执行后端健康检查。此前健康检查通过是验收记录，不能泛化为现有安装器的自动保证。

## 正式版本已定位

[官方最新正式 Release](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.31) 仍为 v0.21.0 / v2026.8.31。通过官方 Git 仓库取得 tag 对应完整 commit：

`29112bef099274229cadff79cdff7bf7b99c4b77`

已提取这个 commit 的 Bash、PowerShell 安装脚本、bootstrap 源码、桌面 package.json、package-lock.json 和 uv.lock。它与此前检查的 main 快照 `9dd6634c5635321cf38840cc30e9b51226689128` 分开存放。正式版本也包含下列回退行为，因此不能通过“main 换成 Release 名称”自动解决全部问题。

对 `raw.githubusercontent.com/NousResearch/hermes-agent/<完整 commit>/scripts/` 两个脚本进行只读下载，均 HTTP 200，字节与 `git show` 完全一致：install.sh SHA-256 为 `85ef536d455e51ab67aa74d79272efd49fe717597dbaadfd3cca179a905f4706`，install.ps1 为 `65552df7a1214b288fbf858f47774e0a561d7587824948677633fda03a0686dc`。单次可达不保证所有网络长期可用。

正式版 npm 锁文件含 1402 个 registry.npmjs.org 解析记录和 7 个本地记录；其中 HTTPS 解析记录均有 integrity 字段。说明上游已有依赖锁定基础，不能笼统称为“全部依赖不固定”。本轮没有下载并逐一验证这些依赖，锁定失败后的回退仍需另行处理。

## 已确认的关键问题

| 问题 | 证据与含义 | 适配要求 |
| --- | --- | --- |
| 脚本支持 pin，GUI 没有等价 CLI | Bash 支持 `--commit`，PowerShell 支持 `-Commit`；检查的 GUI lib.rs 仅解析 update/repair/reinstall 等模式。bootstrap 内部有 commit 字段，但普通前端没有填写它 | 不把内部 Tauri 参数当作受支持的外部命令行接口；追加 `--commit` 不足以证明官方二进制会采纳 |
| 构建参数不等于运行参数 | `HERMES_BUILD_PIN_COMMIT` 在 build.rs 编译时使用 | 对已经下载的签名安装器设置这个环境变量不能重写编译 pin；自编译后也不能继续冒充原厂 Developer ID 包 |
| 指定 commit 可能被防降级逻辑忽略 | Bash 在目标 commit 是当前 HEAD 的旧祖先时，可输出 `Ignoring --commit`，除非显式 force | 安装后必须核对实际 HEAD；遇到较新现有安装先报告，不能自动加 force 覆盖用户环境 |
| Python 锁定可退化 | 正式版两端优先 `uv sync --extra all --locked`，失败后可重新从 PyPI 解析 | 把锁定失败与成功分开；严格固定模式应失败即停止，不能将回退算作同样可重复 |
| Node 依赖锁定可退化 | 桌面安装优先 npm ci，失败可回退 npm install | 需要保存实际执行路径；包锁存在不等于所有分支都遵守包锁 |
| 工具链仍有移动入口 | uv 安装脚本、Node latest-v 系列目录等 | 顶层源码固定后，还应记录并约束实际 uv/Node/Python 版本与来源 |
| Electron 存在第三方回退 | 正式版 Bash 与 PowerShell 都包含 npmmirror 回退 | 官方链失败不能静默变成未经本项目审核的新来源；设置 ELECTRON_MIRROR 能影响部分分支，不代表整个下载链已封闭 |
| 更新/修复会影响 pin | 桌面 bootstrap-runner 按安装状态和 build stamp 选择 commit/branch，修复/更新有避免回退旧 commit 的逻辑 | 首次安装固定成功后仍需单独验证更新与修复，不能只测试一次新装 |

官方脚本按 commit 获取，可作为固定源码的研究入口；仍须审核文件摘要与所有后续下载。这不是建议用户直接运行网络脚本。

## 本项目 Mac 缺口

`src/platform/macos.rs` 的检测、preflight 和安装当前只实现 `DirectAppBundle`。注册表虽然有 `VendorBootstrap` 枚举，但不存在 Hermes Mac 对应执行链：

1. 验证并启动 setup，明确所有子进程和安装根目录，保证 GUI 再启动时仍使用同一环境。
2. 分别记录 setup 身份、实际源码 commit、最终 desktop 版本/架构以及 runtime 状态。
3. 检测最终应用路径与签名类型。最终应用可能本地构建并 ad-hoc 签名，不能要求它复用 setup 的 Team ID，也不能仅凭 package.json 或官方 origin 字符串就认定文件未被篡改。
4. 将后端健康验证做成独立验收，绑定本次安装的路径/进程，避免误认旧实例端口为新版本成功。
5. 处理取消、失败和部分成功。多阶段 bootstrap 会写依赖、源码和配置，不能套用“替换单个 .app”就宣称整体可回滚。

Windows 的 `detect_hermes_fixed_install_at_with` 已检查固定目录、setup 签名入口、桌面 EXE 架构、package.json、Git origin 和 Python 版本文件。但 origin/版本文本不是内容完整性证明；源码 commit、依赖完整性、每次健康复检仍是可加强点。不能把这些检查原样复制到 Mac 就认为达到全部目标。

## 推荐推进路径

第一阶段先形成两端共用的验收合同和只读检测能力：区分“安装器已验证”“最终桌面存在”“runtime 健康”“实际版本固定”，对不完整安装输出具体原因。这部分不依赖安装入口启用。

第二阶段优先采用厂商支持的、可传入不可变 pin 并返回结构化结果的官方 bootstrap 接口，或厂商发布的完整签名桌面包。现有研究确认底层具备 pin 能力，但尚未确认已分发 GUI 有这种外部接口。

若需要以官方固定脚本做隔离实验，应先审核依赖回退和写入范围，在独立 macOS 用户或虚拟机中进行；单独设置 HERMES_HOME 不构成系统级隔离。该方案改变了本项目“只运行已验证厂商安装器”的分发边界，不能作为现有 DMG 适配的小补丁直接启用。

第三阶段完成真实首次安装、启动/健康、固定版本复检、已有新版保护、更新、取消、断网失败和部分安装恢复矩阵，再启用 Mac。Windows 同步复验当前链，不依据历史结果升级为“严格固定版本支持”。

## 可定位的证据

- [正式版 install.sh](https://github.com/NousResearch/hermes-agent/blob/29112bef099274229cadff79cdff7bf7b99c4b77/scripts/install.sh)：commit、防降级、工具链及依赖回退。
- [正式版 install.ps1](https://github.com/NousResearch/hermes-agent/blob/29112bef099274229cadff79cdff7bf7b99c4b77/scripts/install.ps1)：Windows 对应流程。
- [正式版 GUI lib.rs](https://github.com/NousResearch/hermes-agent/blob/29112bef099274229cadff79cdff7bf7b99c4b77/apps/bootstrap-installer/src-tauri/src/lib.rs) 与 [build.rs](https://github.com/NousResearch/hermes-agent/blob/29112bef099274229cadff79cdff7bf7b99c4b77/apps/bootstrap-installer/src-tauri/build.rs)：命令行与编译 pin 的区别。
- [此前 main 快照的 desktop bootstrap-runner](https://github.com/NousResearch/hermes-agent/blob/9dd6634c5635321cf38840cc30e9b51226689128/apps/desktop/electron/bootstrap-runner.ts)：桌面二次 bootstrap/修复的 pin 行为；不据此断言所有已发布二进制采用相同构建参数。

本机提取文件及只读 URL 探测结果：`/Users/zj/Library/Caches/easy-agent/vendor-proof/hermes-release-probe/`。代码保持现状，仅新增本分析文档；不需要重新构建或沿用安装测试冒充本轮执行结果。
