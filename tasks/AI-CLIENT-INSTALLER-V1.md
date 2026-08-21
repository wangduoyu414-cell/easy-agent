---
task_contract_version: 3
card_id: "AI-CLIENT-INSTALLER-V1"
title: "交付从官方优先、固定可信来源安装五款 AI 桌面客户端的轻量跨平台安装助手"
status: "ready"
work_kind: "mixed"
execution_target: "agent-executable"
complexity: "complex"
product_risk: "L4"
orchestration_risk: "O2"
execution_profiles: [investigation, stateful-runtime, external-boundary, ui-workflow, device-integration, configuration]
external_review_policy: "optional"
repo_root: "E:\\Obsidian\\workspaces\\ai-client-installer"
blocked_by: []
---

# 1. 任务身份与就绪状态

- `objective_id`: `OBJ-AIINST-001`
- `readiness`: P0～P3（proof、实现、适配、总装）Ready；P4 正式发布只有在合法 Windows 代码签名证书与 Apple Developer ID/notarization 凭据到位后才可执行。缺凭据时可以完成实现和未签名测试构建，但整卡只能记录 `Implementation complete; validation pending`，不能宣称最终交付完成。
- `authority_sources`: 用户在本任务中的需求与限制；[官方分发与安装调研](../research/official-distribution-and-installation-research-2026-07-31.md)；五款产品及操作系统厂商的官方分发文档；多代理从最小架构、安全/供应链、维护成本角度形成的共同结论。
- `decision_owner`: 用户决定产品范围、品牌名称、是否接受某产品在特定平台“暂不支持”；执行者只能在本卡确认的五产品、官方优先、Claude Windows/macOS 四平台固定回退窄例外和交付边界内选择实现细节。
- `material_unknowns`: `UNK-AIINST-001` ChatGPT Windows 微软轻量安装器主路径和完整 MSIX/离线许可证兜底在 x64/ARM64 干净系统上的首次安装、旧版更新、取消/UAC/授权/策略失败与最终复检；`UNK-AIINST-002` WorkBuddy macOS 两条官方 API SHA-256 与对应 CDN 完整 ZIP 不一致，当前仅该固定身份采用 Apple 平台签名策略，仍需干净机交互验收；`UNK-AIINST-003` Hermes Intel Mac 已由厂商策略关闭为 unsupported，Apple Silicon DMG bootstrap identity 已固定，最终桌面/runtime 行为仍需证明；`UNK-AIINST-004` Claude 四平台固定回退、macOS 双架构真实包和临时安装/更新/回滚已闭合，仍缺 Windows x64/ARM64 与 Apple Silicon 原生干净机交互矩阵。未关闭的产品/平台保持 unsupported 或 validation pending。
- `external_prerequisites`: P4 正式发布需要由发布主体提供可用的 Windows Authenticode 证书、Apple Developer ID Application 证书、notarization 凭据及安全的 CI secret 注入方式；Claude 四平台回退服务扩大传播前还必须完成 Anthropic 再分发/服务条款与腾讯云 AUP 判断，并确定长期 HTTPS 域名。以上任一缺失都使相应正式发布保持 No-Go，但不阻止私有技术验证。

# 2. 业务目标

- `business_object`: 一次本地、可审计、可恢复的五产品安装助手运行。
- `actor`: 需要在个人 Windows 或 macOS 电脑上安装 AI 桌面客户端、且不想手工辨别架构、版本和真假下载来源的用户。
- `workflow_and_trigger`: 用户运行便携 Windows EXE 或打开 macOS 应用，工具检测系统与现有安装，用户选择一个或多个客户端并确认，工具按顺序完成受信任版本解析、下载、验证、安装和复检。
- `problem_or_opportunity`: 官方分发形态不一致，用户容易拿到 Store 引导器、错误架构、旧版本、假冒客户端或“安装器退出但应用没装好”的假成功；上游版本与下载地址又会持续变化。
- `single_outcome`: 用户在一个轻量本地界面中，能够把 WorkBuddy、Hermes、CC Switch、Claude Desktop（含 Code 页）和 ChatGPT 的厂商当前推荐桌面版本安全安装到受支持的 Windows/macOS 设备，并获得真实、可诊断的最终状态。
- `observable_results`: `RESULT-AIINST-001` 工具显示真实 OS/架构、已安装版本与可执行动作；`RESULT-AIINST-002` 每个由本工具直接获取并持有的安装制品来自允许的官方链路，或来自 Claude 四平台的固定受验证回退，并通过对应完整性/平台签名/身份校验；Hermes bootstrap 后续由厂商进程获取的依赖单独披露，不宣称被本工具逐件审计；`RESULT-AIINST-003` 安装后复检到目标应用，并在 macOS 通过 Gatekeeper 可执行性评估；`RESULT-AIINST-004` 某项失败不污染其他项，最终摘要可导出；`RESULT-AIINST-005` 在签名凭据到位后交付签名的 Windows x64/ARM64 便携 EXE 与 notarized macOS Universal DMG。
- `non_goals`: Linux；移动端；Claude Code CLI；ChatGPT Classic；除 Claude 四平台固定受验证回退外的第三方镜像、公共缓存或再分发；任意 URL；远程脚本执行；产品插件/DSL；远程规则平台；账号/云同步；遥测；客户端触发的后台同步；管理厂商后续自动更新；卸载；修复目标产品安装；历史版本；beta/nightly；自动回滚；企业控制台；全盘扫描；自动安装 Git/Virtual Machine Platform；打开 Microsoft Store 页面或执行 `get.microsoft.com` 网页引导器。

# 3. 需求质疑与确认

- `user_statement`: 需要一个很小的脚本程序，可选择安装最新 WorkBuddy、Hermes、CC Switch、Claude Code、GPT 客户端，内置检测、下载、安装，根据系统和硬件选择正确安装包，最终交付便携 EXE 和 macOS 安装包；必须考虑版本变化和长期可用，且不要过度设计。
- `symptom_vs_goal`: “把五个下载按钮放在一起”只是表面功能；真实目标是长期可维护地解析官方当前版本、避免引导器/错包/假包，并对安装结果负责。
- `REQ-AIINST-001` (`required_behavior`): 启动后检测 OS 版本、CPU 架构、可用磁盘、目标软件安装身份与版本；不根据 GPU 等与包选择无关的信息制造伪精确判断。
- `REQ-AIINST-002` (`required_behavior`): 只管理五个固定产品和厂商默认推荐生产通道；Claude Code 在 V1 中指 Claude Desktop 的 Code 页，不指 CLI；GPT 指 OpenAI 新统一 ChatGPT 桌面应用。
- `REQ-AIINST-003` (`required_behavior`): 默认界面只显示产品、当前版本、目标版本、安装/更新状态及必要权限或功能限制；支持单项或多选顺序安装。架构、包类型、文件大小、实际入口/下载节点、镜像时效和签名发布者由后台固定校验并写入脱敏日志，不让用户选择或判断安装通道。
- `REQ-AIINST-004` (`required_behavior`): 下载、验证、安装和复检形成闭环；进程退出码不能单独作为成功依据。
- `REQ-AIINST-005` (`required_behavior`): Windows 交付 x64/ARM64 便携单 EXE，工具自身无需安装；macOS 交付一个 Intel/Apple Silicon Universal DMG。
- `REQ-AIINST-006` (`required_behavior`): 版本、短期地址和当期哈希运行时解析；稳定产品身份、官方入口、官方/镜像独立允许域、更新公钥、镜像清单公钥和签名发布者随安装助手版本管理。
- `REQ-AIINST-007` (`required_behavior`): ChatGPT Windows 必须使用固定 Microsoft Store Product ID `9PLM9XGG6VKS`，从已注册且 Identity/Family/Publisher/安装位置均匹配的 Microsoft Desktop App Installer 定位 WinGet，在后台完成安装或更新且不打开商店页面。WinGet 缺失、过旧或证书固定异常时最多自愈一次，并验证 Microsoft Release 摘要、签名、Bundle 身份、目标架构和依赖闭包；最终必须复检 OpenAI Package Identity/Family/Publisher/架构/版本。
- `REQ-AIINST-008` (`required_behavior`): 用户可导出脱敏日志；日志不得包含账号凭据、完整临时 CDN 查询串或未脱敏的用户隐私路径。
- `REQ-AIINST-009` (`required_behavior`): 每个启用的适配器必须在版本化 `trust-registry` 中具备完整的入口、允许重定向 host、包类型、产品身份、签名/公钥和验证方式；proof 未固定的值使该产品/平台保持 disabled，远端响应不能放宽注册表。
- `REQ-AIINST-010` (`required_behavior`): macOS 不能以“app 已复制”为成功；安装后的 app 必须通过代码签名验证与 Gatekeeper assessment，且下载/解包/复制过程不得移除 quarantine 扩展属性。
- `REQ-AIINST-011` (`required_behavior`): Hermes UI 与结果必须明确“本工具验证官方 bootstrap；bootstrap 后续依赖由 Hermes 厂商链负责”，desktop installed 与 runtime ready 分开报告。
- `REQ-AIINST-012` (`required_behavior`): 五产品 UI 均只暴露一个与当前状态一致的主动作；直接包使用安装/更新，ChatGPT Store 在尚未确认商店版本时使用“检查更新”。ChatGPT 的 WinGet 健康检查/一次性自愈、固定 Store ID 后台安装或更新和精确复检均在该动作后台自动编排，不把安装通道选择交给用户，也不在查询前假称已有新版。
- `REQ-AIINST-013` (`required_behavior`): Claude Windows x64/ARM64 与 macOS Intel/Apple Silicon 仅在官方入口或已确认完整包因地区/网络可用性失败时，才允许回退到代码内固定的受验证服务；清单必须独立验签，下载 URL 必须落在平台独立白名单。Windows 仍验证 Identity/Publisher/架构/版本/AppX 签名；macOS 仍验证 Bundle/Team/目标 slice/版本/codesign/Gatekeeper。客户端不能触发服务端同步，也不能提交任意 URL。
- `INV-AIINST-001`: 不能执行服务器返回的 PowerShell/bash/任意命令；安装行为只能来自本地编译的固定产品策略和参数数组。
- `INV-AIINST-002`: 无法确认允许来源、架构、签名/摘要、安装身份或现有安装状态时必须失败关闭，不自动覆盖或回退到未验证第三方源。
- `INV-AIINST-003`: 不在仓库或发布包中保存五款第三方安装包；固定回退仅限 Claude 当前 Windows x64/ARM64 MSIX 与 macOS Universal DMG，不得扩展为通用抓取、历史包目录或其他产品镜像。
- `INV-AIINST-004`: 不整体以管理员/root 运行；只有具体安装动作在用户确认后按需触发系统提权。
- `INV-AIINST-005`: 用户确认前不关闭目标应用；已确认的 MSIX 更新可由系统部署参数关闭占用中的目标应用，UI 必须提前提示保存内容；不自动接受厂商交互安装器许可，不禁用目标软件自更新。
- `material_ambiguities`: 无需再由用户决定的产品语义；各厂商包身份、签名主体和 direct-install 可行性由对应 proof 按官方证据决定。若某平台无官方匹配包，结果是“厂商暂不支持”，不是猜测性兼容。
- `decisions_and_authority`: 固定五产品、一产品一适配器、无远程控制面、仅 Claude Windows/macOS 四平台固定受验证回退、失败关闭、每产品独立任务、按 MSI/AppX 引擎隔离冲突且其他任务并行、原生轻量 UI 已确认。任何新增产品、其他镜像、远程规则或自动执行脚本均是范围扩张，必须由用户重新授权。
- `CHG-AIINST-003`: 用户于 2026-08-09 明确授权为大陆网络不可达场景增加 Claude Windows x64 香港下载源；要求同步按固定周期自动完成、客户端仅在安装/更新时使用、代理不得暴露或常驻、设计保持最小且防止滥用。
- `CHG-AIINST-004`: 用户于 2026-08-11 明确要求 Claude 及其他产品的普通界面不显示官方/CDN/镜像等来源标签，只显示版本与状态；来源与信任链继续在后台强制校验并保留于脱敏审计日志。
- `CHG-AIINST-005`: 用户于 2026-08-11 明确授权把 Claude 香港回退扩展到 Windows x64、Windows ARM64、macOS Intel 与 Apple Silicon，并要求优先构建 Windows x64/ARM64 测试 EXE。
- `CHG-AIINST-001`: 用户于 2026-08-01 明确要求 ChatGPT 与其他产品保持同一认知模型：一个按钮完成下载与安装，所有内部步骤在后台完成。
- `DEC-AIINST-001` (`partially_reactivated_design`): WinGet `msstore` + App Installer 自愈重新成为 ChatGPT Windows 主路径；旧版的无授权本地闭包兜底仍保持禁用。
- `CHG-AIINST-002`: 用户于 2026-08-02 明确要求 ChatGPT 下载路径不依赖 Windows 商店、不要引导包，并复用之前成功的完整本地包部署路径；旧 Store/WinGet TASK/TEST/EV/GATE 语义全部失效。
- `DEC-AIINST-002` (`superseded_design`): 2026-08-02 的 OpenAI 清单 → 直接 MSIX 设计已被 2026-08-11 运行证据否定，不再作为当前执行或验收依据。
- `CHG-AIINST-006`: 用户继续要求 ChatGPT 在全新电脑上完成安装闭环。2026-08-11 实测当前 OpenAI 清单对应 x64/ARM64 MSIX 均为 HTTP 404，旧包声明 `appLicensing`，证明既有电脑的本地更新成功不能外推为全新电脑首次安装成功。
- `CHG-AIINST-007`: 用户于 2026-08-12 要求修正 ChatGPT 本机版本与商店更新状态容易误解的描述、移除一开始显示近半的伪进度，并要求 Windows/macOS 可直接获取的已验证安装包默认保存在系统 Downloads 目录以便查找。
- `DEC-AIINST-006` (`confirmed_design`): ChatGPT Windows 采用单动作“固定 Store ID → 受信任 WinGet 健康检查/一次性自愈 → 后台 Store 安装或更新 → 固定 OpenAI 身份/架构/版本复检”。不打开 Store UI，不执行网页引导器；Store 传输、策略或授权失败时硬失败，不改用 `winget download --skip-license` 或单个无授权 MSIX 假装成功。

