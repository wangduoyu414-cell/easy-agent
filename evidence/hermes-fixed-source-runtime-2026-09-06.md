# Hermes 固定源码构建与真实前后端验证

日期：2026-09-06，Apple M1。官方源码 commit：`29112bef099274229cadff79cdff7bf7b99c4b77`，对应 [v2026.8.31 / v0.21.0](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.31)。这是独立缓存目录中的源码构建实验，不是运行官方 DMG 完成的一键安装，也不是正式分发包。

## 实际完成的验证

- 固定源码的 Python 项目与锁定依赖安装成功，真实后端启动并通过本项目进程绑定健康探针。
- 官方锁定 Electron 运行包下载、摘要检查成功，真实 ARM64 桌面构建成功。
- 本地 ad-hoc 签名后，最终 desktop Bundle ID、ARM64 切片、主程序名和 deep/strict codesign 检查通过；没有将其当作 Nous Research 的 Developer ID 签名。
- 桌面通过本机回环连接到独立后端，以官方支持的会话令牌机制完成认证。界面能加载 default profile，进入模型服务选择页，显示 client v0.17.0 / backend v0.21.0。
- 没有登录模型服务、导入用户凭据、发送模型请求、启动消息渠道或替换 `/Applications` 中的应用。

## 来源与文件系统问题

GitHub Git 检出遇到 HTTP/2 断流、early EOF；同 commit 的 GitHub codeload 归档在 300 秒后得到约 49 MiB，但网站目录尾部未下载完整。没有把不完整归档作为完整制品，也没有改用第三方镜像。

通过官方 Git 仓库的 commit tree，逐个核对归档文件的 Git blob 哈希。归档中除 `website/` 外的 10,113 个文件均可完整读取并匹配；12 个 PowerShell 文件由归档输出 CRLF，恢复 LF 后才与 Git 对象完全匹配。任何转换后仍不匹配的内容都会拒绝。

随后发现默认 Mac 文件系统上的大小写冲突：两条 `contributors/emails/` 记录仅大小写不同，无法同时保留。这不属于应用代码，但会影响完整源码检出、dirty 状态和固定 commit 的证据。最终构建检出排除 `website/` 与 `contributors/`，其余 **9,177 个实际磁盘文件全部再次匹配官方 Git blob**。Git 元数据使用原始 commit/tree 与已验证 blob，并对排除文件设置 skip-worktree；没有创建伪造的发布提交。

构建 stamp 实际记录：commit 为上述完整 SHA，branch 为 null，dirty 为 false，source 为 local。这里的源码一致性只适用于明确列出的构建范围，不宣称未下载网站目录或大小写冲突目录完整。

## 工具链和依赖

| 项目 | 本次结果 |
| --- | --- |
| Node / npm | 26.8.1 / 11.19.0；满足正式版 engines |
| uv / Python | uv 0.12.9；独立目录 Python 3.13.15 ARM64 |
| npm | `npm ci --ignore-scripts` 成功，安装 1,340 个包；使用官方 npm registry、独立配置与缓存；未运行 npm install 回退 |
| Python | `uv sync --locked --extra all --no-install-project --no-build` 成功，安装 101 个依赖；随后锁定同步构建并安装 hermes-agent 0.21.0 |
| 原生依赖 | 审查安装脚本后单独 rebuild esbuild、node-pty，成功 |
| Electron | 40.10.2，官方 GitHub darwin-arm64 ZIP，使用锁定 npm 包附带的 checksums.json 验证 |
| 桌面打包 | `npm run pack --workspace apps/desktop` 成功，electron-builder 26.15.3，ARM64 |

Electron ZIP 实测 SHA-256：`e889b35e399f374f5dca932195287b373c4b43f8bf242e50c35f88a751511a13`，与锁定包校验值一致。没有设置使用远端替代摘要，没有转 npmmirror。构建工具的发布与签名凭据从实验环境移除，禁用证书自动发现；未提交公证、未发布。

## 组件版本不能混用

| 组件 | 观察值 |
| --- | --- |
| 官网 / 正式 Release / Python 后端 | 0.21.0 |
| 正式版 desktop package.json / 最终 CFBundleShortVersionString | 0.17.0 |
| 最终桌面 Bundle ID | com.nousresearch.hermes |
| 此前官方 DMG 中的 setup | 0.0.1 / com.nousresearch.hermes.setup |

