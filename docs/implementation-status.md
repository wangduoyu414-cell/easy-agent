# 实现与验证状态

更新时间：2026-07-31

## 已实现

- Rust + egui 原生桌面程序，Windows 使用静态 CRT，发布目录只需一个 EXE。
- Windows x64/ARM64 与 macOS Intel/Apple Silicon 平台模型。
- Windows 已安装检测：精确 AppX family/name 和 HKCU/HKLM Uninstall 注册信息，不调用 `Win32_Product`。
- 四类真实在线解析器：WorkBuddy 结构化更新接口、Hermes 官方首页、CC Switch 签名更新清单、Claude 官方稳定 MSIX 重定向。
- ChatGPT Windows 明确 No-Go：未完成 Microsoft 目录依赖闭包和 entitlement proof 时不打开 Store、不下载引导器。
- 版本化嵌入式 `trust-registry`，远端响应只能在固定边界内提供易变版本和地址。
- HTTPS、逐跳 host/path 校验、最多五次重定向、4 MiB 元数据限制、2 GiB 安装包限制。
- 每次随机私有暂存目录、`.part` 写入、拒绝路径逃逸/符号链接/Windows reparse point。
- SHA-256、CC Switch minisign 流式验证接口、Windows Authenticode/AppX 验证、EXE PE 架构和 MSIX manifest 身份读取。
- 安装启动前再次核对稳定路径、长度、摘要和平台签名；MSI/EXE/MSIX 使用结构化进程参数，不拼接 shell 命令。
- UI 安装前确认、顺序批次、下载取消、验证、安装、postcheck、单项失败隔离与批次摘要已接到真实执行路径。
- 已装更高版本、受组织管理、管理状态未知或现有版本未知时在下载前失败关闭；相同版本不重复安装。
- 中文字体、长状态换行、真实检测/解析后台线程和失败关闭界面。

## 2026-07-31 本机观察

环境：Windows 11 Pro build 26100，x64。

| 产品 | 检测结果 | 官方解析结果 | 当前动作 |
|---|---|---|---|
| HermesAgent | 未检测到注册安装 | `0.19.1` | disabled，待 desktop/runtime 干净机复检 |
| Claude Desktop | 已安装 `1.24012.1.0` | `1.24012.9` | disabled，待 Publisher/Family pin 与 clean install |
| ChatGPT | 已安装 `26.721.11231.0`，family `OpenAI.Codex_2p2nqsd0c76g0` | No-Go | disabled，禁止回退 Store 引导器 |
| WorkBuddy | 未检测到注册安装 | `5.3.5.34189228` | disabled，待 clean install/postcheck |
| CC Switch | 已安装 `3.17.0` | `3.19.1` | disabled，待当前 MSI 签名与 clean install/postcheck |

这些版本是当日在线观察，不是代码常量。

## 验证待完成

- Windows x64 干净机：逐产品真实下载、平台签名、交互安装/取消、安装后身份和版本复检。
- Windows ARM64：release cross-build 已尝试；需补装 Visual Studio C++ ARM64/clang-cl 编译组件后重新构建，并完成签名与 ARM64 真机矩阵。
- macOS：五产品 Bundle ID/Team ID pin、Universal 构建、Developer ID 签名、notarization、quarantine 与 Gatekeeper。
- ChatGPT Windows：完整 Microsoft Catalog/FE3 依赖闭包、摘要/签名、授权和无 Store UI 直接安装硬门。
- 正式发布签名：Windows Authenticode 证书与 Apple Developer ID/notary 凭据未提供。

因此当前状态是：`Implementation complete for the Windows x64 foundation; product-install and cross-platform validation pending`。当前 EXE 是开发测试产物，不应标称为最终签名发行版。
