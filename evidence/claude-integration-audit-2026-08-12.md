# Claude 接入与私有镜像交叉审计（2026-08-12）

## 结论

Claude 已按“客户端官方入口优先、四个平台仅在明确可用性失败时进入受验证香港回退”的窄路径接入。Windows 不直接绕过厂商流程部署企业包，而是先下载并验证个人 Setup 所对应的同版本完整 MSIX，再通过 Setup 固定支持的 `--msix-path` 使用本地载荷；这样保留首次安装、按需 UAC 和 Cowork 机器级服务注册流程，同时消除 Setup 启动后再次联网下载约 253 MB 的依赖。香港节点只提供固定安装制品，不代理登录或运行流量，也不代表已经取得 Anthropic 的再分发许可。

## 当前部署

- 东京节点通过 SSH forced-command 只解析 Windows x64/ARM64 Setup、Windows x64/ARM64 MSIX 和 macOS Universal DMG 五个固定目标。
- 香港节点不运行通用代理，也不接受客户端提交 URL；每 30 分钟单实例同步，单个平台失败时保留该平台最后一次成功清单。
- Windows 同步侧验证 Setup 的原生 PE 架构、固定版本资源、Authenticode 完整性与 Anthropic 签名，并验证完整 MSIX 的 Identity、Publisher、架构、版本和 SHA-256；macOS 同步侧验证 DMG、Bundle、版本、最低系统版本和 Universal 双切片。
- 香港 Nginx 只开放四组精确 manifest/signature 路径，以及严格限定的 `ClaudeSetup.exe` / `Claude.msix` / `Claude.dmg` 不可变摘要路径。
- HTTPS 使用 Let's Encrypt IP 证书，Certbot 自动续期已启用；客户端固定 `43.161.214.205`、minisign 公钥与最长 7 天陈旧期，普通 UI 只显示版本和状态。
- 公网清单验签、完整下载、SHA-256、Range `206`、错误路径 `404` 和禁止写入均已验证。

当前 `1.26832.0` Windows Setup：

| 架构 | 字节数 | SHA-256 |
| --- | ---: | --- |
| x64 | 7,020,704 | `c91e8b5d92cf60aa5e688f4e6bb6053b247c29e252724034656a4889df97c814` |
| ARM64 | 6,471,840 | `a7ddb6419c78fad206b3fe1eb214afc505a91ccdcefc13e5ba8df76600fe7109` |

当前与 Setup 绑定的完整 MSIX：

| 架构 | 字节数 | SHA-256 |
| --- | ---: | --- |
| x64 | 266,210,150 | `6dc210bca31b55c9fa307d11c6b13a42c7f3a3886ccc35ca2ecb7e9fceba0139` |
| ARM64 | 261,248,276 | `16f9f9b074b1ce1bbc67c7facfd2279825c6b15b6abee28ccf7fedffe3e23a23` |

## Anthropic 当前分发说明

### Windows

Anthropic 个人下载链提供：

- x64：`https://claude.ai/api/desktop/win32/x64/setup/latest/redirect`
- ARM64：`https://claude.ai/api/desktop/win32/arm64/setup/latest/redirect`

当前 Setup 是对应架构的原生 PE，并由 `Anthropic, PBC` 签名。静态证据确认 Setup 支持 `--msix-path`，其内部提权链也会把该本地路径传给 elevated 进程。项目因此先下载并验证完全同版本 MSIX，再让官方个人 Setup 使用本地载荷；客户端不直接调用 `Add-AppxPackage` 安装 Claude。Setup 仍负责签名复核、按需 UAC、Cowork 机器级服务注册和无管理员权限基础安装路径。

### macOS

官方仍提供 Universal DMG。当前项目使用面向普通用户的 `DirectAppBundle` DMG 路径；企业 PKG 需要另一套提权、安装收据、恢复和复检合同，不能直接塞入现有 DMG 执行器。

### 地区边界

中国大陆不在 Claude 当前官方支持地区列表中。回退只解决安装包获取，不能代理账号登录、订阅校验或客户端运行流量，也不能承诺服务本身可用。

## 客户端落点与交叉影响

1. **官方入口**：Windows 固定 Setup redirect，macOS 固定 Universal DMG redirect；镜像域与官方域完全分离。
2. **执行链**：Windows 下载并验证 Setup → 下载并验证同版本完整 MSIX → 以结构化 `--msix-path` 参数运行厂商 Setup → 精确复检 Claude Package Family、Identity、Publisher、架构和版本。进入厂商安装窗口后不再需要下载代理。
3. **UAC/Cowork**：确认页只说明可能出现管理员授权；允许授权时可完整注册服务，没有管理员权限时仍可完成基础安装，不再把功能缺失误归因于 `easy agent`。
4. **回退分类**：只允许地区限制、403/451、408/429、5xx、连接、超时和响应体中断；安全、证书、签名、合同、身份和白名单错误禁止回退。
5. **并发体验**：Claude Setup 属于厂商 EXE，可与 WorkBuddy、Hermes、ChatGPT Store 或 macOS 应用安装直接并行；只有 MSI 与 MSI、MSIX/Store 与 MSIX/Store 发生同类系统写入冲突时才分别排队，排队期间可单独取消。
6. **服务边界**：客户端不能触发同步、提交 URL、新增产品或下载服务端命令；镜像不是通用下载平台。

## 已完成与剩余硬门

已完成：四平台受限回退配置、Windows Setup 与完整 MSIX 双架构同步、schema 2 同版本绑定、客户端 Setup/MSIX 双重验证和本地参数交付、macOS Universal 同步、公网清单验签、两种架构完整 MSIX 下载摘要、Range `206`、错误路径 `404` 和禁止写入 `403` 边界。

仍需完成：Windows x64/ARM64 干净机的真实首次安装、旧版更新、UAC 允许/拒绝、无管理员权限基础路径和最终 Package postcheck；macOS Intel/Apple Silicon 真机安装矩阵；Anthropic 再分发条款与云服务 AUP 的正式合规判断；IP 证书持续外部到期监控。