# 4. 业务场景与规则

- `SCN-AIINST-S1` 主路径: 首次运行，在受支持设备上检测到目标未安装，解析正确架构的官方推荐包，下载、验证、安装并复检成功。
- `SCN-AIINST-S2` 已安装路径: 检测到同版本时不重复安装；检测到更旧版本时可更新；检测到更高/非管理通道版本时不降级；版本不可比较时不覆盖。
- `SCN-AIINST-S3` 并行路径: 用户可连续启动多个产品任务；下载、验证、厂商 EXE 和 macOS 应用复制互不阻塞，MSI 只与 MSI、MSIX/Store 只与 MSIX/Store 在各自通道 FIFO。一个产品失败后保留其错误且不影响其他任务，除非发生全局磁盘/网络/安全故障。
- `SCN-AIINST-S4` 下载失败路径: 网络断开、代理失败、超时、Range 不支持或磁盘不足时给出可操作错误；安全地重试或从头下载，不执行残缺文件。
- `SCN-AIINST-S5` 信任失败路径: 重定向越界、内容类型/魔数错误、摘要不符、签名无效、发布者/Bundle/package identity 不符时删除或隔离临时文件并拒绝安装。
- `SCN-AIINST-S6` 交互安装路径: WorkBuddy/Hermes 等需厂商窗口时，工具显示等待状态；用户取消或安装器失败后复检并给出真实结果。
- `SCN-AIINST-S7` 平台不支持路径: 系统版本过低、无原生架构包或厂商限制时显示原因与官方链接，不提供错误架构包。
- `SCN-AIINST-S8` ChatGPT Windows 特殊路径: 用户点击唯一“安装/更新”动作；后台核对受信任 WinGet，必要时自愈一次，然后以固定 Store ID 安装或更新并复检；全程不打开商店页面。Store 传输、授权、策略、App Installer 信任、OpenAI 身份、架构或复检失败时给出单一可诊断结果。
- `RULE-AIINST-001`: “最新版”统一解释为厂商当前默认推荐生产版本，不强行套用 stable/latest 双通道术语。
- `RULE-AIINST-002`: 每次安装仅使用当前会话刚解析的候选；短期 CDN 地址不得持久化为下次运行的真相。
- `RULE-AIINST-003`: 已安装状态优先由 package/bundle/registry identity 决定，快捷方式和文件名只作辅助证据。
- `RULE-AIINST-004`: 只有安装后检测到预期身份和可接受版本才进入 succeeded；退出码 0 但复检失败进入 failed/postcheck_failed。
- `RULE-AIINST-005`: `.part` 只在当前下载生命周期内复用；无 Range 支持就从头下载，启动时清理过期临时文件，不建立长期包缓存。
- `RULE-AIINST-006`: Windows ARM64 只能选择官方 ARM64 包；厂商明确支持 x64 模拟且真机验证通过时，才可作为带清晰标签的例外。WorkBuddy V1 不采用该例外。
- `RULE-AIINST-007`: 供应商安装器启动后不强制终止；取消只承诺覆盖解析、下载和校验阶段。
- `RULE-AIINST-008`: 临时下载目录每次随机唯一并限制为当前用户；创建与打开文件时拒绝 symlink/junction/reparse point 越界，规范化后必须仍位于本次临时根内；验证前文件只保留 `.part`，验证失败立即删除或隔离且永不改为可执行最终名。
- `RULE-AIINST-008A`: 完成摘要、平台签名、身份、版本和架构验证后，将非 Store 目标包额外复制到操作系统 Downloads 已知目录供用户查找；复制时重新计算长度/SHA-256，源文件前后重复绑定，同名冲突使用摘要后缀且不得覆盖。安装执行仍只允许私有暂存副本，公开 Downloads 副本不得成为执行信任边界。
- `RULE-AIINST-009`: macOS postcheck 至少包含 bundle/version/Team identity、嵌套代码签名完整性和 Gatekeeper 可执行性；任一失败只能报告 `installed_not_launchable` 或 failed，不能报告 succeeded。
- `RULE-AIINST-010`: ChatGPT Windows 的 Store ID、Package Identity/Family/Publisher、目标架构、App Installer Identity/Family/Publisher、最低 WinGet 版本和官方修复资产名由本地 trust-registry 固定。自愈最多一次；退出码 0 后仍须精确复检。`appLicensing` 包不得在 Store 失败后降级为无授权本地包兜底。
- `RULE-AIINST-011`: signer/Team/public-key 不能远程放宽。变更必须随新版代码提交官方来源、日期、样包 identity/signature 和独立复核证据；过渡期多主体仅在当前官方产物仍实际使用时并存，所有受支持当前产物迁移且矩阵通过后移除旧主体。
- `state_and_lifecycle`: `scanning → ready → resolving → downloading → verifying → awaiting_user_install/installing → postchecking → succeeded|installed_not_launchable|failed|cancelled|unsupported|unknown_installed`；每个产品持有独立状态，批次只聚合，不复制业务真相。
- `data_and_compatibility`: 本地仅持久化安装助手设置、最近一次脱敏日志和解析 fixture 版本；不保存账号、目标应用配置或第三方包。支持 Windows 10 build 17763+ x64/ARM64 和 macOS 12+ Intel/Apple Silicon；每个产品再应用自身最低版本。
- `permission_and_security`: 普通用户启动；按需 UAC/系统授权；安装前二次确认；进程参数使用结构化数组；临时目录随机唯一、仅当前用户可写且拒绝重解析点越界；不提升整个 GUI；macOS 不清除 quarantine。
- `operations_and_observability`: 每步记录时间、产品、版本、目标架构、来源标识、重定向域、字节数、验证结果、安装方式、退出码和复检结果；URL 查询串与用户路径脱敏；支持复制/导出。
- `risk_sensitive_invariants`: 完整 trust-registry 与制品签名身份共同成立才可执行；不执行服务器返回的命令；不安装未知/错误架构；不覆盖未知安装；不降级更高版本；不把 bootstrap 下游依赖描述为已完整审计；macOS 未过 Gatekeeper 不得成功；ChatGPT Windows 必须固定 Store ID、受信任 WinGet/App Installer 身份和 OpenAI Package Identity/Family/Publisher/架构，且不得在授权失败后用无授权本地包伪造成功；不把测试下载或第三方包提交到仓库；正式制品必须签名/notarized。
- `inapplicable_faces_with_reason`: 无服务端数据库、无多用户并发、无云账号、无业务迁移、无支付、无遥测；运行时并发仅限 UI 与单个下载任务协调，多产品安装刻意串行。

