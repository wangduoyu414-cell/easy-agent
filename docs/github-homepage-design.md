# GitHub 首页设计记录

日期：2026-08-04

目标：让首次访问者在一分钟内理解 `easy agent` 的对象、平台、安装路径、真实可用状态和安全边界，同时让贡献者知道应从哪里继续。

## 参考项目与观察

| 项目 | 已观察的主页做法 | 适合 easy agent 的原因 |
| --- | --- | --- |
| [Ollama](https://github.com/ollama/ollama) | 品牌图、直接的 Download 分区、按平台拆分的最短路径、文档/生态入口 | 用户先关心“它是什么、怎样得到它”；本项目采用显眼品牌、平台安装表与独立安装指南。 |
| [Tauri](https://github.com/tauri-apps/tauri) | 状态/许可证/社区徽章、Introduction、Getting Started、Features、Platforms、Contributing | 将稳定事实与开发入口并列；本项目采用不夸大的状态徽章、功能/平台表和贡献入口。 |
| [LocalSend](https://github.com/localsend/localsend) | 快捷锚点、About、Screenshots、Download 矩阵、How It Works、Troubleshooting、Build | 信息密度高但先给下载；本项目用品牌视觉代替无效截图，提供安装矩阵、工作流程和详细构建文档。 |
| [RustDesk](https://github.com/rustdesk/rustdesk) | 顶部品牌、产品截图、社区链接、构建文档和贡献提示 | 视觉与信任信息能降低首次理解门槛；本项目加入应用图标、清晰价值主张和安全/贡献入口。 |

以上 README 均于 2026-08-04 通过各自 GitHub 默认分支读取；它们是信息架构参考，不代表本项目与其有任何合作或背书关系。

## 本项目的取舍

已采用：

- 顶部品牌图、产品名、一句中英文价值主张和少量事实徽章；
- 前置的安装路径与“当前无正式签名发布包”的醒目说明；
- 平台/交付状态矩阵、工作流程图和安全边界；
- 不让 README 承担所有细节：将完整安装步骤、维护规则和状态证据链接到独立文档；
- 贡献入口与安全变更要求，便于问题、文档和测试贡献。

刻意未采用：

- 不显示虚假的下载量、星标数、测试通过徽章、兼容平台或“stable”标签；
- 不提供 `curl | sh`、绕过 Gatekeeper 或忽略签名的安装指令；
- 不放置无法复现的产品截图，也不使用第三方客户端商标作为 `easy agent` 的应用图标；
- 不把尚未闭合的 macOS 厂商安装 Gate 表述为已完成。

## 建议同步到 GitHub 仓库设置的元数据

以下值应在准备发布时通过仓库 Settings 或 `gh repo edit` 设置；它们不是代码生成的事实，也不应在未确认时自动宣称已经生效：

```text
Description: Fail-closed installer assistant for five AI desktop clients on Windows and macOS.
Topics: ai, desktop, installer, rust, eframe, windows, macos, security
Homepage: 仅在拥有独立项目网站时设置；不重复填写 GitHub 仓库 URL。
```

发布后还应补充：已签名 Release、版本说明、SHA-256、macOS notarization 状态、Windows Authenticode 状态和可复现的 CI 链接。只有这些证据真实存在时才可把 README 徽章从 `validation-gated` 改为生产状态。
