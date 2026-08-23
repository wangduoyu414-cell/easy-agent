# 构建输出目录

`dist/` 只用于本地或 CI 构建输出，不再提交可执行文件、DMG、校验和或 release manifest。

这样可以避免仓库文件页长期保留已经过期、未签名或旧品牌的验证产物。可复现构建命令见 [安装指南](../docs/installation.md)。正式终端用户文件只有在完成签名、公证（macOS）或 Authenticode（Windows）及干净机验证后才能通过 GitHub Releases 发布；未签名验证文件只能放在明确标注限制的 prerelease 中，且不得冒充正式版或 latest release。
