# easy agent 品牌资源

`easy-agent-icon.png` 是项目所有者于 2026-08-04 提供的原始应用图标。本目录中的其他文件均从该原图无生成式转换而来：

- `easy-agent-icon-512.png`：eframe 运行时窗口图标与 GitHub README 标识；
- `easy-agent.ico`：Windows EXE 资源；
- `../../packaging/macos/easy-agent.icns`：macOS `.app` / DMG 资源。

替换品牌时必须同时更新上述三种平台资源，并运行 `cargo test --test branding_contract`，避免 Windows、macOS 和 README 出现不一致的应用图标。
