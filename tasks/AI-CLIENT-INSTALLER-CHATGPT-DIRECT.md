---
task_contract_version: 3
card_id: "AI-CLIENT-INSTALLER-CHATGPT-DIRECT"
title: "让 ChatGPT Windows 与其他产品一样一键直连安装或更新"
status: "superseded"
work_kind: "mixed"
execution_target: "agent-executable"
complexity: "complex"
product_risk: "L4"
orchestration_risk: "O2"
execution_profiles: [stateful-runtime, external-boundary, ui-workflow, configuration]
external_review_policy: "optional"
repo_root: "E:\\Obsidian\\workspaces\\ai-client-installer"
blocked_by: []
---

> 2026-08-12 superseded：本卡记录的是已经废止的直接 MSIX 设计。当前实现改为“验证并启动绑定固定 Product ID 的微软轻量安装器；仅在明确的微软网络/分发服务失败时使用官方完整 MSIX 与离线许可证兜底”，不再依赖 WinGet/App Installer 自愈。见 `evidence/chatgpt-windows-store-recovery-2026-08-11.md` 和当前实现文档。

# 1. 任务身份与就绪状态

- `objective_id`: `OBJ-CHATGPT-ONECLICK-001`
- `readiness`: Ready；允许实现、只读取证、自动测试和未签名测试构建。当前主机不执行真实 ChatGPT 安装或更新，缺少可丢弃真机时状态只能是 `Implementation complete; validation pending`。
- `authority_sources`: 用户确认“一个按钮：下载+安装，所有流程在后台完成”；用户于 2026-08-02 进一步确认“不依赖 Windows 商店、不要引导包、使用之前成功过的路径”；本机 AppX 部署日志中多个本地 ChatGPT MSIX 成功记录；已安装且签名有效的 OpenAI 客户端内置 Windows 更新合同；OpenAI 官方 `persistent.oaistatic.com` 更新清单与发布路径；仓库当前实现。
- `decision_owner`: 用户决定单按钮体验以及禁止 Store/WinGet/引导器依赖；执行者在固定官方来源、失败关闭和最小责任边界内选择实现细节。
- `material_unknowns`: OpenAI 未来是否改变官方清单 schema、路径或包身份；Windows ARM64 真机部署结果；这些变化通过合同校验失败关闭，不允许自动猜测备用下载源。
- `CHG-CHATGPT-002`: 用户最新指令废止原 Store/WinGet 主路径、自愈和闭包兜底合同；相关旧 TASK/TEST/EV/GATE 语义全部由本卡当前版本替代。
- `DEC-CHATGPT-002`: Windows ChatGPT 默认路径固定为“OpenAI 官方更新清单 → 对应架构完整 MSIX → 本地 AppX 部署 → 精确复检”，不得调用 Microsoft Store、WinGet、`get.microsoft.com` 引导器或第三方镜像。依据为用户 2026-08-02 明确确认及本机既有成功部署证据。

# 2. 业务目标

- `actor`: 只想点击一次、不需要理解 Store、WinGet、MSIX 或内部安装通道的 Windows 用户。
- `workflow_and_trigger`: 用户在 ChatGPT 行点击唯一“安装”或“更新”按钮；安装助手后台检测系统与架构、解析官方最新版本、下载完整包、验证、部署并复检。
- `single_outcome`: ChatGPT 与其他直接包产品保持同一认知模型：一个按钮完成下载和安装/更新，成功后显示真实已安装版本。
- `observable_results`: `RESULT-CHATGPT-001` 未安装时可直接安装；`RESULT-CHATGPT-002` 已安装旧版本时可直接更新；`RESULT-CHATGPT-003` 已是最新版时不重复安装；`RESULT-CHATGPT-004` 全流程不打开 Store UI、不触发 Windows/微软账户登录、不运行引导包；`RESULT-CHATGPT-005` 安装后以真实 AppX 身份、Publisher、架构和版本复检结果为准。
- `non_goals`: ChatGPT Classic、PWA/WebView 替代、第三方镜像、FE3 私有抓包、Store/WinGet 自愈、系统策略绕过、自动登录、长期缓存或再分发 OpenAI 包、当前主机真实安装。

# 3. 需求质疑与确认