# 5. 当前证据与目标差异

- `FACT-AIINST-001`: 项目已迁移为独立仓库 `E:\Obsidian\workspaces\ai-client-installer`；其父目录是受治理的 Obsidian 知识库，父仓库通过本地 ignore 规则隔离此子仓库。本任务不得修改父知识库或原 `E:\shipin` 口播剪辑仓库中的无关内容。
- `FACT-AIINST-002`: 仓库已有 Rust/egui 应用、Windows x64 真实执行链、macOS DMG/ZIP/tar.gz 检测验证安装核心、版本化 trust-registry、解析/安全测试、Windows 构建脚本和 macOS Universal 签名/公证脚本。
- `FACT-AIINST-003`: WorkBuddy 有结构化更新接口；macOS Intel/Apple Silicon 分别返回 ZIP 与 SHA-256，因此选择 ZIP 作为自动化合同；没有 Windows ARM64 包。最终 Mac Bundle/Team identity 尚未固定。
- `FACT-AIINST-004`: Hermes 官网分发的是签名 bootstrap；桌面与 runtime 是两个可观察状态，bootstrap 还会获取下游依赖；厂商明确不支持 Intel Mac。
- `FACT-AIINST-005`: CC Switch 提供带 minisign 签名的结构化 latest.json、x64/ARM64 MSI、同一 Universal macOS tar.gz 和固定更新公钥，是五款中最稳定的解析合同。
- `FACT-AIINST-006`: Anthropic 官方 Windows 部署文档明确提供 x64/ARM64 完整 MSIX，并给出 `Add-AppxPackage` 与机器级 `Add-AppxProvisionedPackage -SkipLicense -Regions all` 安装方式。2026-08-15 实测个人 Setup 仍会再次下载约 255 MB，因此项目改为直接验证并部署 MSIX；macOS 继续使用 Universal DMG。
- `FACT-AIINST-007`: 本机观察新统一 ChatGPT 包身份为 `OpenAI.Codex_2p2nqsd0c76g0`。2026-08-02 清单对应直接 MSIX 曾可达，但 2026-08-11 当前清单 `26.803.10989.0` 对应 x64/ARM64 地址在本机与东京节点均返回 HTTP 404；仍可用的旧 x64 MSIX 的 `AppxManifest.xml` 声明 `appLicensing`。因此直接包仅证明部分已授权电脑可更新，不能证明全新电脑首次安装闭环。
- `FACT-AIINST-008`: 调研期间仅对 WorkBuddy/Hermes Windows 安装包做下载与静态签名检查，没有执行安装；所有真实安装结果仍待任务执行验证。
- `ASM-AIINST-001`: Rust + eframe/egui 能以足够小的责任面提供简单原生 UI 和无 WebView 便携 Windows EXE；由 `TASK-AIINST-FOUNDATION` 用实际产物证明。
- `ASM-AIINST-002`: Cargo release build 加本地平台脚本可形成签名 Windows EXE 与 macOS Universal DMG；正式签名依赖用户/组织提供合法证书与密钥。
- `current_execution_path`: UI → trust-registry → 产品 adapter；ChatGPT Windows 进入绑定固定 Store ID 的微软轻量安装器并在明确分发失败时进入完整 MSIX/离线许可证兜底；Claude Windows 进入官方完整 MSIX直连或签名香港回退；其他产品进入私有下载/摘要或 updater 签名 → Windows AppX/MSI/EXE 或 macOS DMG/归档 verifier → 安装执行 → 身份/版本/架构 postcheck。
- `current_behavior`: Windows x64 五产品执行基础已接通，Windows x64/ARM64 自家 EXE 已从 macOS 交叉构建；macOS Universal 核心已实现并通过两个 Apple target 编译/Clippy。WorkBuddy、CC Switch、Claude、ChatGPT 的两类 Mac 条目已启用；Claude 四平台下载/验证回退闭合，真机安装仍为 validation pending；Hermes Intel 为 unsupported，Apple Silicon bootstrap 最终状态待证明。
- `target_delta`: 在独立子项目中建立一个薄 UI、单一编排器、五个明确适配器、共享下载/验证与两个平台安装执行模块，完成真机可验证发布链。
- `evidence_gaps`: 见 `UNK-AIINST-*`；还缺 Windows x64/ARM64、macOS Intel/Apple Silicon 的干净环境安装证据、WorkBuddy/Claude 合同闭环与自家制品正式签名证据。

# 6. 范围与责任边界

- `allowed_write_scope`: `E:\Obsidian\workspaces\ai-client-installer\**`；可新增源码、测试、fixture、构建/签名脚本、文档、脱敏 evidence 和本项目 CI。若必须修改仓库根配置，先证明无法在子目录隔离并停止请求授权。
- `allowed_adjacent_scope`: 为本项目增加最小 Git ignore、许可证说明、第三方通知、测试证书占位说明和本地开发脚本；不得修改父 Obsidian 知识库或原口播剪辑项目。
- `hard_protected_scope`: `E:\Obsidian\AGENTS.md`、父知识库的 tracked/dirty 文件、`E:\shipin\**` 及其他用户未跟踪文件；不得提交第三方安装包、签名私钥、账号、真实用户日志或 CDN 临时令牌。
- `protected_contracts_and_invariants`: 五产品固定；官方源优先；仅 Claude Windows/macOS 四平台允许固定受验证回退；失败关闭；无远程命令；无全盘扫描；普通用户启动；安装后复检；交付 Windows 便携 EXE 和 macOS Universal DMG。
- `authorization_limits`: 本任务卡授权实现、测试、构建本地制品，以及在用户明确提供的东京/香港服务器上部署 Claude 四平台私有同步验证链；不授权在用户当前主机实际安装/升级五款软件，不授权购买证书、接受厂商企业条款、推送/提交或写入范围外第三方系统。
- `stop_if_scope_expands`: 如果除 Claude 四平台窄例外外的产品必须依赖第三方镜像，或需要远程下发可执行规则、绕过平台安全、自动接受许可、关闭杀毒/AppLocker/Gatekeeper、存储其他第三方包、修改范围外项目或进行未授权外部写入，立即停止并回报。

# 7. 实现蓝图

- `blueprint_status`: confirmed at responsibility boundary；Rust 原生 UI 为推荐方案，若 foundation 不能满足无额外 GUI runtime、可签名便携 EXE 和 Universal DMG，可在不改变边界的前提下提交替代方案证据。
- `caller_entry_consumer`: caller=用户点击扫描/安装；entry=桌面 UI 与安装编排器；consumer=操作系统注册的真实目标应用及用户可读的最终摘要。
- `boundary_deltas`: 新增 App Shell、Install Orchestrator、Runtime Context、Download/Verify、Windows/macOS Installer、五 Product Adapters、日志与 packaging；唯一 Claude 服务端增量是三个固定上游制品到四个平台清单的定时同步、校验与签名，不提供远程控制面、任意 URL 抓取或客户端触发接口。
- `shared_state_data_contracts`: `RuntimeContext`、`ProductState`、`DetectionResult`、`ReleaseCandidate`、`VerificationPolicy/Report`、`InstallPlan/Outcome`、`PostcheckResult`、`BatchSummary`。版本保留 raw 字符串，比较器归适配器；未知是合法状态，不用默认值掩盖。
- `expected_touchpoints_or_search_anchors`: `Cargo.toml`；`src/app/`；`src/core/`；`src/platform/windows/`；`src/platform/macos/`；`src/adapters/{workbuddy,hermes,cc_switch,claude_desktop,chatgpt}.rs`；`tests/fixtures/<product>/`；`packaging/`；`evidence/`；具体文件可在保持这些责任边界时调整。
- `wiring_to_final_consumer`: 只有真实 UI 手势能沿 adapter resolver → downloader → verifier → platform installer → product postcheck 到达操作系统已安装应用，并在批次摘要中显示一致结果，才算接到最终消费者。
- `failure_and_recovery`: resolver 失败不创建安装计划；下载失败保留可重试状态并清理不可信文件；验证失败永不执行；安装器取消/异常后复检；UI 崩溃重启时清理过期临时文件并重新扫描，不从不完整状态继续执行；不做自动回滚。
- `implementation_freedom`: 可选择具体 Rust crate、日志格式、UI 排版和内部文件切分；不得引入 Electron、常驻服务、远程规则、通用插件或额外数据库，除非 foundation 证据表明当前方案无法满足并获得用户授权。
- `selected_profile_obligations`:
  - `investigation`: 先关闭 ChatGPT Store 授权/无 UI、WorkBuddy macOS 包、Hermes 跨架构和 macOS identity 四类 proof；结论写入 evidence，失败时可使单产品/单平台 No-Go，不能被实现绕过。
  - `stateful-runtime`: 每产品状态转移单向、可取消边界明确；批次聚合不覆盖子状态；重启只恢复设置/日志，不恢复正在安装的外部进程假状态。
  - `external-boundary`: 官方 HTTP/redirect/manifest/package 是不可信输入；必须有超时、大小限制、解析边界、签名/身份验证、脱敏日志和真实源 smoke test。
  - `ui-workflow`: 覆盖扫描中、未安装、已是最新版、可更新、未知安装、下载中、等待厂商安装、成功、失败、取消、不支持；最终动作与 core 状态绑定，不做纯视觉假状态。
  - `device-integration`: 注册表/AppX、codesign/Gatekeeper、DMG mount/copy、MSI/EXE/MSIX 进程、UAC 和应用占用均需真机证据；权限请求只发生在必要步骤。
  - `configuration`: 产品白名单、稳定入口、公钥、签名身份和最低系统规则属于版本化本地配置；改动需测试与审阅，不能从未签名远端覆盖。

