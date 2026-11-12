# macOS 下载排查（2026-11-12）

基于远端 main 的 07308c6（preview.5）在 Apple Silicon 上复查。区分是否启用、元数据解析、包下载、验证和安装，不用一个状态代表整条链。

| 产品 | 实测与根因 | 处理 |
| --- | --- | --- |
| CC Switch | 历史操作日志连续三次在下载前因运行中而退出；当前相同预检仍能复现。官方 4.0.30 的完整 Universal tar.gz 在 ARM64 验证通过（更新签名、Bundle 身份、Team、架构、codesign/Gatekeeper）。 | 先保存已验证安装包，到激活前才拒绝替换运行中的应用。 |
| Claude | 官方桌面重定向在本机返回 403，响应标有 Cloudflare challenge；原备用 IP 的 TLS 证书与该 IP 不匹配。本机 Claude 也正在运行。 | 增加官方 CDN 索引路径。索引声明 5.1231.2 的 Universal ZIP；严格限定版本、域名和 40 位十六进制文件名，探测同名 DMG，仍需通过完整平台校验。不放宽证书或区域回退规则。 |
| WorkBuddy | 当前两架构官方元数据及两份 ZIP 均可开始下载。本机预检因运行中拒绝替换。 | 使用相同的延后安装检查；保留现有腾讯身份和签名限定。 |
| ChatGPT | 官方双架构 appcast 与相应 ZIP 字节范围请求通过。本机已安装客户端也因运行中无法通过原预检。 | 下载与安装检查分开；本次没有重新验证完整的约 820 MB ARM64 ZIP，也未替换客户端。 |
| Hermes、Clash Verge Rev | macOS 产品在注册表中被禁用，解析器不会尝试下载。 | 未把未验证的安装链改成可用。 |

项目自己的解析器在修复后为所有已启用的八个 macOS 产品/架构组合返回官方候选。范围请求只验证当前网络下文件存在且可传输，不保证完整下载、安装成功或其他网络可用。macOS 阶段测试、Clippy、格式检查及双架构本地验证构建是独立检查。未操作备用节点、未安装到用户 Applications。

完整包复验：Claude 的 385,334,560 字节官方 DMG 与 WorkBuddy 的 447,126,372 字节官方 ARM64 ZIP，均在本次单连接 180 秒探测超时后以固定 ETag 和逐段 Content-Range 检查重取，未将截断文件作为证据。项目的 macos_artifact_proof 对完整文件执行 ARM64、版本读取和 Apple 平台验证：Claude 5.1231.2 / com.anthropic.claudefordesktop / Q6L2SF6YDW；WorkBuddy 5.9.3 / com.tencent.workbuddy.mac / FN2V63AD2J。分段取样工具仅用于诊断，不改变应用下载策略。CC Switch 4.0.30 完整包同样通过检查，Team 为 R8UR22V2F9。

本地验证 DMG 的 SHA-256 为 753a317f56d5ca46825d810e0ba8e38c965dd90b0ca7ae29d13e69a6192f1bd8，核实存在 x86_64 和 arm64 切片。119 项测试通过、8 项默认忽略；严格 Clippy、格式检查通过。产物为未公证的验证包。

后续更新（同日）：Clash Verge Rev 与 Hermes 新增独立的“下载安装包”按钮，上表中两者的禁用状态描述的是修复前的自动安装入口。用与按钮相同的下载函数实际从官方来源下载并保存以下完整文件；均通过 Apple 签名、Gatekeeper、固定 Team / Bundle 和目标 Mach-O 架构检查，没有执行安装器：

| 保存到系统下载目录的文件 | SHA-256 | 发布者 Team |
| --- | --- | --- |
| Clash.Verge_2.5.10_aarch64.dmg | 40d4a7da4543d75f573a5cfcc31543b0b4bf7e02caa122a9ca50654859585839 | JPH3Z7PPBB |
| Clash.Verge_2.5.10_x64.dmg | b1d6938680f3e411b5961e56591faaa92c95e9676f54ebfdfa98b7eab374023e | JPH3Z7PPBB |
| Hermes-Setup-arm64.dmg | b61e047efe3059faf1c55fec3252e661f2d2a993a7a3eebf5cc6a9aa5c1790f5 | T2F6S8MF7C |

Clash 以 GitHub 官方稳定版发布记录的实际资产大小和 SHA-256 为额外校验，并验证内含应用版本。Intel 包在 Apple Silicon 主机上验证了签名和架构，未做 Intel 真机启动。Hermes 校验对象为官网提供的 setup，不能由此推断其下游软件版本或已安装。上述早期助手 DMG 摘要仅对应前一轮构建；请使用当前 dist 产物的同名 SHA256SUMS 文件。