- `user_statement`: 不增加认知成本，只保留一个下载+安装按钮；下载路径不依赖 Windows 商店，不使用引导包，优先复用已经成功过的完整本地包部署路径。
- `REQ-CHATGPT-001` (`required_behavior`): UI 只显示一个与检测状态匹配的主动作，不显示下载通道选择。
- `REQ-CHATGPT-002` (`required_behavior`): 运行时读取固定 OpenAI 官方更新清单，根据系统架构构造并下载对应的完整 `ChatGPT-{arch}.msix`。
- `REQ-CHATGPT-003` (`required_behavior`): 清单必须校验 schema、版本格式和 `OpenAI.Codex` 包身份；下载地址必须落在固定 HTTPS host 与路径前缀内。
- `REQ-CHATGPT-004` (`required_behavior`): 下载后必须校验 AppX 签名、Package Identity、Publisher、Package Family、架构和版本，再使用结构化本地 `Add-AppxPackage` 部署。
- `REQ-CHATGPT-005` (`required_behavior`): 安装完成后重新检测固定 Package Family、Publisher、目标架构和不低于候选版本；进程退出码 0 不能单独构成成功。
- `INV-CHATGPT-001`: 不调用 Microsoft Store UI/URI、WinGet、`get.microsoft.com` 引导器，不执行服务器返回命令，不修改 Store/WinGet 或系统策略。
- `INV-CHATGPT-002`: 当前主机只做读取、官方元数据请求、HEAD 验证、代码测试和构建；不得真实安装或更新 ChatGPT。
- `INV-CHATGPT-003`: 远端清单只能提供版本和固定合同内的元数据，不能扩大 host、路径、包类型、Publisher、Identity 或架构权限。
- `material_ambiguities`: 无需用户选择内部通道；ARM64 真实部署属于验证缺口，不改变 x64 实现合同。
- `decisions_and_authority`: `DEC-CHATGPT-002` 已确认；旧 Store 方案作为被废止证据保留在历史 diff/执行证据中，不再是运行时合同。

# 4. 业务场景与规则

- `SCN-CHATGPT-001` 首次安装: 未检测到目标包；解析官方清单、下载匹配架构 MSIX、验证、部署并复检成功。
- `SCN-CHATGPT-002` 更新: 已安装版本低于清单版本；允许关闭正在运行的 ChatGPT，部署完整新 MSIX 并复检新版本。
- `SCN-CHATGPT-003` 已最新: 已安装版本不低于清单版本；直接返回“已是最新版本”，不下载、不部署。
- `SCN-CHATGPT-004` 合同或网络失败: schema、版本、身份、URL、签名、Publisher、架构、下载、部署或复检任一失败即停止该产品；不得退回 Store、引导器或第三方源。
- `RULE-CHATGPT-001`: 每次点击最多解析一次候选、下载一次、部署一次；重复点击由现有批次互斥处理。
- `RULE-CHATGPT-002`: 清单 URL、包 host/路径、Package Identity/Family/Publisher 和允许包类型随安装助手版本管理；动态版本不得修改这些边界。
- `RULE-CHATGPT-003`: 取消只覆盖自有下载和未启动步骤；AppX 部署开始后不强制杀死系统部署进程。
- `RULE-CHATGPT-004`: 安装命令必须使用本地已验证文件的结构化参数；运行中的目标应用可由 AppX 部署参数关闭，不能依靠交互安装向导。
- `risk_sensitive_invariants`: 任一执行前签名、身份、Publisher、架构或二次文件绑定失败均不得启动部署；下载文件仅存在于本次私有暂存目录并在会话结束后清理。
- `inapplicable_faces_with_reason`: 无云账号、数据库、卸载、回滚或跨用户安装；登录弹窗不属于本流程，程序不得主动触发。

# 5. 当前证据与目标差异