## 7.1 最小模块责任

| 模块 | 只负责 | 不负责 |
|---|---|---|
| App Shell | 产品列表、确认、进度、错误、日志导出 | 解析厂商页面、执行 shell 字符串 |
| Install Orchestrator | 每产品状态机、并行下载/校验、按 MSI/AppX 系统引擎隔离的 FIFO 通道、厂商 EXE/macOS 直并行、取消边界、调用链 | 产品特例、平台签名细节 |
| Runtime Context | OS/build/arch/disk/权限、当前用户临时目录 | 根据无关硬件参数猜包 |
| Product Adapter ×5 | 检测规则、官方最新版解析；Claude 四平台受限回退；候选/安装计划、复检规则 | 通用下载、UI、跨产品策略 |
| Downloader/Verifier | redirect、`.part`、大小/魔数、摘要/签名报告 | 决定产品是否允许未知签名 |
| Windows Installer | EXE/MSI/MSIX 的结构化启动、UAC、退出码 | 拼接远程 PowerShell 命令 |
| macOS Installer | DMG/ZIP/app 验证、复制、授权、卸载挂载点 | 绕过 Gatekeeper/quarantine |
| Packaging | 自家 EXE/DMG 构建、签名、公证、产物哈希 | 把第三方安装包嵌入自家 EXE/DMG；镜像同步由独立窄范围服务负责 |

## 7.2 规范信任注册表

`config/trust-registry.toml` 是唯一可执行信任规范。表中标为 `P0-pin` 的值在 proof 取得实包证据后写入源码并复核；在此之前对应 adapter/platform 必须是 disabled。运行时响应只能提供“易变字段”，不能新增 host、包类型、签名主体、公钥或应用身份。

| 产品/平台 | 初始入口与允许 host | 包类型 | 固定产品身份 | 必须验证 | 初始启用状态 |
|---|---|---|---|---|---|
| WorkBuddy Windows x64 | `www.workbuddy.cn/v2/update`；`www.workbuddy.cn`、`download.codebuddy.cn` | EXE；当前官方 x86 NSIS bootstrap 面向 x64 产品 | Product=`WorkBuddy`；Signer=`Tencent Technology (Shenzhen) Company Limited`；postinstall executable=`WorkBuddy.exe` | HTTPS 链、最终 host、bootstrap PE machine=x86、Authenticode 有效且 signer 精确匹配；接口 hash 非空时再校验 hash；安装后 `WorkBuddy.exe` 必须为 x64 且版本达到目标 | enabled in validation build；Windows x64 clean update proof pending |
| WorkBuddy macOS Intel/ARM64 | 同上两 host | 官方分架构 ZIP | 应用名=`WorkBuddy.app`；Bundle ID=`com.workbuddy.workbuddy`；Team ID=`FN2V63AD2J` | 官方 SHA-256、app bundle/version/arch、codesign、Gatekeeper、quarantine 保留 | disabled：两架构 API SHA 与 CDN 完整 ZIP 不一致；clean install proof pending |
| Hermes Windows x64/ARM64 | `hermes-agent.nousresearch.com`；`hermes-assets.nousresearch.com` | EXE bootstrap | Product=`Hermes`；Signer=`Nous Research Inc.` | HTTPS 链、host、PE、Authenticode/signer；bootstrap 与 runtime 状态分离 | disabled until both-arch P0 proof |
| Hermes macOS Intel | 同上两 host | 无 | 厂商明确不支持 | 不下载、不尝试 Rosetta 兼容 | unsupported |
| Hermes macOS Apple Silicon | 同上两 host | DMG bootstrap | 应用名=`Hermes.app`；Bundle ID=`com.nousresearch.hermes.setup`；Team ID=`T2F6S8MF7C` | DMG/app identity、codesign、Gatekeeper、quarantine；bootstrap 下游只披露不逐件背书 | disabled until bootstrap/install proof |
| CC Switch Windows x64/ARM64 | `dl.ccswitch.io/latest.json`；官方 fallback `github.com/farion1231/cc-switch/releases/latest/download/latest.json`；允许 `dl.ccswitch.io`、`github.com`、`release-assets.githubusercontent.com`、`objects.githubusercontent.com` | MSI | Product=`CC Switch`；Updater pubkey=`dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEM4MDI4QzlBNTczOTI4RTMKUldUaktEbFhtb3dDeUM5US9kT0FmdGR5Ti9vQzcwa2dTMlpibDVDUmQ2M0VGTzVOWnd0SGpFVlEK` | latest.json platform mapping、minisign/Tauri signature、MSI arch/identity；若有 Authenticode 同时记录 | enabled after clean proof |
| CC Switch macOS Intel/ARM64 | 同上入口/host | 清单签名 tar.gz | 应用名=`CC Switch.app`；Bundle ID=`com.ccswitch.desktop`；Team ID=`R8UR22V2F9`；同一 updater pubkey | minisign、archive bounds、bundle/version/arch、codesign、Gatekeeper、quarantine | disabled until Intel/Apple Silicon clean install proof |
| Claude Windows x64/ARM64 | `claude.ai/api/desktop/win32/{arch}/msix/latest/redirect`；各架构独立签名 manifest/signature/artifact 回退路径 | 完整 MSIX | Package Name/Family=`Claude`/`Claude_pzs8sxrjxfjjc`；Publisher 固定 | 官方/回退版本与架构、回退 minisign/时效/SHA-256、MSIX AppX 签名/Publisher/Identity；本地固定管理员命令机器级部署，安装后精确 package identity/版本 | 两架构启用；无 Setup 二次下载，clean install proof pending |
| Claude macOS Universal | `claude.ai/api/desktop/darwin/universal/dmg/latest/redirect`；`claude.ai`、`downloads.claude.ai`；Intel/ARM64 独立签名清单 | DMG | 应用名=`Claude.app`；Bundle ID=`com.anthropic.claudefordesktop`；Team ID=`Q6L2SF6YDW` | 清单 minisign/SHA/时效、bundle/version/universal slices、codesign、Gatekeeper、quarantine | Intel/Apple Silicon 启用；下载/验证已闭合，clean install proof pending |
| ChatGPT Windows x64/ARM64 | Microsoft Store Product ID `9PLM9XGG6VKS`；App Installer 自愈只访问 Microsoft WinGet stable Release | 后台 Store 安装/更新 | Identity=`OpenAI.Codex`；Package Family=`OpenAI.Codex_2p2nqsd0c76g0`；Publisher=`CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B` | 受信任 WinGet 位置与身份、固定 Store ID、一次性自愈资产摘要/签名/依赖闭包、安装后 identity/publisher/arch/version postcheck；禁止 Store UI、网页引导器和无授权本地包兜底 | x64 enabled in validation build；ARM64 disabled until Snapdragon clean proof |
| ChatGPT macOS Intel/ARM64 | `persistent.oaistatic.com/codex-app-prod/appcast-x64.xml` / `appcast.xml`；同 host 架构 ZIP 前缀 | 架构 ZIP | 新统一应用名=`ChatGPT.app`；Bundle ID=`com.openai.codex`；Team ID=`2DC432GLL2`；Classic identity 单独记录 | appcast 版本/架构映射、bundle/version/arch、codesign、Gatekeeper、quarantine；官方支持下限 macOS 14；不得卸载 Classic | disabled until Intel/Apple Silicon clean install proof |

运行时可变字段仅限 version、size、官方响应中的 URL/短期 token、digest/signature、release notes 和依赖版本；Claude 四平台回退还可提供签名清单内的不可变相对路径、摘要和同步时间。回退主机、清单公钥、最大陈旧期、Package/Bundle 身份不能远程放宽。注册表加载失败、字段为空、adapter 试图使用未登记值时必须编译失败或运行时 fail closed。

## 7.3 执行阶段与并行安全

1. `P0 proof-first`：先在可丢弃环境关闭 `UNK-AIINST-001..004`，固定 trust-registry。只允许为取证写最小探针，不先完成生产 adapter。ChatGPT Windows Store 授权/无 UI 链、WorkBuddy macOS 包型、Hermes 跨架构、五款 macOS identity 是硬输出。
2. `P1 foundation/core`：P0 可与纯 UI/构建 foundation 并行；core 必须先建立状态机、临时文件和验证接口，任何真实 adapter 不得绕开。
3. `P2 adapters/UI`：完成注册表条目的 adapter 可并行实现；UI 在 core contract 固定后并行。某一产品/平台 P0 No-Go 只使该项显示 unsupported，不阻塞其余四产品、另一操作系统或 ChatGPT macOS。
4. `P3 assembly`：最终源码 revision 上完成四类硬件矩阵、受限网络/代理、安全夹具和仓库保护检查。
5. `P4 signed release`：仅在 `external_prerequisites` 到位后执行 Authenticode、Developer ID、notarization、staple 和最终制品复验。凭据缺失时停在 `Implementation complete; validation pending`，未签名测试包不作为用户交付物。

# 8. TASK 与 ASSEMBLY 计划

### TASK-AIINST-FOUNDATION

- `links`: `OBJ-AIINST-001`, `REQ-AIINST-005`, `ASM-AIINST-001`, `ASM-AIINST-002`
- `owns_behavior`: 建立隔离子项目、Rust 原生 UI walking skeleton、构建与打包骨架，证明安装助手自身能在目标平台启动且不依赖额外 GUI bootstrap。
- `business_result`: 用户获得可运行的空壳 Windows x64/ARM64 便携 EXE 和 macOS Universal app/测试 DMG，后续逻辑有真实载体。
- `target_delta`: 从仅有文档变为可扫描系统并显示五产品占位状态的签名前测试壳。
- `integration_edges`: App Shell → Runtime Context；构建 → packaging；不接任何真实安装动作。
- `expected_touchpoints`: Cargo workspace、app/main、platform probe、packaging scripts、CI matrix、licenses/notices。
- `linked_tests`: `TEST-AIINST-FOUNDATION`
- `stop_conditions`: 产物需要安装额外 GUI runtime、无法生成 Windows ARM64 或 macOS Universal、许可证不适合分发，或必须把代码放进现有项目责任边界。

