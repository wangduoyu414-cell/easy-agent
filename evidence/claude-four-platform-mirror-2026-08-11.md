# Claude 四平台固定回退与 Windows 构建证据

日期：2026-08-11

## 已部署链路

- 东京强制解析器只接受 Windows x64、Windows ARM64、macOS Universal 三个固定目标；任意其他命令被拒绝。
- 香港同步器为 Windows x64、Windows ARM64、macOS Intel、macOS Apple Silicon 发布四组独立 minisign 清单和不可变制品路径。
- 定时服务二次运行结果为 `success`，四个平台均复用并重新核对当前不可变制品；Nginx 配置检查通过。
- 四个平台公网制品的 `Range: bytes=0-0` 均返回 HTTP 206，`Content-Range` 总长度与签名清单一致。

当前 Claude `1.26832.0`：

| 平台 | 字节数 | SHA-256 |
| --- | ---: | --- |
| Windows x64 Setup | 7020704 | `c91e8b5d92cf60aa5e688f4e6bb6053b247c29e252724034656a4889df97c814` |
| Windows ARM64 Setup | 6471840 | `a7ddb6419c78fad206b3fe1eb214afc505a91ccdcefc13e5ba8df76600fe7109` |
| macOS Universal（Intel 清单） | 348265472 | `7d471f79873777173df0771e36ec9b44cb210b5dc796fd6c7b529b48830eb5d7` |
| macOS Universal（ARM64 清单） | 348265472 | `7d471f79873777173df0771e36ec9b44cb210b5dc796fd6c7b529b48830eb5d7` |

Windows 同步侧验证 Setup 的原生 PE 架构、版本资源、Authenticode 完整性和 Anthropic 签名；macOS 同步侧验证 `Claude.app`、Bundle ID、版本、最低 macOS 版本和 x64/ARM64 双 slice。客户端仍会再次验证 Windows Setup，并在执行后复检最终 Claude Package；Mac 端继续执行 Apple codesign/Team/Gatekeeper 验证。

## 客户端验证

- 最终工作树的全部自动测试通过：89 项执行通过，7 项环境型测试默认忽略。
- Clippy `-D warnings` 通过。
- 四平台在线测试同时覆盖当前官方解析和固定签名回退，清单签名、平台/架构、版本和下载阶段绑定均通过。
- 未知重定向、安全合同、签名、时效、路径、大小、摘要或平台身份错误继续失败关闭。
- 多产品并发测试证明下载/校验互不阻塞，真正安装与 postcheck 通过 FIFO 通道串行，排队状态可单独取消。

## Windows 测试构建

| 文件 | PE machine | SHA-256 |
| --- | --- | --- |
| `easy-agent-windows-x64.exe` | AMD64 `0x8664` | `3da9a83726490875b0a2ea380399042747c4380a58a4a187b04babcd280236ae` |
| `easy-agent-windows-arm64.exe` | ARM64 `0xAA64` | `49cbf963e445b50eecbae8001470ab3cae1f5d2c3da6d4956d0ef86cd5fc60fe` |

两者均为 Windows GUI 子系统，包含 ICON、GROUP_ICON 和 VERSIONINFO，版本资源中的产品名、文件描述和内部名均为 `easy agent`，只导入 Windows 系统 DLL。x64 为 9,645,568 bytes，ARM64 为 8,582,144 bytes。它们尚未做 Authenticode 签名，因此状态为 `Implementation complete; validation pending`：仍需对应 Windows 真机启动、下载、安装和 postcheck 证据。

本轮测试包与 `SHA256SUMS.txt`、中文测试说明已写入新的真实 SMB 共享：`smb://192.168.0.119/qimistudio/easy-agent-测试版-2026-08-11-224451/`。本机稳定入口为 `/Users/admin/Volumes/qimistudio`，指向当前挂载的 `/Volumes/qimistudio`；旧中文路径不再作为交付入口。