- `FACT-CHATGPT-001`: 本机当前包为 `OpenAI.Codex` / `OpenAI.Codex_2p2nqsd0c76g0` / x64；AppX 日志记录多个本地 MSIX 通过 `Add-AppxPackage` 成功 Add/Update，其中来源位于用户 Downloads 目录。
- `FACT-CHATGPT-002`: 已签名 OpenAI 客户端使用 `https://persistent.oaistatic.com/codex-app-prod/windows-store-update.json`，并按 `releases/{buildVersion}/ChatGPT-{arch}.msix` 构造完整包地址。
- `FACT-CHATGPT-003`: 2026-08-02 只读请求返回 schema `1`、identity `OpenAI.Codex`、版本 `26.727.6591.0`；x64 与 ARM64 完整 MSIX 地址均返回 HTTP 200 和 AppX 内容类型。版本是动态观察值，不得写死为最新版本。
- `FACT-CHATGPT-004`: 当前仓库把 ChatGPT 解析为 `MicrosoftStore` 计划，运行时依赖 WinGet/App Installer，与 `DEC-CHATGPT-002` 冲突。
- `ASM-CHATGPT-001`: OpenAI 当前客户端采用的清单与 release 路径是面向生产更新的长期合同；未来若合同变化，本工具应失败关闭并通过发布新版本适配，而不是猜测或切换非固定来源。
- `current_execution_path`: UI 扫描 → resolver 返回 Store 计划 → orchestrator 进入 WinGet/AppX Store transport → postcheck。
- `target_delta`: resolver 改为读取 OpenAI 清单并返回普通 `DirectPackage(MSIX)`；复用现有下载、验证、结构化执行和 postcheck 链；ChatGPT 的 Store transport 不再可由嵌入信任配置到达。
- `evidence_gaps`: 尚未在可丢弃 Windows x64/ARM64 环境执行真实首次安装和旧版更新；当前主机不得用真实点击关闭该缺口。

# 6. 范围与责任边界

- `allowed_write_scope`: 本仓库 `src/`、`config/`、`tests/`、`docs/`、`evidence/`、`tasks/`、`dist/` 与必要依赖锁文件。
- `hard_protected_scope`: 父 Obsidian 仓库、`E:\\shipin\\**`、真实用户应用安装、系统策略、账户/凭据、第三方包提交。
- `protected_contracts_and_invariants`: 五产品固定范围、官方源、失败关闭、单按钮、无 Store UI/引导器、无远程命令、安装后复检、无第三方包镜像。
- `authorization_limits`: 授权实现、测试、官方元数据只读取证和未签名测试构建；不授权在当前主机真实部署 ChatGPT。
- `stop_if_scope_expands`: 需要管理员修改系统策略、接受企业许可、维护服务端或镜像、使用私有 Store capability、登录账户、打开 Store UI、执行不透明远程脚本或无法从官方清单构造完整包。

# 7. 实现蓝图

- `blueprint_status`: confirmed at responsibility boundary。
- `caller_entry_consumer`: caller=ChatGPT 行唯一按钮；entry=现有 resolver/安装编排器；decision=生成直接 MSIX 候选并完成版本判断；consumer=Windows 注册的固定 Package Family 与 UI 最终状态。
- `expected_touchpoints_or_search_anchors`: `src/adapters/` 新增或扩展 ChatGPT 清单解析；`src/adapters/resolver.rs` 返回直接包；`config/trust-registry.toml` 固定官方清单、release path 与包身份；`src/platform/windows.rs` AppX 部署参数；`src/app.rs` 直接包状态展示；resolver/security/UI tests；任务与维护文档。
- `wiring_to_final_consumer`: ChatGPT resolver 返回 `ReleaseCandidate` 后必须进入与 Claude 相同的私有下载、MSIX 验证、执行前二次绑定、`Add-AppxPackage` 和 direct postcheck；最终 UI 由重新扫描的真实 AppX 状态刷新。
- `failure_and_recovery`: 元数据或合同失败不生成候选；下载/验证失败不执行；部署失败保留脱敏错误摘要；postcheck 超时报告“结果未知”而不是假成功；所有失败均不切换 Store/WinGet/引导器。
- `implementation_freedom`: 满足目标、合同、边界和验收时，局部结构由执行者选择；不新增通用 provider/plugin 框架，不为单产品复制第二套下载器。
- `selected_profile_obligations`:
  - `stateful-runtime`: 单次解析/下载/部署、取消边界、唯一终态和 postcheck 必须有自动测试。
  - `external-boundary`: 固定 HTTPS 合同、超时、大小限制、URL 白名单、签名/身份/架构验证、脱敏错误和真机证据。
  - `ui-workflow`: ChatGPT 行只保留一个状态匹配动作，执行状态回流并最终刷新。
  - `configuration`: 官方清单、host/path、Package Identity/Family/Publisher、启用架构和包类型由嵌入配置固定并在启动时验证。

# 8. TASK 与 ASSEMBLY 计划

### TASK-CHATGPT-ONECLICK-001