### TASK-AIINST-CORE

- `links`: `REQ-AIINST-001`, `REQ-AIINST-003`, `REQ-AIINST-004`, `REQ-AIINST-006`, `REQ-AIINST-008`, `REQ-AIINST-009`, `REQ-AIINST-010`, `INV-AIINST-001`, `INV-AIINST-002`, `RULE-AIINST-002`, `RULE-AIINST-004`, `RULE-AIINST-005`, `RULE-AIINST-008`, `RULE-AIINST-009`, `RULE-AIINST-011`
- `owns_behavior`: 实现类型安全的每产品状态机、并行下载/校验、按 MSI/AppX 引擎隔离的系统安装通道、厂商 EXE/macOS 直并行、独立取消、受约束 trust-registry、随机唯一临时目录与重解析点防护、redirect/domain 校验、摘要/签名验证接口、结构化进程启动、macOS Gatekeeper postcheck 和脱敏日志。
- `business_result`: 五个适配器只需声明产品事实即可进入同一安全闭环，不重复实现下载/取消/日志。
- `target_delta`: 从 UI 空壳变为可由 fake adapter 完成成功、失败、取消和复检的完整本地流程。
- `integration_edges`: UI command → orchestrator → adapter contract → downloader/verifier → platform installer → postcheck → UI state/log。
- `expected_touchpoints`: core model/state/orchestrator、HTTP client、temp lifecycle、signature abstraction、platform process API、fake adapter/tests。
- `linked_tests`: `TEST-AIINST-CORE`, `TEST-AIINST-SECURITY`
- `stop_conditions`: 需要执行远端命令、需要持久化任意安装脚本、无法避免整体提权、状态机允许未验证文件进入 installer。

### TASK-AIINST-WORKBUDDY

- `links`: `REQ-AIINST-002`, `REQ-AIINST-006`, `INV-AIINST-002`, `RULE-AIINST-006`, `FACT-AIINST-003`, `UNK-AIINST-002`
- `owns_behavior`: WorkBuddy Windows x64 与 macOS Intel/Apple Silicon 的检测、官方版本解析、包选择、签名/哈希策略、交互安装和复检。
- `business_result`: 受支持平台能安装当前官方 WorkBuddy；Windows ARM64 被准确标记为无官方原生包。
- `target_delta`: 关闭 macOS ZIP/DMG proof，并把 update API 的易变字段安全映射为候选。
- `integration_edges`: WorkBuddy adapter → official update API/download domain → shared verifier → EXE 或 macOS app install → registry/bundle postcheck。
- `expected_touchpoints`: adapter、三平台 fixture、Windows signer allowlist、mac bundle/team evidence、clean-machine scripts/evidence。
- `linked_tests`: `TEST-AIINST-WORKBUDDY`
- `stop_conditions`: 只能通过第三方镜像获取、macOS 无法建立 hash/signature identity、Windows ARM64 只能依赖推测性模拟安装、静默参数无官方或实测依据。

### TASK-AIINST-HERMES

- `links`: `REQ-AIINST-002`, `REQ-AIINST-004`, `INV-AIINST-002`, `FACT-AIINST-004`, `UNK-AIINST-003`
- `owns_behavior`: Hermes 官网 current/build 解析、signed bootstrap 验证与启动、桌面安装状态和 runtime readiness 分离、跨架构 proof。
- `business_result`: 用户能启动经过本工具来源与签名验证的官方 Hermes bootstrap，并清楚知道“本工具验证范围止于 bootstrap”“桌面已装”还是“runtime 已就绪/待首次配置”。
- `target_delta`: 从静态签名证据变为 Windows x64/ARM64、macOS Intel/Apple Silicon 的真实 bootstrap 结果。
- `integration_edges`: Hermes adapter → official landing/assets → bootstrap → desktop identity/runtime probe → independent result fields。
- `expected_touchpoints`: HTML parser/fixture、Windows signer、bundle id、runtime probes、bootstrap disclosure、platform evidence。
- `linked_tests`: `TEST-AIINST-HERMES`
- `stop_conditions`: 官网结构无法稳定识别、bootstrap 签名不符、跨架构结果不可确定、UI 无法准确披露验证边界、实现试图接管或重写 Hermes 下游脚本/依赖链。

### TASK-AIINST-CCSWITCH

- `links`: `REQ-AIINST-002`, `REQ-AIINST-006`, `INV-AIINST-002`, `FACT-AIINST-005`
- `owns_behavior`: CC Switch signed latest.json 解析、minisign/Tauri 验签、Windows MSI 与 macOS signed artifact 安装、bundle/MSI postcheck。
- `business_result`: 两平台两架构可安装公开 latest release，且不会误用 main 分支版本或 Portable ZIP。
- `target_delta`: 将官方 updater contract 直接复用为外部安装信任链。
- `integration_edges`: CC Switch adapter → primary/fallback latest.json → minisign verifier → MSI/macOS installer → registered identity。
- `expected_touchpoints`: adapter、public key fixture、latest.json fixtures、tamper tests、MSI/bundle detection。
- `linked_tests`: `TEST-AIINST-CCSWITCH`
- `stop_conditions`: 产物验签失败、fallback 不在项目官方配置内、需要扫描或接管 portable/Homebrew 安装。

### TASK-AIINST-CLAUDE

- `links`: `REQ-AIINST-002`, `REQ-AIINST-004`, `REQ-AIINST-006`, `INV-AIINST-002`, `FACT-AIINST-006`, `UNK-AIINST-004`
- `owns_behavior`: Claude Desktop Windows x64/ARM64 官方完整 MSIX 与 macOS Universal DMG 的解析、校验、安装、检测和 Code 前置条件提示。
- `business_result`: 用户安装的是包含 Code 页的 Claude Desktop，而不是 CLI；Windows 不经过脚本安装器。
- `target_delta`: 固化 Anthropic 官方部署文档公开的 redirect 与 package/bundle identity；为 Windows x64/ARM64 与 macOS Intel/Apple Silicon 增加官方可用性失败后的固定签名回退；获得四类硬件 clean-machine 和 macOS signing evidence。
- `integration_edges`: Claude adapter → official MSIX/DMG redirect；允许的可用性失败进入固定签名清单 → MSIX/DMG verifier → Windows 本地 `Add-AppxProvisionedPackage` 管理员部署或 mac app install → package/bundle postcheck → Git/subscription informational warning。
- `expected_touchpoints`: adapter、官方/镜像 fixture、镜像签名/时效/URL 隔离、MSIX AppX 签名/Publisher/最终 Package identity tests、管理员部署错误回执、UI 版本/状态呈现、脱敏来源审计、bundle/team proof、managed-install detection。
- `linked_tests`: `TEST-AIINST-CLAUDE`
- `stop_conditions`: 需要安装 CLI/Node、MSIX 签名/Publisher/架构或最终 Package identity 不符、企业受管安装会被破坏。

### TASK-AIINST-CHATGPT

- `links`: `REQ-AIINST-002`, `REQ-AIINST-006`, `REQ-AIINST-007`, `REQ-AIINST-012`, `DEC-AIINST-006`, `INV-AIINST-002`, `INV-AIINST-003`, `FACT-AIINST-007`, `UNK-AIINST-001`, `UNK-AIINST-004`
- `owns_behavior`: ChatGPT Windows 单动作受信任 WinGet 检查/一次性自愈、固定 Store ID 后台安装和 package-family/version postcheck；macOS 官方 appcast ZIP 解析、安装和新/Classic 区分。
- `business_result`: 用户只点击一个安装/更新按钮，不会看到通道选择、Store UI、WinGet、引导器或账户登录；后台在固定 OpenAI 官方边界内自动完成或给出一个真实、可诊断的最终失败。
- `target_delta`: 将 ChatGPT Windows 接到既有 Microsoft Store 后台链；固定 Store ID、目标 Package Identity/Family/Publisher、受信任 App Installer 身份和一次性自愈边界；不打开 Store UI，也不保留 OpenAI 安装包。
- `integration_edges`: UI 单动作 → 固定 Store plan → WinGet 健康检查/一次性自愈 → `msstore` 后台安装 → package-family/identity/publisher/architecture/version postcheck；OpenAI macOS download → ZIP → new-app bundle postcheck。
- `expected_touchpoints`: typed Store resolver、trust-registry、Windows Store/WinGet 执行器、App Installer repair/identity tests、mac new-vs-classic detection、clean-machine evidence。
- `linked_tests`: `TEST-AIINST-CHATGPT`
- `stop_conditions`: 固定 Store ID、受信任 WinGet/App Installer Identity/Family/Publisher/安装位置、Microsoft 修复资产摘要/签名/依赖闭包、OpenAI Identity/Publisher/Family/架构或 postcheck 任一无法确认；需要管理员级系统策略绕过、长期保存/镜像 OpenAI 包、打开 Store UI/URI、执行网页引导器、人工登录账户或依赖 FE3 私有抓包才能继续。上述结果使 ChatGPT Windows 进入失败/No-Go，不阻塞其他产品或 ChatGPT macOS。

### TASK-AIINST-UI

- `links`: `REQ-AIINST-001`, `REQ-AIINST-003`, `REQ-AIINST-008`, `REQ-AIINST-012`, `SCN-AIINST-S1`, `SCN-AIINST-S2`, `SCN-AIINST-S3`, `SCN-AIINST-S7`, `SCN-AIINST-S8`
- `owns_behavior`: 把真实 core 状态呈现为五产品列表、扫描、选择、确认、逐项进度、等待厂商窗口、结果摘要和日志导出。
- `business_result`: 非技术用户能理解将安装什么、为何不能安装、当前卡在哪一步，并能在一个产品失败后继续。
- `target_delta`: 从开发状态视图变为可交付的最小用户流程；不增加设置中心、账户或插件页。
- `integration_edges`: UI action/state subscription ↔ orchestrator；系统对话框/安装器窗口提示；summary/log export。
- `expected_touchpoints`: app view/model、accessibility labels、confirmation modal、progress/cancel/error/result screens、UI tests。
- `linked_tests`: `TEST-AIINST-UI`, `TEST-AIINST-ASSEMBLY`
- `stop_conditions`: UI 自己持有第二套安装状态、暴露 ChatGPT 内部通道选择、可绕过验证、为了视觉效果引入浏览器 runtime 或复杂前端框架。