最终桌面 UI 自己也分别显示 client 与 backend 版本。直接套用单一 `candidate.version` 与包内版本相等的 postcheck，会错误拒绝这一官方源码构建结果。后续 bootstrap 必须有分别固定/验证的 setup、desktop、runtime、source commit 合同。

## 真实联调中的失败与修正

初次把前端和手动启动的后端放在同一测试配置根，桌面退出/重启的后端清理会影响该目录下的实验进程，连接因此中断。最终使用独立前端 HERMES_HOME、userData 和后端 HERMES_HOME，避免互相归属错误；不能仅靠“指定一个临时目录”宣称整条生命周期已经隔离。

其次，公开 `/api/health` 返回正常不代表受保护 API 能用。仅填写测试占位 token 时，界面能看到健康状态，但 profiles 请求返回 401，仍无法正常连接。最终后端使用官方 `HERMES_DASHBOARD_SESSION_TOKEN`，前端使用同一随机临时会话令牌，通过 `HERMES_DESKTOP_REMOTE_URL` / `HERMES_DESKTOP_REMOTE_TOKEN` 连接回环端口。没有关闭认证或使用用户账号令牌。

该实验使用桌面的 remote 接入模式连接**同机后端**，不是默认 local bootstrap 自动安装链。它证明真实桌面与真实后端可以互通，不证明官方 DMG 的首次安装、修复、升级、取消或数据回滚全部通过。

最终健康探针在 HTTP 请求前后检查 OS 进程身份、实际 Python 路径、源码工作目录、监听归属和启动时间；得到 version 0.21.0、process_binding_checked true。source_integrity_checked 与 model_access_checked 仍为 false：源码核对是前述独立证据，模型访问没有测试。

## 项目集成及仍未关闭的 Gate

Mac 正式扫描已接入 Hermes 只读观察，详情区能显示只有安装器、部分安装、组件存在但未验证、版本记录不一致等情况。观察结果不把 installed 改为 true，也不解除禁用；不自动探测任意端口或启动进程。

本项目 113 项测试、严格 Clippy、双架构 release 构建和 Universal 打包通过。新 easy-agent 验证 DMG 摘要：`fbc636e66a135336320548e5a80baaad2221fe0c33196f230808f9937a49d5eb`。仍未公证、未发布；它与本次单独构建的 Hermes 测试应用是不同制品。

**官方一键安装入口仍未闭合。** 当前已分发的签名 setup 默认跟随 main，外部固定 pin 接口尚未确认，下游锁定失败还允许重新解析或镜像回退。源码实验的成功不能替代这些证据，也不能把自建 ad-hoc 应用当成官方签名包启用 DirectAppBundle。

继续按现有分发合同启用，需要厂商提供可验证的固定版本 bootstrap（含明确下游行为）或完整签名桌面包。另做“由 easy-agent 管理源码构建”的安装模式，则需要独立的来源、工具链、取消、更新和恢复合同，属于分发方式变更，不能用当前诊断结果偷偷放行。

## 本机证据与清理范围

实验根：`/Users/zj/Library/Caches/easy-agent/hermes-fixed-validation/`。源码/工具链/依赖/构建产物留在此处，便于复验，不加入本项目 Git。

原始证据位于 `/Users/zj/Library/Caches/easy-agent/vendor-proof/`：`hermes-fixed-{npm,uv,project,electron,native-deps,pack}.log`、`hermes-real-{layout,desktop,health-final}.json`、`hermes-real-ui.txt`、`hermes-real-ui.jpeg`、`hermes-integrated-build.log`。实验目录还有 `source-verification.json`、`source-filesystem-audit.json`、`build-source-verification.json` 和 build/install-stamp.json。

会话令牌只用于本次回环联调，不写进报告或本项目仓库。结束验证时仅停止本次测试桌面与后端，并分别撤销测试 app 和残留 Helper 的 LaunchServices 注册；随后读取注册表快照，确认没有本次测试 app 路径条目。不操作用户其他应用、凭据或原有 `~/.hermes` 数据。清理结果记录在 `hermes-validation-cleanup.json`。

后续来源、版本、生命周期与恢复验收条件见[固定源码安装模式待决设计](../docs/hermes-source-install-design.md)，该方案尚未启用。
