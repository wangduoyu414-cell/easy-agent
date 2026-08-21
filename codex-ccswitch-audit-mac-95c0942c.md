# Codex + CC Switch 全链路只读审计 — mac-95c0942c

## 1. 一页结论

这台 Mac 已经实现了“官方 ChatGPT 登录 + 第三方 API Key 上游 + 扩展模型目录”的三层分离：

- 两个可运行的 Codex CLI 均报告 ChatGPT 登录，`auth.json` 使用 `chatgpt` 模式。
- 第三方 Provider `codexmanager_server` 使用独立静态 Token；该 Token 与 CC Switch 当前 Provider 中的 Token 完全相同。
- CC Switch 的 `preserveCodexOfficialAuthOnSwitch` 为 `true`，其官方认证快照与活动 `auth.json` 的账号、access token、refresh token 均相同。
- `model_catalog_json` 强制指向 `~/.codex/models_catalog.json`，因此模型菜单并不只由官方登录或上游 `/models` 决定。

当前请求没有经过 CC Switch 本地路由。CC Switch 的 UI 偏好 `enableLocalProxy=true`，但数据库中的 `proxy_enabled=0`、`enabled=0`、`live_takeover_active=0`，端口 15721 无监听；Codex 直接指向第三方上游 `https://49.232.229.239/codex/v1`。

显式 macOS 代理和代理环境变量均关闭，但 Clash Verge/Mihomo 的透明 TUN/fake-IP 路径正在生效：OpenAI 域名解析为 `198.18.0.x`，Codex 也与该假 IP 网段建立连接。因此“系统代理关闭”不等于真实直连。

最可能造成与 Windows 基准差异的因素是：

1. Desktop 内嵌 CLI 为 `0.147.0-alpha.1.2`，shell/npm CLI 为 `0.147.0`；当前任务使用前者。
2. 运行时 service tier 是 `default`、reasoning effort 是 `high`，但磁盘默认分别是 `priority` 和 `ultra`，说明任务启动时状态或线程覆盖仍在生效。
3. 官方缓存 9 个模型、强制目录 30 个、实时上游 32 个；三者不是同一个数据源。
4. 强制目录比实时上游旧，缺少 `grok-4.6` 与 `kimi-k3-256k`。

## 2. 实际请求链路

已确认的路径是：

`Codex Desktop/CLI` → `Clash Verge/Mihomo 透明 TUN/fake-IP 接管` → `第三方 Provider 49.232.229.239:443` → `GET /codex/v1/models`

证据包括：Codex 到 `49.232.229.239:443` 的已建立连接、OpenAI 域名的 `198.18.0.x` 假 IP、运行中的 Mihomo，以及成功的上游探测。

CC Switch 正在运行并打开 `~/.cc-switch/cc-switch.db`，但没有监听 15721，Codex `base_url` 也不指向本地端口。因此当前链路不是：

`Codex → CC Switch 本地代理 → 上游`

## 3. 官方身份与推理凭据是否分离

是，且当前证据一致。

- 官方身份：`auth_mode=chatgpt`；ID/access/refresh token 均存在，报告仅记录长度和指纹。
- 推理凭据：自定义 Provider 静态 Token 存在，长度 64，指纹与 CC Switch 当前 Provider 相同。
- `auth.json` 不包含活动 `OPENAI_API_KEY`；第三方 Token 位于 Provider 配置中，没有覆盖官方登录缓存。
- Keychain 中定向查询的 `Codex Auth` 条目不存在，因此 `auto` 模式当前实际使用 file store；不存在 file/Keychain 双缓存冲突。

## 4. 配置优先级

所有采样入口都未设置 `CODEX_HOME`，并共享同一 `HOME`，因此有效目录为默认 `~/.codex`。未发现项目级配置、`/etc/codex/managed_config.toml`、`requirements.toml` 或 MDM 注入的 Codex TOML。

有效磁盘默认值：

- model：`gpt-5.6-sol`
- provider：`codexmanager_server`
- catalog：`~/.codex/models_catalog.json`
- service tier：`priority`
- reasoning effort：`ultra`

当前任务 model/provider 与磁盘一致，但 service tier 为 `default`、effort 为 `high`。`runtime_config_stale_or_thread_pinned=true`。

## 5. CC Switch 当前 Provider

数据库当前 Provider 是 `codex`，类别为 custom，base URL 和 wire API 与有效 Codex 配置一致；Token 也完全相同。官方认证快照存在并与活动认证完全一致。

需要注意两个状态差异：