- `links`: `OBJ-CHATGPT-ONECLICK-001`, `REQ-CHATGPT-001..005`, `INV-CHATGPT-001..003`, `RULE-CHATGPT-001..004`, `DEC-CHATGPT-002`
- `owns_behavior`: 从 ChatGPT 唯一 UI 动作到官方清单解析、对应架构 MSIX 下载、验证、部署、postcheck 和最终 UI 状态的完整纵向行为切片。
- `target_delta`: 将 ChatGPT 从 Store 计划切换为现有 direct-package 主链；不改变其他四产品的解析和执行合同。
- `integration_edges`: UI → resolver → direct candidate → downloader → MSIX verifier → AppX deployment → Windows package registration → postcheck → UI refresh。
- `expected_touchpoints`: 第 7 节 search anchors；允许新增一个职责单一的 ChatGPT adapter 和 fixture，不新增远程服务或第二套安装框架。
- `linked_tests`: `TEST-CHATGPT-ONECLICK-001`, `TEST-CHATGPT-ONECLICK-002`
- `stop_conditions`: 官方清单无法稳定绑定固定 identity/path；完整 MSIX 需要未声明依赖；实现需要 Store/WinGet/登录/管理员策略变更；或发现当前 AppX identity/publisher 与固定合同不一致。

- `assembly_not_required_reason`: 只有一个纵向行为切片；UI、解析、下载、部署和复检必须作为同一 TASK 集成验收。

# 9. 验证与验收

- `consumer_chain_validation`: 必须证明 ChatGPT 行唯一按钮 → resolver direct plan → 私有下载 → MSIX 验证 → 本地 AppX 部署 → 固定身份 postcheck → UI 刷新的完整链；仅新增解析函数、仅修改文案或仅验证 URL 均不能通过。
- `real_integration_evidence`: 在专用可丢弃 Windows x64/ARM64 环境记录首次安装与旧版更新，包括 OS/架构、清单、最终 URL、包签名/Identity/Publisher/架构、部署日志、Store/WinGet/引导器/登录窗口未启动监控和最终 AppX 状态。当前主机只允许非执行 proof。
- `failure_recovery_ownership_validation`: 程序负责元数据合同校验、下载取消与清理、执行前二次绑定、部署错误映射、有限 postcheck 和真实失败状态；用户只负责保存运行中客户端内容以及提供可丢弃真机。缺少真机时保持 `Implementation complete; validation pending`，不得把失败恢复变成用户选择下载通道。

### RISK-CHATGPT-001

- `scenario`: 远端清单被错误解释或未来格式变化后构造到非官方/错误架构包。
- `impact`: 供应链、兼容性和错误安装风险。
- `control_or_acceptance_owner`: 固定 schema/identity/URL 合同、允许架构映射、嵌入信任注册表、解析器失败测试和下载后 AppX 精确验证。

### RISK-CHATGPT-002

- `scenario`: AppX 部署返回成功但目标未安装、版本未更新，或运行中应用导致部署/复检卡住。
- `impact`: 假成功、重复更新和不可理解的 UI 状态。
- `control_or_acceptance_owner`: 结构化关闭目标应用参数、固定次数 postcheck、Package Family/Publisher/架构/版本验证和真机安装证据。

### TEST-CHATGPT-ONECLICK-001

- `links`: `TASK-CHATGPT-ONECLICK-001`, `REQ-CHATGPT-001..005`, `SCN-CHATGPT-001..004`, `RISK-CHATGPT-001..002`
- `method`: fixture 和单元/集成测试覆盖有效清单、schema/identity/version/架构变化、URL 白名单、direct plan、已最新/更新判断、结构化 AppX 参数、执行前二次绑定、postcheck 成功/失败/超时，以及嵌入配置中无启用 Store 分发条目。
- `expected_observable_result`: ChatGPT 解析为固定官方 host 的 MSIX direct candidate；所有合同变化失败关闭；一个按钮进入现有 direct 链；其他四产品回归通过。
- `failure_path_covered`: 网络/HTTP、清单过大或无效、schema、版本、identity、URL、签名、Publisher、Package Family、架构、下载、部署、postcheck、取消和重复调用。
- `cannot_prove`: 不证明 OpenAI 在线端点在所有地区持续可达，也不证明真机 AppX 部署成功。

### TEST-CHATGPT-ONECLICK-002

- `links`: `TASK-CHATGPT-ONECLICK-001`, `REQ-CHATGPT-002..005`, `INV-CHATGPT-001..002`, `RISK-CHATGPT-002`
- `environment_sensitive`: true
- `method`: 在专用可丢弃 Windows x64/ARM64 环境分别执行首次安装和旧版更新，记录官方清单、最终 MSIX URL、签名/Identity/Publisher/架构、部署结果、Store/WinGet/登录窗口未启动监控和最终 AppX 状态。
- `expected_observable_result`: 单按钮完成直连完整包安装/更新；不启动 Store、WinGet、引导器或账户登录；最终身份和版本正确。
- `failure_path_covered`: 运行中应用、网络中断、磁盘空间、AppX 部署失败、架构不匹配和 postcheck 超时。
- `cannot_prove`: 不证明未来官方合同永久不变。