### TASK-AIINST-RELEASE

- `links`: `REQ-AIINST-005`, `INV-AIINST-003`, `INV-AIINST-004`, `UNK-AIINST-005`
- `owns_behavior`: 最终矩阵构建、SBOM/第三方通知、自家制品签名、公证、哈希、干净机器 smoke test 和发布说明。
- `business_result`: 用户获得可信、可启动的 Windows x64/ARM64 便携 EXE 与一个 macOS Universal DMG。
- `target_delta`: 从开发构建变为可交付签名制品，不携带第三方安装包或私钥。
- `integration_edges`: final source revision → platform builds → signing/notarization → clean-machine scan/install smoke → checksums/release manifest。
- `expected_touchpoints`: packaging、CI/release scripts、entitlements、certificate secret references、SBOM/licenses、release/evidence docs。
- `linked_tests`: `TEST-AIINST-RELEASE`, `TEST-AIINST-ASSEMBLY`
- `stop_conditions`: 缺少合法签名凭据、私钥会进入仓库/日志、Windows EXE 仍依赖额外安装、macOS Gatekeeper 拒绝、制品包含任一第三方安装包。

### ASSEMBLY-AIINST-001

- `participating_tasks`: `TASK-AIINST-FOUNDATION`, `TASK-AIINST-CORE`, `TASK-AIINST-WORKBUDDY`, `TASK-AIINST-HERMES`, `TASK-AIINST-CCSWITCH`, `TASK-AIINST-CLAUDE`, `TASK-AIINST-CHATGPT`, `TASK-AIINST-UI`, `TASK-AIINST-RELEASE`
- `end_to_end_entry`: 用户从最终签名制品启动扫描，选择产品并确认安装。
- `shared_contract_state_data`: `RuntimeContext`、五个 `ProductState`、`ReleaseCandidate`、`VerificationReport`、`InstallOutcome`、`PostcheckResult` 与 `BatchSummary`；所有模块使用同一状态机和产品身份，不复制第二套 UI 真相。
- `final_consumer`: 操作系统中注册且通过包/Bundle/签名/版本复检的真实目标应用，以及用户看到的最终批次摘要。
- `cross_task_failure_path`: 任一 resolver、下载、验证、安装或复检失败只终止对应产品；ChatGPT Windows proof No-Go 只禁用该产品/平台；全局磁盘/安全故障停止批次；崩溃重启后重新扫描并清理过期临时文件，不从半完成状态猜测续装。
- `linked_test_evidence_gate`: `TEST-AIINST-ASSEMBLY` / `EV-AIINST-ASSEMBLY` / `GATE-AIINST-ASSEMBLY`
- `owns_behavior`: 在最终 revision 上串联系统检测、五适配器、下载/验证、平台安装、UI、日志和发布制品，执行完整矩阵与保护范围检查。
- `business_result`: 同一安装助手在四类硬件环境上对五产品给出一致、可信的安装或不支持结果。
- `integration_edges`: 所有 TASK 的生产入口与真实最终消费者；不以 mock 结果替代真机 proof。
- `linked_tests`: `TEST-AIINST-ASSEMBLY`, `TEST-AIINST-SECURITY`, `TEST-AIINST-RELEASE`
- `stop_conditions`: 任一 required 产品/平台未通过且未被官方“不支持”规则明确排除；最终制品与测试 revision 不一致；文档或信任配置落后于实现。

# 9. 验证与验收

- `consumer_chain_validation`: 从用户选择开始，沿 UI → orchestrator → product adapter → official resolver → downloader/verifier → platform installer → OS package/bundle/registry → postcheck → final summary 验证；任何 mock 只可用于单元测试，不能替代至少一次相应平台真机闭环。
- `real_integration_evidence`: 使用可回滚的干净 Windows x64、Windows ARM64、macOS Intel、macOS Apple Silicon 环境；记录 OS build、安装助手 hash、解析元数据摘要、目标包 hash/signature、安装动作、最终应用 identity/version 和清理结果。不得在用户当前主机上擅自安装。
- `failure_recovery_ownership_validation`: 执行者负责把每个失败状态接到明确恢复动作并用对应 TEST 取证：解析/网络问题可重试，验证失败清理并拒绝执行，外部安装器取消后复检，批次失败隔离，崩溃后重新扫描；用户只负责许可/UAC 等必须由人完成的交互和范围裁决，不能被测试替身代替。

### RISK-AIINST-001

- `scenario`: 官方页面/API 变化导致解析到旧包、HTML 错误页或错误架构。
- `impact`: 安装失败、安装错包或供应链风险。
- `control_or_acceptance_owner`: 结构化解析、fixture/live smoke、package magic、architecture/identity 验证和失败关闭；执行者负责，用户不接受未缓解结果。
- `linked_tests`: `TEST-AIINST-CORE`, `TEST-AIINST-SECURITY`, 各 adapter TEST。

### RISK-AIINST-002

- `scenario`: ChatGPT 固定 Store ID 不可用、WinGet/App Installer 身份或修复资产不符、Store 授权/策略失败、错误架构安装，或最终 OpenAI AppX 身份/版本复检失败。
- `impact`: 错包、降级、假成功，或重新出现用户明确拒绝的 Store/引导器体验。
- `control_or_acceptance_owner`: 固定清单/URL/Identity/Publisher/Family、包内候选版本一致、执行前二次绑定、direct postcheck、Store/WinGet 不可达测试和 `TEST-AIINST-CHATGPT` clean-machine hard gate。
- `linked_tests`: `TEST-AIINST-CHATGPT`

### RISK-AIINST-003

- `scenario`: 签名主体轮换、证书异常或 bootstrap 下游下载不可见。
- `impact`: 误拒绝官方包或把未审计下游描述成可信。
- `control_or_acceptance_owner`: 固定 signer/Team identity、官方变更复核、清晰 bootstrap 披露、安装助手新版更新信任根。
- `linked_tests`: `TEST-AIINST-SECURITY`, `TEST-AIINST-HERMES`, `TEST-AIINST-WORKBUDDY`

### RISK-AIINST-004

- `scenario`: 安装器需要管理员权限、应用占用、用户取消或安装一半。
- `impact`: 系统状态不一致或假成功。
- `control_or_acceptance_owner`: 普通用户启动、按需提权、禁止强杀、每次 postcheck、无自动回滚承诺、明确恢复建议。
- `linked_tests`: `TEST-AIINST-CORE`, 各 adapter TEST, `TEST-AIINST-ASSEMBLY`

### RISK-AIINST-005

- `scenario`: 自家便携 EXE/DMG 未签名或依赖额外 GUI runtime。
- `impact`: SmartScreen/Gatekeeper 拦截，安装助手自身不可用。
- `control_or_acceptance_owner`: foundation portability proof 与 release signing/notarization gate；签名凭据由用户/发布主体提供。
- `linked_tests`: `TEST-AIINST-FOUNDATION`, `TEST-AIINST-RELEASE`

### TEST-AIINST-FOUNDATION

- `links`: `TASK-AIINST-FOUNDATION`, `REQ-AIINST-005`
- `method`: 在干净 Windows x64/ARM64 与 macOS Intel/Apple Silicon 启动最小制品；检查无安装步骤、无 WebView/Node/Python 等额外运行时要求；macOS 检查 Universal slices。
- `expected_observable_result`: 两个 Windows 便携 EXE 双击可运行；一个 macOS Universal app/DMG 在两类 Mac 可运行；显示真实 OS/build/arch/disk。
- `failure_path_covered`: 缺运行时、错误架构、系统版本过低、只读目录、非 ASCII 用户路径。
- `cannot_prove`: 不证明五产品安装或正式签名声誉。

### TEST-AIINST-CORE

- `links`: `TASK-AIINST-CORE`, `REQ-AIINST-003`, `REQ-AIINST-004`, `RULE-AIINST-002`, `RULE-AIINST-004`, `RULE-AIINST-005`
- `method`: 使用本地 fake HTTP/adapter/installer 覆盖所有状态转移、多产品并行下载/校验、MSI/AppX 同通道 FIFO、跨通道及 EXE 直并行、排队取消、Claude 官方/回退 MSIX 版本绑定与本地机器级部署命令、元数据瞬时断线重试、无 Range 重下、磁盘不足、超时、进程取消与 postcheck mismatch。
- `expected_observable_result`: 未验证文件永不进入 installer；每产品状态与批次摘要一致；失败可重试且临时文件按策略清理。
- `failure_path_covered`: 断网、半包、重定向循环、安装器退出 0 但复检失败、一个产品失败后继续。
- `cannot_prove`: 不证明真实厂商源和平台签名 API。

### TEST-AIINST-SECURITY

- `links`: `TASK-AIINST-CORE`, `ASSEMBLY-AIINST-001`, `INV-AIINST-001`, `INV-AIINST-002`, `INV-AIINST-003`, `INV-AIINST-004`, `INV-AIINST-005`
- `method`: 注入白名单外 redirect、HTML 伪包、篡改摘要、错误 Authenticode/Team ID、错误 package family/bundle、路径/参数注入、symlink/junction/reparse point 越界、超大响应、日志敏感字段；使用受控系统代理和未受信 TLS 替换验证失败可诊断且不会进入安装；检查发布包内容。
- `expected_observable_result`: 所有恶意/不确定输入在执行前被拒绝；参数不经 shell 拼接；日志已脱敏；发布包和仓库无第三方包/私钥。
- `failure_path_covered`: signer 轮换、证书过期/吊销不可查、系统代理不可达、代理替换内容、临时目录符号链接/路径冲突、trust-registry 缺项或远端试图扩权。
- `cannot_prove`: 不证明厂商自身完整供应链或所有企业安全软件兼容。

### TEST-AIINST-WORKBUDDY

