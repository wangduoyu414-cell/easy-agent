# AI 客户端安装助手

一个固定管理五款 AI 桌面客户端的轻量本地安装助手：WorkBuddy、HermesAgent、CC Switch、Claude Desktop（含 Code 页）和 ChatGPT。

当前实现已经具备 Windows x64 原生界面、本机安装检测、在线官方版本解析、嵌入式信任注册表、受控重定向、私有临时下载、摘要/签名验证接口、执行前制品二次绑定和结构化安装命令。尚未取得干净机与签名身份完整证据的产品会在界面中保持“验证待完成”，不会回退 Microsoft Store 引导器、远程脚本或第三方镜像。

## 当前交付状态

- Windows x64：可构建单文件便携 EXE；检测和版本解析已实机冒烟验证。
- Windows ARM64：Rust target 已安装并实际尝试 release 构建；当前主机缺 Visual Studio C++ ARM64/clang-cl 组件，且无 ARM64 真机，暂不作为已交付制品。
- macOS Universal：提供签名/notarization 构建脚本和 Bundle 模板；当前 Windows 环境无法完成 Apple 构建与 Gatekeeper 验收。
- 五款软件的自动安装：生产执行链已实现，但所有条目默认失败关闭；只有对应平台的 clean-machine proof 与身份 pin 完成后，才允许在 `config/trust-registry.toml` 中启用。

详见 [实现与验证状态](docs/implementation-status.md)、[V1 执行任务卡](tasks/AI-CLIENT-INSTALLER-V1.md) 和 [官方分发调研](research/official-distribution-and-installation-research-2026-07-31.md)。

## 本地开发

```powershell
cargo check --all-targets
cargo test --test resolver_fixtures
cargo test --test security_boundaries
```

构建 Windows x64 便携包：

```powershell
.\packaging\build-windows.ps1 -Architecture x64
```

输出写入 `dist/`，不包含任何第三方客户端安装包。

## 设计边界

- 固定五产品，不做通用插件、远程规则平台或后台服务。
- 运行时只解析版本和短期官方地址，不能扩大内嵌 host、包类型、签名主体、公钥或应用身份。
- 不执行服务器返回的 PowerShell/bash；PowerShell 仅用于本地编译的固定 Windows 检测、签名和 MSIX 安装策略，文件路径通过环境变量或结构化参数传递。
- 不镜像、不提交、不长期缓存第三方安装包。
