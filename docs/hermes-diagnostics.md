# Hermes 安装状态只读诊断

后续集成：Mac 正式扫描与 Hermes 详情已使用布局观察摘要，仍不把观察结果当安装成功。桌面身份和后端探针已在真实固定源码构建样本上验证，详见[真实运行证据](../evidence/hermes-fixed-source-runtime-2026-09-06.md)。

`hermes_installation_probe` 是 Windows/macOS 共用的诊断入口，不是安装命令，也不参与安装成功判定。它不会运行 Git、安装脚本、Hermes 或 Python，不访问健康检查端口，不修改被检查目录。

```sh
# Mac Apple Silicon：第一个参数是 HERMES_HOME，而非其中的 hermes-agent 源码目录。
cargo run --locked --example hermes_installation_probe -- \
  "$HOME/.hermes" macos arm64 \
  29112bef099274229cadff79cdff7bf7b99c4b77
```

Windows 在 PowerShell 中使用以下形式；ARM64 布局检查可把最后的 `x64` 改成 `arm64`，不代表该平台已启用安装支持。

```powershell
cargo run --locked --example hermes_installation_probe -- "$env:LOCALAPPDATA\hermes" windows x64
```

预期 commit 参数可省略；填写时必须是完整 40 位十六进制 SHA。上例对应审计时的正式版 v2026.8.31，不是程序固定的默认版本。

| 字段 | 含义 |
| --- | --- |
| setup_present | 固定目录存在安装器文件；尚未验证其签名 |
| desktop_executable_present | 存在目标平台预期布局的桌面程序；尚未读取 PE/Mach-O 架构或验证应用身份 |
| runtime_python_entry_present | 存在 Python 文件或符号链接入口；不跟随链接，悬空链接也可能为 true，不代表 Python 可用 |
| source_commit | 读取 Git HEAD/分支引用得到的声明 commit；没有执行 Git，也没有验证工作区文件内容 |
| expected_commit_matches | HEAD 与调用者指定 commit 的比较；缺少任一值时为 null |
| marker_commit_matches | 安装完成标记中的 commit 与 HEAD 的比较；缺少任一值时为 null |
| declared_desktop_version | package.json 声明版本，不是可执行文件的实测版本 |
| missing | 未观察到的布局或元数据项目；为空也不代表安装可信或健康 |
| signature_checked / runtime_health_checked / source_integrity_checked | 本阶段均为 false，后续必须分别实现 |

退出码 0 只代表诊断完成，可能仍有 missing 项或 commit 不一致。读取异常、非法参数、元数据超限、危险引用路径或重复 Mac 桌面布局返回非零。

目前识别普通克隆目录下的 detached HEAD、loose branch ref 和 packed refs；不支持 Git worktree 的 `.git` 文件、外置 Git 目录、自定义桌面输出路径。Mac 同时存在 `release/mac` 和 `release/mac-arm64` 时拒绝猜测。Windows 两种架构布局分别观察。

元数据限制为 1 MiB，拒绝指向外部的符号链接路径，不输出源码配置或用户凭据。诊断是文件系统快照，并非应对并发恶意替换的安全验证器，不能把结果用于跳过生产签名、文件绑定或安装后复检。

此阶段没有改变 GUI 产品启用状态或 Windows 原有安装成功判定，也没有增加自动启动 Hermes 的健康探测。下一阶段需要把安装器验证、最终 desktop 验证和绑定实际进程的 runtime 健康检查接入明确的状态合同。

## Mac 桌面身份诊断（后续新增）

```sh
cargo run --locked --example hermes_desktop_probe -- /absolute/path/to/Hermes.app
```

检查最终桌面的 `com.nousresearch.hermes` Bundle ID、Hermes 主程序名称、包内版本、ARM64 切片和 deep/strict codesign。setup 的 `com.nousresearch.hermes.setup` 不会通过。本地构建的 ad-hoc 签名只证明签名校验通过，不能证明 Nous Research 发布者身份；结果明确保留 `vendor_publisher_verified: false`、`gatekeeper_checked: false`、`runtime_health_checked: false`。

此诊断独立于生产安装验证，不改变注册表或使用诊断结果授权安装。Windows 尚未实现对应的桌面签名探针。

## Mac 已运行后端的绑定检查（后续新增）

```sh
# PID、端口和预期版本必须来自明确的待检查实例；不会自动猜测或启动实例。
cargo run --locked --example hermes_health_probe -- "$HOME/.hermes" BACKEND_PID PORT 0.21.0
```

请求前后分别检查 Darwin 记录的进程启动时间、所有者和实际可执行路径，要求它与该目录 venv 的 Python 一致；通过限时 lsof 核查源码工作目录及该 PID 的 IPv4 回环监听端口。身份不明、路径不同、PID 已变化或端口不属于它时失败。

只请求 `http://127.0.0.1:PORT/api/health`，不走代理、不跟随重定向、不发送凭据；请求限时 3 秒、响应不超过 64 KiB，要求 HTTP 200、布尔 `ok: true` 和与调用者预期完全一致的版本。没有退回到更广泛的 `/api/status` 或尝试登录。

成功只表示观察时这个绑定进程的存活接口正常：不证明源码完整性、模型访问、消息收发、全部依赖健康或安装成功。它也不是抵抗恶意同用户进程竞争的认证机制。Windows 的系统级进程绑定尚未实现。

严格路径检查可能拒绝使用不同工作目录、IPv6/通配监听或框架 Python 启动器的实例；不要因此改写用户 Python 链接或跳过绑定。需要用真实官方安装验证布局后再添加有证据的兼容规则。测试用临时 venv 指向实际框架可执行文件，仅验证诊断机制，没有改动真实环境。

本阶段测试另覆盖临时 ad-hoc 签名应用（不冒充厂商）、签名后修改 plist、setup 身份误用、严格健康响应、重定向、超大响应，以及临时 Python 服务的真实 PID/目录/监听绑定。它们是受控样本测试，不是 Hermes 真实安装验收。

2026-09-06 验证：新增 7 项测试，覆盖残留安装、标记与 HEAD 不一致、引用解析/路径逃逸、超限元数据、符号链接及两端布局；全套 108 项通过，7 项在线测试默认忽略，fmt、严格 Clippy 与 Intel all-targets check 通过。Windows 布局测试在 M1 上使用临时目录样本执行，不代表 Windows 真机测试。

本机 `~/.hermes` 只读探测没有观察到完整安装布局，输出保存于 `/Users/zj/Library/Caches/easy-agent/vendor-proof/hermes-local-observation.json`。这仅覆盖指定目录，不代表其他自定义位置不存在 Hermes。
