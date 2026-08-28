# Codex + CC Switch 只读取证审计 — UNSPECIFIED

## 结论

本次无法完成目标 Windows 审计：执行主机实际为 macOS（Darwin 25.4.0），而协议要求采集 Windows 注册表、Credential Manager、WinHTTP、MSIX、Windows 进程和三层环境变量等证据。为避免把 macOS 等价项误报为 Windows 事实，采集在平台预检后停止。

全程保持只读；未登录、注销或刷新账号，未切换 Provider，未重启任何程序，未调用模型推理接口，未执行认证或未认证的 `/models` 探测，也未读取或输出配置、凭据、数据库业务记录或日志正文。

机器标签未填写，因此报告使用安全回退标签 `UNSPECIFIED`。

## 1. 一页摘要

| 项目 | 结果 |
| --- | --- |
| 审计协议 | `codex-ccswitch-audit-v1` |
| 采集模式 | 只读 |
| 目标平台 | Windows |
| 实际平台 | macOS / Darwin 25.4.0 |
| OS 版本 / Build | 26.4.1 / 25E253 |
| 架构 | x86_64 |
| 时区 | Asia/Shanghai |
| 平台匹配 | 否 |
| 完整审计状态 | 阻塞：平台不匹配 |
| 认证模型探测 | 未执行；协议设置为 NO |
| 未认证端点探测 | 未执行；平台预检失败 |

主机名与当前用户名只在 JSON 报告中以长度和协议规定的 16 位指纹表示，未记录原值。

## 2. 实际请求链路

不能确认。未采集 Windows Codex 进程、CC Switch 本地监听、系统代理或上游连接证据，因此不能判断请求实际走官方 OpenAI、CC Switch 本地路由或第三方 Provider。

## 3. 身份与推理凭据分离状态

不能确认。未执行 `codex login status`，未检查 `auth.json`、Windows Credential Manager、官方登录缓存、第三方 Provider Token 或二者的匹配关系。

## 4. 配置来源和优先级

不能确认。未检查 Windows `CODEX_HOME`、项目级 `.codex/config.toml`、ProgramData、策略注册表项或进程启动参数。没有输出或保存任何配置原文。

## 5. CC Switch 当前 Provider

不能确认。未读取 Windows CC Switch 的 `settings.json`、`cc-switch.db`、Provider、Endpoint、代理接管、健康检查或日志聚合数据。

## 6. 模型目录差异

不能确认。未读取 `models_cache.json`、强制模型目录或任何模型菜单缓存。

## 7. 环境变量

不能确认。未采集 Windows Process、User、Machine 三个作用域的环境变量，也未输出当前非 Windows 主机的环境变量。

## 8. 代理、DNS、TLS

不能确认。未采集 WinHTTP、Internet Settings、Clash/Mihomo 配置、DNS、hosts、TLS 证书、监听端口或活跃连接。未发起网络探测。

## 9. 版本与启动时序

不能确认。未盘点 Windows Codex CLI、Desktop/MSIX、VS Code 扩展、npm 安装、包装脚本或相关进程的版本与启动时间。

## 10. 已确认异常

1. `platform_mismatch`：目标是 Windows，实际执行主机为 macOS；这是完整审计的阻塞项。
2. `machine_label_unspecified`：请求中的机器标签仍为占位符，报告采用 `UNSPECIFIED`。

## 11. 尚不能确认的事项

协议列出的登录状态、官方认证缓存、第三方 Provider、CC Switch 接管、模型菜单来源、代理、DNS、TLS、版本差异、启动顺序、缓存新鲜度、端点可达性及全部 invariants 均未在目标 Windows 主机上验证。

要获得可逐字段比较的有效报告，必须在目标 Windows 电脑的本地 Codex 会话中运行同一协议，并填写唯一机器标签。此报告不得作为该 Windows 电脑状态的证据。

## 12. collection_errors

| 阶段 | 代码 | 类型 | 严重性 | 安全说明 |
| --- | --- | --- | --- | --- |
| platform_preflight | platform_mismatch | UnsupportedHostPlatform | fatal | 预期 Windows，实际为 Darwin；已停止 Windows 专属采集。 |
| input_validation | machine_label_missing | MissingRequiredLabel | non-fatal | 标签占位符未替换，使用 `UNSPECIFIED`。 |
