# easy agent 品牌迁移与 macOS Universal 验证

日期：2026-08-04

## 结论

- 应用已从 `AI Client Installer` 更名为 `easy agent`；Rust 包/可执行文件名为 `easy-agent`，运行时应用 ID 与 macOS Bundle ID 为 `io.github.wangduoyu414-cell.easy-agent`。
- 项目所有者提供的原始 PNG 已无生成式改动地转换为运行时 PNG、Windows `.ico` 和 macOS `.icns`；产品列表继续显示各客户端的官方图标，避免将应用品牌图误作第三方客户端身份。
- 重新构建的 macOS Universal `.app` / DMG 同时包含 `x86_64` 与 `arm64`，应用/DMG codesign、DMG 挂载后的应用复验和 Intel 启动冒烟均通过。
- 所有厂商 macOS 安装条目仍为 `enabled = false`。品牌迁移没有改变 WorkBuddy 摘要不一致、Claude stable redirect challenge、干净机矩阵或正式签名/公证等安全 Gate。

## 资源与包身份

| 使用位置 | 文件 / 值 |
| --- | --- |
| README 与 eframe 窗口图标 | `assets/branding/easy-agent-icon-512.png`，512×512 PNG |
| Windows EXE 资源 | `assets/branding/easy-agent.ico`，256×256 ICO；`build.rs` 写入图标、ProductName、FileDescription、InternalName 和 OriginalFilename |
| macOS App 图标 | `packaging/macos/easy-agent.icns`，1,519,508 bytes |
| macOS 显示名 | `easy agent` |
| macOS 可执行文件 | `easy-agent` |
| macOS Bundle ID | `io.github.wangduoyu414-cell.easy-agent` |
| macOS Bundle 图标字段 | `CFBundleIconFile=easy-agent.icns` |

`cargo test --test branding_contract` 会在以后阻止运行时图标、Windows 资源与 macOS Bundle 身份出现漂移。

## 当前工作树构建结果

- `cargo fmt --all`、`cargo test --all-targets`、`cargo clippy --all-targets --all-features -- -D warnings` 均通过；测试结果为 55 项通过、2 项网络型证据测试按设计忽略。
- macOS x86_64 与 ARM64 Release 均成功构建，随后由 `packaging/build-macos.sh` 使用 `EASY_AGENT_DIST_DIR` 写入仓库外的临时目录。
- 临时 DMG：12,365,375 bytes，SHA-256 `3900c2b45da5f5214bb672bd20b07b78ca367aa377b009fb92da33c465cbeeb1`。
- Universal 主程序 SHA-256：`8442e8594dfbe236bf8869e9688c6b236820e5a7f9a17df70c7e28e58132be8d`。
- `lipo -verify_arch x86_64 arm64`、app 深度 codesign、DMG codesign、DMG 只读挂载和挂载后 app 深度 codesign 全部通过。
- Intel 主机从当前 `easy agent.app` 直接启动主程序并持续运行 30 秒，验证流程随后以 SIGTERM 终止（状态 143）；没有触发任何第三方客户端安装动作。
- `spctl --assess --type execute` 返回状态 3 / `rejected`，符合 ad-hoc、未公证验证包的预期；这不是正式 Gatekeeper 通过证据。

## 发布与仓库首页

- `README.md` 已采用品牌头图、价值主张、真实状态徽章、安装决策表、工作流程、安全边界、平台矩阵、文档和贡献入口。
- 主页取舍调研见 `docs/github-homepage-design.md`；其中对 Ollama、Tauri、LocalSend 和 RustDesk 的公共 README 结构进行了对照，但没有伪造其下载量、CI、合作关系或背书。
- 新增手动 GitHub Actions 验证工作流：macOS Universal 和 Windows x64/ARM64。Windows 构建在 Windows runner 上执行，确保 `.ico` 真正进入原生 EXE 资源。

## 清理

- 本次 Universal 构建目录位于仓库外的 `/tmp/easy-agent-branded-proof.HDttxF`，在最终检查完成后删除。
- 没有将第三方客户端安装包、签名凭据、用户日志或临时下载 URL 写入仓库。
