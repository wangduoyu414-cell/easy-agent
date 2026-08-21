# 构建输出目录

`dist/` 只用于本地或 CI 构建输出，不再提交可执行文件、DMG、校验和或 release manifest。

这样可以避免仓库文件页长期保留已经过期、未签名或旧品牌的验证产物。可复现构建命令见 [安装指南](../docs/installation.md)；面向用户的文件只应在完成签名、公证（macOS）或 Authenticode（Windows）及干净机验证后，通过 GitHub Releases 发布。
