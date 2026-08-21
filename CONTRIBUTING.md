# 参与贡献

感谢你帮助改进 `easy agent`。这个项目的首要目标是可靠和可审计，而不是通过扩大下载范围来提高“兼容率”。

## 开始前

1. 阅读 [README](README.md)、[实现与验证状态](docs/implementation-status.md) 和 [维护手册](docs/maintenance.md)。
2. 搜索现有 Issue，避免重复工作；涉及产品分发合同的改动请先说明官方依据。
3. 不要提交第三方安装包、账号数据、令牌、完整临时 URL、用户日志或开发者签名凭据。

## 本地检查

```bash
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --target x86_64-apple-darwin
cargo check --all-targets --target aarch64-apple-darwin
```

如变更应用名、图标、Bundle 或 Windows EXE 资源，还必须执行：

```bash
cargo test --test branding_contract
```

## 对安全敏感的改动

以下项目都属于信任根或执行边界，不能凭网页猜测、通配符或“兼容模式”放宽：

- 允许的下载主机、URL 路径、包类型、摘要和 updater 公钥；
- Authenticode 主体、MSIX Identity/Family/Publisher；
- macOS 应用名、Bundle ID、Developer Team ID、架构、codesign 和 Gatekeeper 状态；
- 归档展开、链接、临时目录、安装替换和回滚代码。

修改这些内容的 Pull Request 必须提供官方来源、脱敏的只读取证、覆盖新旧行为的测试，以及明确的 Intel/Apple Silicon 或 Windows 架构验证计划。不要为了启用一个按钮而绕过未关闭的 Gate。

## 文档与界面

请让用户先看到结论、受支持平台和安全限制，再看到细节。避免把“验证中”“内部构建”或“已观察到的制品身份”写成“已正式发布”或“所有用户可安装”。