### EV-CHATGPT-ONECLICK-001

- `for`: `TEST-CHATGPT-ONECLICK-001`
- `required_evidence_shape`: 最终候选上的格式、clippy、全部测试、resolver fixture、安全边界测试输出，以及 direct URL/计划/部署参数的断言。

### EV-CHATGPT-ONECLICK-002

- `for`: `TEST-CHATGPT-ONECLICK-002`
- `required_evidence_shape`: 可丢弃 Windows 环境信息、官方清单与包 URL、签名/身份/架构、部署日志、Store/WinGet/登录窗口未启动证明、最终 Package Family/Publisher/版本和操作日志。

| ID | 场景 | 关联 | 通过条件 | 证据 | 不能证明 |
|---|---|---|---|---|---|
| GATE-CHATGPT-ONECLICK-001 | 单按钮 direct 链与安全边界 | OBJ-CHATGPT-ONECLICK-001 / TASK-CHATGPT-ONECLICK-001 / TEST-CHATGPT-ONECLICK-001 | 自动测试覆盖完整消费者链、合同失败关闭、Store 路径不可达且四产品回归通过 | EV-CHATGPT-ONECLICK-001 | 在线服务与真机部署 |
| GATE-CHATGPT-ONECLICK-002 | Windows 真机完整包安装/更新 | OBJ-CHATGPT-ONECLICK-001 / TASK-CHATGPT-ONECLICK-001 / TEST-CHATGPT-ONECLICK-002 | x64/ARM64 各完成首次安装和旧版更新；无 Store/WinGet/引导器/登录窗口且最终身份真实 | EV-CHATGPT-ONECLICK-002 | 未来合同永久稳定 |

# 10. 产物与完成回写

- `required_deliverables`:
  - `src/` ChatGPT direct resolver 与真实执行链连接
  - `config/trust-registry.toml`
  - `tests/fixtures/chatgpt/`、resolver/security/UI 测试
  - `docs/implementation-status.md`、`docs/maintenance.md`、`evidence/`
  - `dist/easy-agent-windows-x64.exe` 与校验/manifest（仅未签名测试构建）
- `documentation_impact`: updated；说明 OpenAI 官方清单、完整 MSIX 直装、版本变化维护、失败关闭边界和真机验证状态，并删除运行时依赖 Store/WinGet 的表述。
- `repository_hygiene_requirement`: 不提交 OpenAI 包、临时下载、用户日志、短期版本化包 URL、凭据或父仓库修改。
- `external_review`: policy=optional；当官方合同绑定、MSIX 信任边界、AppX 执行参数或 Store 路径可达性影响安全 Go/No-Go 时触发独立安全复核。
- `non_completion_rules`: 未连接真实 UI/direct consumer、合同变化测试不足、Store/WinGet 仍可由启用配置触发、文档/制品未同步、保护范围异常或真机 Gate 缺失时不得宣称最终发布完成；缺真机环境时只能 `Implementation complete; validation pending`。

```json
{
  "schema_version": 1,
  "validators": [
    {
      "validator_id": "rust-format",
      "validation_kind": "closure",
      "command": ["cargo", "fmt", "--check"],
      "cwd": ".",
      "timeout_seconds": 120
    },
    {
      "validator_id": "rust-clippy",
      "validation_kind": "behavior",
      "command": ["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"],
      "cwd": ".",
      "timeout_seconds": 600
    },
    {
      "validator_id": "rust-tests",
      "validation_kind": "behavior",
      "command": ["cargo", "test", "--all-targets"],
      "cwd": ".",
      "timeout_seconds": 900
    },
    {
      "validator_id": "fixture-contracts",
      "validation_kind": "behavior",
      "command": ["cargo", "test", "--test", "resolver_fixtures"],
      "cwd": ".",
      "timeout_seconds": 300
    },
    {
      "validator_id": "security-boundaries",
      "validation_kind": "behavior",
      "command": ["cargo", "test", "--test", "security_boundaries"],
      "cwd": ".",
      "timeout_seconds": 300
    }
  ]
}
```

执行 run ID、candidate/history、manifest/receipt hash、实际命令结果、final revision 和终态写入执行 sidecar 或 Completion Report，不写回本任务卡。