- `links`: `TASK-AIINST-WORKBUDDY`, `SCN-AIINST-S1`, `SCN-AIINST-S6`, `SCN-AIINST-S7`
- `method`: fixture + live endpoint；Windows x64 干净机交互安装；Windows ARM64 不支持判断；两类 Mac 分别验证对应架构 ZIP、官方 SHA-256、app 签名、安装与复检。
- `expected_observable_result`: x64/两类 Mac 安装当前官方版本；Mac 已安装 app 通过 codesign 与 Gatekeeper 且 quarantine 未被清除；ARM64 Windows 不下载错包；取消安装不报成功。
- `failure_path_covered`: API 字段缺失、Windows hash 为空、Mac ZIP 摘要/签名不符、交互安装取消。
- `cannot_prove`: 不证明未来更新接口永不变化。

### TEST-AIINST-HERMES

- `links`: `TASK-AIINST-HERMES`, `SCN-AIINST-S1`, `SCN-AIINST-S6`
- `method`: fixture + live homepage；Windows x64/ARM64 与 Apple Silicon Mac 运行 signed bootstrap，记录桌面安装、runtime 首次配置、网络与失败恢复；Intel Mac 验证不会下载并明确显示厂商不支持。
- `expected_observable_result`: 三类受支持环境的正确架构桌面通过平台可执行性检查；Intel Mac 准确 unsupported；结果区分 desktop installed 与 runtime ready；明确 bootstrap 后续依赖不在本工具逐件审计范围，下游失败不被隐藏。
- `failure_path_covered`: HTML/build 变化、bootstrap 取消、依赖网络失败、runtime 不完整、安装目录自定义。
- `cannot_prove`: 不证明 Hermes bootstrap 下游每个依赖的供应链由本工具审计。

### TEST-AIINST-CCSWITCH

- `links`: `TASK-AIINST-CCSWITCH`, `SCN-AIINST-S1`, `SCN-AIINST-S2`, `SCN-AIINST-S5`
- `method`: 对 primary/fallback latest.json 做 fixture/live 校验；篡改产物测试 minisign；Windows x64/ARM64 MSI 与两类 Mac 安装/复检。
- `expected_observable_result`: 只安装 public latest signed release；macOS app 同时通过 updater 签名、codesign 与 Gatekeeper；main ahead version 不影响判断；portable/Homebrew 不被误接管。
- `failure_path_covered`: 主端点不可用、fallback、签名篡改、错误平台 URL、已装更高版本。
- `cannot_prove`: 不证明作者未来永不更换公钥或包格式。

### TEST-AIINST-CLAUDE

- `links`: `TASK-AIINST-CLAUDE`, `SCN-AIINST-S1`, `SCN-AIINST-S2`, `SCN-AIINST-S7`
- `method`: official redirect live check；四平台清单验签/篡改/过期/URL 越界与架构限定；验证仅 403/451、408/429、已识别地区不可用、连接/超时/响应体中断及服务端错误可回退，未知重定向、合同/白名单/证书/签名/Identity/Publisher/Bundle/Team 异常禁止回退；Windows x64/ARM64 Setup clean install/update/postcheck 与无管理员权限基础路径；macOS Universal DMG 两架构；检查 managed package、Git 缺失提示与自更新共存。
- `expected_observable_result`: 安装 Claude Desktop 且 Code 页可见；macOS app 通过 codesign/Gatekeeper；CLI 未被安装；缺 Git/订阅只提示不误判安装失败；受管安装不被破坏。
- `failure_path_covered`: redirect challenge/变化、镜像签名篡改/过期/越界、错误回退分类、Setup signer/PE 架构或最终 Package identity/版本不符、UAC 取消、AppLocker、应用运行中、Gatekeeper 拒绝。
- `cannot_prove`: 不证明用户账号具有 Code 订阅权限或 Cowork 虚拟化能力。

### TEST-AIINST-CHATGPT

- `links`: `TASK-AIINST-CHATGPT`, `REQ-AIINST-007`, `REQ-AIINST-012`, `DEC-AIINST-006`, `SCN-AIINST-S8`, `RISK-AIINST-002`
- `environment_sensitive`: true
- `method`: 用 fixture/自动测试覆盖固定 Store plan、受信任 WinGet 定位、一次性 App Installer 自愈、Microsoft Release 摘要/签名/Bundle/依赖闭包、Store 错误分类、无授权本地兜底禁止、OpenAI Package Identity/Publisher/Family/架构 postcheck、取消与重复点击。在干净 Windows x64/ARM64 上执行唯一 UI 动作，证明首次安装/旧版更新、运行中应用关闭、授权/策略失败分类和 postcheck，监控不得打开 Store UI 或执行网页引导器。两类 Mac 从官方入口安装新统一应用并检查与 Classic 共存、codesign/Gatekeeper/quarantine。
- `expected_observable_result`: Windows 目标 package family/identity/publisher/architecture/version 精确匹配；用户只触发一个动作；WinGet 从受信任 App Installer 位置运行并只使用固定 Store ID；全程不打开 Store UI、不执行网页引导器或服务端命令；任一授权、策略、信任、身份、架构、部署或复检异常分类失败且不切换到无授权本地包。macOS 新应用可由 Gatekeeper 接受且不卸载 Classic。
- `failure_path_covered`: WinGet 缺失/过旧/身份异常、App Installer 修复摘要/签名/依赖异常、Store 网络/授权/策略失败、OpenAI Identity/Publisher/Family/架构不匹配、部署失败、运行中应用、postcheck 超时、用户取消、重复点击、Store UI/网页引导器意外启动、Classic 冲突。
- `cannot_prove`: 不证明 OpenAI 官方合同永久稳定；只证明记录日期与系统矩阵。

### TEST-AIINST-UI

- `links`: `TASK-AIINST-UI`, `REQ-AIINST-001`, `REQ-AIINST-003`, `REQ-AIINST-008`
- `method`: 自动状态渲染测试 + 四类目标环境手工流程；覆盖键盘操作、缩放、长产品名/错误、确认、取消、日志导出。
- `expected_observable_result`: 用户在安装前看清当前版本、目标版本、状态和必要权限/功能限制；来源、架构与包身份由后台校验和日志审计；所有 core 状态有唯一可理解呈现，操作不可绕过安全门。
- `failure_path_covered`: 扫描慢、未知安装、无网络、安装器在后台、单项失败、窗口关闭重开。
- `cannot_prove`: 不证明所有辅助技术组合；至少完成系统缩放和键盘可达性基线。

### TEST-AIINST-RELEASE

- `links`: `TASK-AIINST-RELEASE`, `REQ-AIINST-005`, `RISK-AIINST-005`
- `environment_sensitive`: true
- `method`: 对最终 revision 构建 Windows x64/ARM64 与 macOS Universal；执行 Authenticode、codesign、notary/staple、SBOM、第三方通知、hash、恶意内容扫描和干净机启动。
- `expected_observable_result`: 在外部签名凭据到位时 Windows 签名有效、无外部 GUI runtime；macOS Gatekeeper 接受且 Universal；manifest/hash 与最终制品一致；不含第三方安装包或秘密。凭据缺失时该 TEST 明确未通过并保持 `validation pending`。
- `failure_path_covered`: 证书缺失/过期、notary 失败、构建不可复现、错误架构、SmartScreen/Gatekeeper 阻止、私钥泄漏扫描。
- `cannot_prove`: Authenticode 新证书立即获得 SmartScreen 声誉。

### TEST-AIINST-ASSEMBLY

- `links`: `ASSEMBLY-AIINST-001`, `SCN-AIINST-S1`, `SCN-AIINST-S2`, `SCN-AIINST-S3`, `SCN-AIINST-S4`, `SCN-AIINST-S5`, `SCN-AIINST-S6`, `SCN-AIINST-S7`, `SCN-AIINST-S8`
- `environment_sensitive`: true
- `method`: 最终签名制品在四类干净环境运行完整批次：至少一个已安装、一个需更新、一个首次安装、一个故意源/签名失败夹具；随后执行所有真实五产品可用路径和卸载/快照还原。
- `expected_observable_result`: 结果与每个 adapter 独立证据一致；失败隔离；无错包、假成功、降级、残留可信状态或保护范围修改。
- `failure_path_covered`: 批次中间重启/关闭、网络切换、磁盘耗尽、用户取消厂商安装器、目标应用运行中。
- `cannot_prove`: 不证明所有未来厂商版本、地区网络和企业策略组合。

### EV-AIINST-FOUNDATION

- `for`: `TEST-AIINST-FOUNDATION`
- `required_evidence_shape`: 四类目标环境的启动收据、依赖检查、架构信息、macOS slices、截图与构建 revision；证明工具自身无需额外 GUI bootstrap。

### EV-AIINST-CORE

- `for`: `TEST-AIINST-CORE`
- `required_evidence_shape`: 自动测试报告、状态转移覆盖、fake HTTP/installer 场景、临时文件前后快照和最终 revision。

### EV-AIINST-SECURITY

- `for`: `TEST-AIINST-SECURITY`
- `required_evidence_shape`: 每个恶意夹具的拒绝点、未启动进程证据、签名/摘要报告、日志脱敏样例、仓库与制品秘密/第三方包扫描结果。

### EV-AIINST-WORKBUDDY

- `for`: `TEST-AIINST-WORKBUDDY`
- `required_evidence_shape`: live API 摘要、fixture、Windows signer、macOS hash/Team identity、三类支持环境安装前后检测和 Windows ARM64 unsupported 截图。

### EV-AIINST-HERMES

- `for`: `TEST-AIINST-HERMES`
- `required_evidence_shape`: 官网/build 解析收据、bootstrap 签名、四类硬件安装过程、desktop/runtime 双状态和失败恢复日志。

### EV-AIINST-CCSWITCH

- `for`: `TEST-AIINST-CCSWITCH`
- `required_evidence_shape`: primary/fallback 清单、更新公钥与 minisign 验签、tamper 拒绝、Windows 双架构和 macOS 双架构安装前后身份。

### EV-AIINST-CLAUDE