- `settings.json` 的 `currentProviderCodex` 是 `default`，数据库 current Provider 名称却是 `codex`。
- `enableLocalProxy=true` 只是 UI/偏好状态；实际 proxy、takeover 和监听均关闭。

因此判断 CC Switch 是否真正切换成功，应该以“写入 Codex 的有效配置 + 数据库 current Provider”判断，而不能只看本地代理或托盘 UI。

## 6. 模型菜单的真实来源

| 数据源 | 总数 | 可见数 | 说明 |
| --- | ---: | ---: | --- |
| 官方/远程缓存 `models_cache.json` | 9 | 7 | client 0.147.0，2026-08-08 更新 |
| 强制目录 `models_catalog.json` | 30 | 21 | client 0.146.0，2026-07-27 更新 |
| 实时上游认证 `/models` | 32 | 不适用 | 只反映上游支持，不含 Codex visibility |

菜单主要由强制目录及其 `visibility` 决定，不是把 `/models` 返回值原样全部展示。上游当前额外提供 `grok-4.6` 与 `kimi-k3-256k`，但它们不在强制目录，因此不会自动出现在菜单。远程缓存独有隐藏模型 `gpt-5.6-sol-wm`。

## 7. GUI 与终端差异

GUI、CC Switch 与当前 shell 都没有 `CODEX_HOME`、Provider 或 API Key 环境覆盖；它们均落到 `~/.codex`。显式代理变量也一致为缺失。

真正的入口差异是二进制：Finder 启动的 Desktop 使用内嵌 alpha CLI，登录 shell 默认运行 npm stable CLI。两者目前登录状态相同，但版本、启动参数和未来配置解析行为可能不同。

## 8. Rosetta/多版本风险

本机是 Intel Mac。Codex、Clash 和当前 CC Switch 进程以 x86_64 原生运行，不是 Rosetta；没有发现 arm64/x86_64 两套 Codex 混装。

但确实存在两个 Codex 版本：

- Desktop 内嵌：`0.147.0-alpha.1.2`
- npm/shell：`0.147.0`

当前任务明确使用 Desktop 内嵌版本，而不是 shell 默认版本。

## 9. 代理、DNS、TLS

macOS `scutil --proxy` 显示 HTTP、HTTPS、SOCKS 和 PAC 均关闭；代理环境变量也不存在。然而 OpenAI 域名和直接指定公共 DNS 的查询仍返回 `198.18.0.x`，说明 DNS 流量也被透明接管，所谓 1.1.1.1/8.8.8.8 结果不是独立公网 DNS 结果。

OpenAI 与 ChatGPT 证书由 Google Trust Services 签发，元数据正常。第三方上游 URL 使用 IP 地址，而观察到的证书身份元数据不包含该 IP；这是兼容性和安全风险，尽管本次客户端探测成功。

OAuth 回调端口 1455 未发现监听冲突。

未认证 `/models` 返回 401；认证 `/models` 返回 200 和 32 个模型。整个审计没有调用 `/responses`、Chat Completions 或任何推理接口。

## 10. 与目标之间的缺口

目标能力在磁盘和当前运行状态上基本具备。剩余缺口是：

1. 强制模型目录落后于上游，少两个实时模型，并且 visibility 只显示 21 个。
2. Desktop 与 npm CLI 版本不同；Windows 基准若使用另一版本，菜单和配置行为可能不同。
3. 当前线程的 tier/effort 与磁盘默认不同，说明“改了配置”不等于既有线程立即采用新值。
4. CC Switch 的 UI Provider 标记、数据库 current Provider 和本地代理状态是三套不同状态，不能混为一谈。
5. 透明 TUN/fake-IP 路径可能导致 Mac 与使用显式代理或不同 DNS 模式的 Windows 电脑出现网络差异。
6. `~/.cc-switch/cc-switch.db` 为 0644，在多用户 Mac 上可能被其他本地用户读取。

## 11. collection_errors 和尚不能确认的事项

- 网络时间同步状态需要管理员权限；本次仅确认时区、本地时间、UTC 与偏移一致。
- `codex doctor --json` 在限定时间内未完成；其余配置、认证、运行时和网络证据已通过实际文件、进程和只读诊断补齐。
- `lsof` 未暴露 Mihomo 的活动 YAML 路径，因此未读取具体 TUN/fake-IP 配置和规则；透明接管由 DNS 与连接证据推断。
- CC Switch 当前 schema 中未找到权威的“自定义 Codex Directory”字段，因此该项仍为 unknown。
- 未通过 UI 自动化读取模型选择器；菜单结论来自强制目录、visibility 和当前运行时的交叉证据。
