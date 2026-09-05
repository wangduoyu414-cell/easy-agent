# 安装来源、跨平台身份与关联影响审计

> 后续已完成 Hermes 固定源码与真实运行验证，并重建 easy-agent；最新验证包摘要见[后续证据](hermes-fixed-source-runtime-2026-09-06.md)。本文保留前一阶段检查快照。

日期：2026-09-06（Asia/Shanghai）。基线 `9757d66`，本地分支 `codex/apple-silicon-validation`。本次检查覆盖五款产品 × Windows/macOS × x64/ARM64 共 20 个配置组合。测试主机为 Apple M1；本轮没有 Windows 或 Intel Mac 真机运行验证。本文替代同日首次 Apple Silicon 验证中 WorkBuddy 的失败结论，历史记录不删除。

## 结论与来源要求

可靠来源需要同时满足：固定的官方入口、架构与包型正确、可验证的发布者身份，以及安装后身份与版本复检。HTTPS 下载成功、官网上一个版本号或安装器本身签名正确，都不能单独证明最终应用可信或可重复安装。

- WorkBuddy：两种 Mac 官方包均迁移 Bundle ID，已修复；保留腾讯 Team、签名、架构和版本校验。
- Hermes：官方 Mac setup 有效签名且已公证，但默认使用可变 `main` 分支，尚无已验证的固定版本完整桌面包；本项目缺少 Mac bootstrap 最终桌面/runtime 的安装与复检实现，继续禁用并明确显示原因。
- Claude：当前网络下官方 CDN 超时/受限，既有签名回退元数据可验证；回退也存在低速，不能保证所有网络稳定。没有新增镜像或放宽证书验证。
- ChatGPT、CC Switch：本轮已检查的包未发现类似 Bundle/Package 身份漂移。Windows 签名系统与真实安装仍须在 Windows 上复验。

## WorkBuddy 根因、修复与剩余边界

官方入口仍是 `https://www.workbuddy.cn/v2/update?platform=workbuddy-darwin-{x64,arm64}`，下载固定到腾讯 `download.codebuddy.cn/workbuddy/saas/darwin-{arch}/` 下的版本化 ZIP。

两个架构当前候选都是 `5.5.3.37748631`，包内版本 `5.5.3`，Team `FN2V63AD2J` 不变，Bundle ID 从 `com.workbuddy.workbuddy` 改为 `com.tencent.workbuddy.mac`。此前不仅安装包验证失败，现有新版 WorkBuddy 的本机检测也会失败。

修复涉及注册表、WorkBuddy 专属摘要策略边界与本机检测。只有已安装、经签名验证的同 Team WorkBuddy 可以使用旧 ID；下载/暂存的新包以及安装后的复检仍要求新 ID。不会把任意腾讯应用或其他产品视为 WorkBuddy。

| 官方 ZIP | 实际 SHA-256 | 结果 |
| --- | --- | --- |
| ARM64 | `04938b6f82328ac3682ea5762826642620faa3c23199f70bebdf5bcbf5ee3111` | deep/strict codesign、Gatekeeper、版本与目标架构通过 |
| x64 | `0114dc40c8ebc298644bf8c38bb03d707eb205e1f17329aa5b18a89d16d571f8` | deep/strict codesign、Gatekeeper、版本与目标架构通过 |

两者仍与官方 API 的 `sha256hash` 不符。沿用项目已有、仅限 WorkBuddy/macOS 的 Apple 平台签名策略，不能推广成“忽略所有摘要错误”。错误摘要仍应推动厂商修复。

两个真实 ZIP 均通过生产激活代码的临时首次安装、同版本重复替换、强制失败回滚和失败新装清理。新增测试覆盖旧 ID 仅适用于已安装应用、错误 Team/产品/ID 拒绝，以及摘要策略不能扩大到其他身份。没有真实旧 ID 签名样本，因此不能宣称跨 Bundle ID 的实际旧版升级已通过。

本机 `/Applications/WorkBuddy.app` 5.4.7 已恢复正常检测，界面显示“可更新至 5.5.3”。运行中应用的 preflight 正确拒绝覆盖。本轮未退出或替换用户的 WorkBuddy。

## Hermes：安装器不等于最终应用

检查来源：