- `for`: `TEST-AIINST-CLAUDE`
- `required_evidence_shape`: official redirect/包元数据；东京强制解析与香港直拉/定时同步记录；镜像清单公钥、签名、时效、SHA-256、不可变路径及脱敏来源日志；MSIX publisher/package identity/包内版本；DMG Team/bundle identity；四类环境安装结果、Git/订阅提示和 managed-install 保护结果。公网发布另附再分发/AUP Go。

### EV-AIINST-CHATGPT

- `for`: `TEST-AIINST-CHATGPT`
- `required_evidence_shape`: OpenAI 官方清单响应、固定 release URL 构造、目标 MSIX signature/Identity/Publisher/Family/architecture/manifest version、direct 状态记录、x64/ARM64 单动作 clean-install/update 录像/日志、Store/WinGet/引导器/登录窗口未启动证明、macOS 新应用与 Classic 共存结果和 Go/No-Go 决策。

### EV-AIINST-UI

- `for`: `TEST-AIINST-UI`
- `required_evidence_shape`: 状态渲染自动测试、四类环境关键流程截图/短录像、键盘/缩放结果、确认信息与脱敏日志导出样例。

### EV-AIINST-RELEASE

- `for`: `TEST-AIINST-RELEASE`
- `required_evidence_shape`: 最终 revision、构建日志、SBOM/third-party notices、Authenticode/codesign/notary/staple 收据、制品 hash、内容清单与干净机启动结果。

### EV-AIINST-ASSEMBLY

- `for`: `TEST-AIINST-ASSEMBLY`
- `required_evidence_shape`: 最终签名制品在四类环境的完整批次收据、每产品安装前后差异、失败隔离、临时资源清理、保护范围 diff 和最终 Go/No-Go 汇总。

- `execution_evidence_destination`: 所有实际 run state、命令结果、制品 manifest、最终 revision 与 Completion Report 写入 `.run/AI-CLIENT-INSTALLER-V1/` 和 `evidence/`，第三方安装包本体不进入仓库，也不回填活动卡事实字段。

| ID | 场景 | 关联 | 通过条件 | 证据 | 不能证明 |
|---|---|---|---|---|---|
| GATE-AIINST-FOUNDATION | 安装助手自身可运行 | TASK-AIINST-FOUNDATION / TEST-AIINST-FOUNDATION | Windows 双架构便携 EXE 与 macOS Universal 测试包在干净环境启动且无额外 GUI runtime | EV-AIINST-FOUNDATION | 正式证书声誉与五产品安装 |
| GATE-AIINST-CORE | 核心状态与失败恢复 | TASK-AIINST-CORE / TEST-AIINST-CORE | 状态机、批次、取消、下载、安装和 postcheck 的成功/失败路径闭环 | EV-AIINST-CORE | 真实厂商源 |
| GATE-AIINST-SECURITY | 执行前安全门 | TASK-AIINST-CORE / TEST-AIINST-SECURITY | 白名单外、篡改、错身份、注入与敏感日志场景均在执行前被拒绝 | EV-AIINST-SECURITY | 厂商内部供应链 |
| GATE-AIINST-WORKBUDDY | WorkBuddy 适配 | TASK-AIINST-WORKBUDDY / TEST-AIINST-WORKBUDDY | Windows x64 与两类 Mac 成功；Windows ARM64 准确 unsupported；无假成功 | EV-AIINST-WORKBUDDY | 未来接口永不变化 |
| GATE-AIINST-HERMES | Hermes 适配 | TASK-AIINST-HERMES / TEST-AIINST-HERMES | Windows x64/ARM64 与 Apple Silicon Mac bootstrap 结果可复现，Intel Mac 准确 unsupported，desktop/runtime 状态不混淆，UI 明示本工具信任边界止于已验证 bootstrap | EV-AIINST-HERMES | bootstrap 下游每个制品的厂商内部审计 |
| GATE-AIINST-CCSWITCH | CC Switch 适配 | TASK-AIINST-CCSWITCH / TEST-AIINST-CCSWITCH | signed latest release 在目标矩阵闭环，portable/Homebrew 不被误接管 | EV-AIINST-CCSWITCH | 未来公钥/包格式不变 |
| GATE-AIINST-CLAUDE | Claude Desktop 适配 | TASK-AIINST-CLAUDE / TEST-AIINST-CLAUDE | 官方优先与四平台受限回退分类正确；清单签名/时效/URL/摘要和脱敏来源审计闭合；UI 只显示版本/状态；四类硬件完成真机路径或明确 validation pending，Code/CLI/企业管理边界未破坏；扩大传播需另有再分发/AUP Go | EV-AIINST-CLAUDE | 用户订阅和 Cowork 能力 |
| GATE-AIINST-CHATGPT | ChatGPT 单动作后台安装硬门 | TASK-AIINST-CHATGPT / TEST-AIINST-CHATGPT | Windows x64/ARM64 在一个 UI 动作下完成受信任 WinGet 检查/一次性自愈、固定 Store ID 后台安装与精确 postcheck，且不打开 Store UI 或执行网页引导器；授权/策略/身份/架构/部署失败即该项 No-Go，不阻塞其余产品；macOS 新应用通过 Gatekeeper | EV-AIINST-CHATGPT | Microsoft Store 服务与授权可用性 |
| GATE-AIINST-UI | 用户流程 | TASK-AIINST-UI / TEST-AIINST-UI | 安装前信息完整、状态唯一可理解、失败可诊断、日志可脱敏导出 | EV-AIINST-UI | 所有辅助技术组合 |
| GATE-AIINST-RELEASE | 正式制品 | TASK-AIINST-RELEASE / TEST-AIINST-RELEASE | 外部签名凭据到位后 Windows Authenticode 与 macOS notarization 通过，制品/manifest/hash 对齐且无第三方包/秘密；缺凭据时不得通过 | EV-AIINST-RELEASE | SmartScreen 即时声誉 |
| GATE-AIINST-ASSEMBLY | 最终端到端与仓库卫生 | TASK-AIINST-FOUNDATION / TASK-AIINST-CORE / TASK-AIINST-WORKBUDDY / TASK-AIINST-HERMES / TASK-AIINST-CCSWITCH / TASK-AIINST-CLAUDE / TASK-AIINST-CHATGPT / TASK-AIINST-UI / TASK-AIINST-RELEASE / TEST-AIINST-ASSEMBLY | 最终签名制品在四类环境完成可用路径或官方依据的 unsupported/No-Go，失败隔离且仅修改允许范围 | EV-AIINST-ASSEMBLY | 所有未来版本、地区网络和企业策略 |

# 10. 产物与完成回写

- `required_deliverables`:
  - `src/` 与构建配置
  - `config/trust-registry.toml`
  - `tests/fixtures/`
  - `packaging/`
  - `docs/maintenance.md`
  - `evidence/`
  - `dist/easy-agent-windows-x64.exe`
  - `dist/easy-agent-windows-arm64.exe`
  - `dist/easy-agent-macos-universal.dmg`
  - `dist/SHA256SUMS.txt`
  - `dist/release-manifest.json`
- `documentation_impact`: updated；实现时同步 README、支持矩阵、五适配器官方入口/稳定身份/变更手册、构建签名和用户故障说明。若某产品/平台 No-Go，必须在支持矩阵和 UI 文案中一致反映。
- `repository_structure_impact`: 项目作为 `E:\Obsidian\workspaces\ai-client-installer` 独立 Git 子仓库；不改父 Obsidian 知识库或原口播剪辑项目结构。
- `repository_hygiene_requirement`: 第三方包、`.part`、真实下载缓存、证书/私钥、notary 凭据、真实用户日志、临时 CDN URL 不得提交；生成物仅在明确发布目录保留；执行前后记录并保护现有 dirty worktree。
- `external_review`: policy=optional；当 ChatGPT FE3/Catalog 解析、trust-registry host/signer/Team/public-key 变化、平台提权/安装参数或第三方许可解释存在可能改变 Go/No-Go 的冲突时触发独立安全/架构复核。信任根变更必须有实现者之外的一次独立复核；普通 UI 和局部解析修复不自动触发外部评审。
- `non_completion_rules`: 任一 required gate、完整 trust-registry、真机矩阵、签名/notarization、文档同步、最终 revision 对齐或保护范围检查缺失时不得标记完成；仅有 mock/单元测试不得宣称可交付；ChatGPT 下载成功但依赖闭包/direct install 未通过不得宣称 Windows 支持；签名凭据缺失时只能记录 `Implementation complete; validation pending`，未签名测试包不是最终交付物。
- `final_revision_revalidation`: required；所有自动化、真机证据、制品 hash、签名和发布 manifest 必须对应同一最终 revision。
- `execution_evidence_destination`: 实际命令、candidate/history、manifest/receipt hash、最终 revision、验证 pending/failed 状态与 Completion Report 只写执行 sidecar/evidence，不回写活动任务卡。

# 11. 计划中的机器验证入口

以下命令是目标项目建立后的稳定验证合同；foundation 若经证据批准更换构建栈，必须先同步本节和相应验收，不能让命令与真实执行链脱节。

```json
{
  "schema_version": 1,
  "validators": [
    {
      "validator_id": "rust-format",
      "command": ["cargo", "fmt", "--check"],
      "cwd": ".",
      "timeout_seconds": 120
    },
    {
      "validator_id": "rust-clippy",
      "command": ["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"],
      "cwd": ".",
      "timeout_seconds": 600
    },
    {
      "validator_id": "rust-tests",
      "command": ["cargo", "test", "--all"],
      "cwd": ".",
      "timeout_seconds": 900
    },
    {
      "validator_id": "fixture-contracts",
      "command": ["cargo", "test", "--test", "resolver_fixtures"],
      "cwd": ".",
      "timeout_seconds": 300
    },
    {
      "validator_id": "security-boundaries",
      "command": ["cargo", "test", "--test", "security_boundaries"],
      "cwd": ".",
      "timeout_seconds": 300
    }
  ]
}
```

真机安装、Windows/macOS 签名、公证、Microsoft Catalog/FE3 live proof 和厂商交互安装器不能由上述命令替代，必须按对应 `TEST-*` 保存环境敏感证据。
