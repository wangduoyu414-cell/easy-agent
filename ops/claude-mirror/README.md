# Claude 四平台受验证回退同步

这套服务只同步五个固定上游制品，并为四个客户端平台发布独立清单：

- Windows x64 官方个人 Setup
- Windows ARM64 官方个人 Setup
- Windows x64 同版本完整 MSIX
- Windows ARM64 同版本完整 MSIX
- macOS Universal DMG，同时发布 Intel 与 Apple Silicon 清单

东京节点只接受 `resolve windows x64`、`resolve windows arm64`、`resolve windows x64 msix`、`resolve windows arm64 msix`、`resolve macos universal` 五个强制命令。它优先访问 Anthropic 文档公开的 `claude.ai` 入口；公开入口触发地区页或 challenge 时，才使用固定的私有 `api.anthropic.com` 兼容解析入口。客户端信任注册表始终只包含公开入口和香港清单，不包含私有兼容入口。

香港节点从最终固定 `downloads.claude.ai/releases/...` URL 下载完整包，拒绝降级和同版本 URL 静默变化，验证后生成 minisign 清单：

- Windows：Setup 验证原生 PE 架构、版本资源、Authenticode 完整性和 Anthropic 签名；MSIX 验证 ZIP/manifest 完整性、Identity、Publisher、架构、版本与 SHA-256。schema 2 清单要求 Setup 和 MSIX 版本一致；Windows 客户端仍必须重复验证 Setup 与 MSIX 的 AppX 签名/身份，再通过 `--msix-path` 交给 Setup，并在执行后复检最终 Claude Package Identity/Family/Publisher、架构和版本。
- macOS：DMG 可解析性、`Claude.app`、Bundle ID、版本、最低 macOS 版本、代码签名资源存在，以及主程序同时包含 x64/ARM64 slice；Mac 客户端仍必须验证 codesign、Team ID 与 Gatekeeper。

服务边界：

- 不启动 sing-box，不开放代理端口。
- 不允许客户端触发同步，也不接受任意 URL。
- 每 30 分钟单实例同步；单个平台失败不会阻止其他平台尝试，失败时保留最后一次成功清单。
- Nginx 只公开四组精确清单路径及严格限定的 `ClaudeSetup.exe`、`Claude.msix`、`Claude.dmg` 不可变安装包路径；无目录浏览、反向代理或远程规则。
- 回退只解决安装制品获取，不代理 Claude 登录或运行流量，也不改变 Anthropic 的地区支持范围。

私有数据布局：

```text
/srv/easy-agent/private/artifacts/claude/windows/{x64|arm64}/<version>/<sha256>/ClaudeSetup.exe
/srv/easy-agent/private/artifacts/claude/windows/{x64|arm64}/<version>/<sha256>/Claude.msix
/srv/easy-agent/private/artifacts/claude/macos/{x64|arm64}/<version>/<sha256>/Claude.dmg
/srv/easy-agent/private/manifests/claude/{windows|macos}/{x64|arm64}/latest.json
/srv/easy-agent/private/manifests/claude/{windows|macos}/{x64|arm64}/latest.json.minisig
```

公网入口遵循同一结构，例如：

```text
https://43.161.214.205/manifests/claude/windows/arm64/latest.json
https://43.161.214.205/manifests/claude/macos/arm64/latest.json.minisig
https://43.161.214.205/artifacts/claude/windows/x64/<version>/<sha256>/ClaudeSetup.exe
https://43.161.214.205/artifacts/claude/windows/x64/<version>/<sha256>/Claude.msix
https://43.161.214.205/artifacts/claude/macos/x64/<version>/<sha256>/Claude.dmg
```

部署要求：香港节点安装 `curl`、`python3`、`7zip`、`minisign`、`osslsigncode`、OpenSSH client 与 Nginx；东京节点安装 `curl`、`python3` 和 OpenSSH server。私钥只存在香港服务器的受限配置目录；仓库只保存 [`mirror-signing.pub`](mirror-signing.pub)。

常用验证：

```bash
sudo systemctl start easy-agent-claude-sync.service
systemctl status easy-agent-claude-sync.service
systemctl list-timers easy-agent-claude-sync.timer --all
sudo nginx -t
```

公网服务器使用 IP HTTPS 证书，必须保持 Certbot 自动续期和外部健康检查。清单不缓存；摘要路径安装包使用 immutable 缓存并支持 Range。扩大传播或长期商用前，运营者仍需完成对应软件条款和云服务 AUP 审核。