- [官方首页](https://hermes-agent.nousresearch.com/)：当时显示 0.21.0，并链接官方域名上的 `Hermes-Setup.dmg`。
- [正式 Release v2026.8.31](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.31)：非 prerelease，但 API 返回没有二进制附件。
- 上游源码固定到提交 `9dd6634c5635321cf38840cc30e9b51226689128`，避免用检查后的 main 变化解释当时结果。

实际 DMG 里的 `Hermes.app` 是 ARM64 setup：版本 `0.0.1`，Bundle `com.nousresearch.hermes.setup`，Team `T2F6S8MF7C`，Developer ID 主体 Brooklyn Nicholson；deep/strict codesign 通过，Gatekeeper 接受 Notarized Developer ID。

实际启动日志明确为 `Pin { commit: None, branch: Some("main") }`，随后从 GitHub main 下载 `install.sh`。首次 GUI 操作实际进入用户默认 `~/.hermes`，完成 prerequisites 后开始克隆仓库；发现来源未固定后终止了本次安装器及其子进程，未完成桌面安装。临时 `HERMES_HOME` 不能保证另一次由系统启动的 GUI 继承它。用户目录可能留有厂商安装器的前置依赖、缓存和日志，未递归删除该目录或清除未知用户数据。

源码关联证据：

- [build.rs](https://github.com/NousResearch/hermes-agent/blob/9dd6634c5635321cf38840cc30e9b51226689128/apps/bootstrap-installer/src-tauri/build.rs)：不可变 commit 需要构建时显式选择，普通构建可只记录分支。
- [store.ts](https://github.com/NousResearch/hermes-agent/blob/9dd6634c5635321cf38840cc30e9b51226689128/apps/bootstrap-installer/src/store.ts)：普通 GUI 启动传入 `commit: null`、`branch: null`，使用构建默认值。
- [install_script.rs](https://github.com/NousResearch/hermes-agent/blob/9dd6634c5635321cf38840cc30e9b51226689128/apps/bootstrap-installer/src-tauri/src/install_script.rs)：按 commit/branch 获取脚本，移动分支重取，网络失败有缓存回退。
- [install.sh](https://github.com/NousResearch/hermes-agent/blob/9dd6634c5635321cf38840cc30e9b51226689128/scripts/install.sh) 和 [install.ps1](https://github.com/NousResearch/hermes-agent/blob/9dd6634c5635321cf38840cc30e9b51226689128/scripts/install.ps1)：Electron 下载失败存在 `npmmirror.com` 回退。Mac 最终 Electron 应用可由脚本本地构建并 ad-hoc 签名。

因此，“官网 0.21.0”“setup 0.0.1”“最终桌面及 Python runtime 版本”必须分开。本项目直接应用包路径要求包内版本与候选一致，且复检固定 Bundle/Team；仅打开 `enabled` 或把 setup 复制到 Applications 都无法解决问题。

Windows x64 当前 EXE 的 PE 架构为 x64，证书表中的主体仍为 `Nous Research Inc.`，但 Windows 脚本也有可变源码/依赖与第三方回退，属于关联风险。本轮没有运行 Windows EXE，不能断言它与实测 Mac 二进制的编译 pin 完全相同。项目原有 Windows 安装成功证据不等于“当前最终运行时固定到正式 Release”。Windows ARM64 继续禁用；Intel Mac 继续不支持。

后续开发应先取得官方支持的固定 commit/release 安装入口或已签名完整桌面包，再分别定义 setup 成功、最终 desktop 身份/架构/版本、runtime 健康与失败/取消结果。不能把脚本的退出码当最终安装完成。固定顶层 commit 也还需检查依赖是否可变；本轮不重写或接管厂商整套安装脚本。

## 四平台检查矩阵

“解析”只指生产适配器返回候选；“静态”不包含 Windows WinVerifyTrust、UAC、注册、重启或客户端运行。

| 产品 | macOS ARM64 | macOS x64 | Windows x64 | Windows ARM64 |
| --- | --- | --- | --- | --- |
| WorkBuddy | 官方 5.5.3.37748631；身份修复、真实 ZIP 验证、临时激活/回滚通过 | 同左；M1 检查 x64 包，未运行 Intel 客户端 | 官方版本化 EXE 已下载；x86 安装引导器、腾讯证书主体符合既有合同；非 Windows 签名验收 | 无已接入的官方原生包，禁用 |
| Hermes | 官方 setup 签名/公证通过；main 与最终应用验证阻塞，禁用 | 官方不支持 | 官方固定路径 EXE；证书主体/x64 静态检查；最终下载链有上述关联风险 | bootstrap 真机未证明，禁用 |
| CC Switch | 官方 3.20.1 universal tar；更新签名、Apple 身份、临时激活通过 | 当前 tar 更新签名、Apple 身份、x64 切片通过 | 官方 3.20.1 MSI 已下载，生产 minisign 验证通过；本轮未检查 MSI 安装属性/运行 | 仍缺真机验证，禁用 |
| Claude | 1.46388.4 universal DMG；当前 ARM64 制品临时激活/回滚通过 | 同版候选/签名回退解析通过；本轮未新增 Intel 客户端运行证明 | 官方 MSIX 解析；直连超时；同版签名回退元数据通过，完整下载未完成 | 同左，目标架构 arm64 |
| ChatGPT | 官方 26.901.41600 ZIP；Sparkle、Apple 身份、临时激活通过 | 当前 ZIP Sparkle/Apple 身份/x64 切片通过 | Store ID 与官方完整 MSIX 路径；26.901.5280.0 身份/架构静态检查通过 | 同版 ARM64 MSIX 静态检查通过；真机未测 |

Claude Windows 官方连接 75 秒内未完成；回退下载各 240 秒后仍只有约 53–68 MB / 261–267 MB，未取得完整包，不能宣称其 ZIP、摘要或平台签名通过。签名清单锁定版本 1.46388.4，x64 SHA `f3925248cf40b46c59043878b4c4f1835e7687082b5a52217bd72b73bfbf0b12`，ARM64 SHA `26584e2b97e80df3e9ac3c745b806b9711aefb74b079af7c6687286734ebd327`；这两个值是清单声明，不是本轮实测完整文件摘要。现有 IP 回退不是厂商官方下载；它必须继续经过项目签名清单、候选一致性、文件摘要和厂商平台签名检查。

ChatGPT 两个 Windows MSIX 实际 Identity 为 `OpenAI.Codex`、Publisher `CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B`、版本 26.901.5280.0，架构分别 x64/arm64；Windows.Desktop 最低 10.0.19041.0，签名文件存在。未发现身份漂移，但不能根据 macOS 读取 manifest 宣称 Windows 信任链有效。

## 可重复核查与交付

新增只读 `examples/distribution_audit.rs`，复用生产解析/回退验证，输出 20 行 JSONL，不下载、不安装。参考系统版本是 Windows 11 build 26100 和 macOS 26.6.2，不能代表所有旧系统。输出区分解析、Store 计划、禁用、本地制品摘要及更新签名；明确 `platform_signature_checked: false`。

```sh
cargo run --locked --example distribution_audit
# 可选：审计既有文件 PRODUCT-OS-ARCH.EXT，以及生产规则允许的签名回退。
EASY_AGENT_AUDIT_ARTIFACT_ROOT=/absolute/path/to/artifacts \
EASY_AGENT_AUDIT_FALLBACK=1 cargo run --locked --example distribution_audit
```

最终构建：101 项测试通过、7 项默认忽略；fmt、严格 Clippy、Intel/ARM64 all-targets check 与双架构 release 构建通过。两个架构 WorkBuddy 真实制品测试另外显式运行通过。界面确认已安装 WorkBuddy 与更新版本显示正常，Hermes 显示“安装源未固定稳定版本，暂不可用”。

新 DMG：`easy-agent-macos-universal-UNNOTARIZED-VALIDATION.dmg`，SHA-256 `3d481a674bd9581bc7e073e3aea485f646be7dcf4e4b36e946ffbe49e00c63e3`。仍为 ad-hoc 签名的本地验证制品，未公证、未发布。

本机原始日志/JSON/已下载样本保存在 `/Users/zj/Library/Caches/easy-agent/vendor-proof/`，构建制品在相邻 `dist/`。日志及大安装包不提交仓库。关键日志包括 `workbuddy-{arm64,x64}-activation.log`、`workbuddy-installed.log`、`workbuddy-preflight.log`、`{cc_switch,chatgpt}-intel-current.log`、`distribution-audit-full.jsonl`、`final-build.log` 和 `final-check-{intel,arm64}.log`。原始 JSON 保留检查当时的配置原因，新提示以当前注册表为准。
