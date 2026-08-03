# 官方图标来源

这些文件只用于在安装器中识别对应产品。PNG/原始 SVG 均来自官方资源；WorkBuddy 的运行时 PNG 由随仓库保存的官方 SVG 机械栅格化，没有重新绘制或改变品牌图形。

| 文件 | 官方来源 | 取证说明 | SHA-256 |
|---|---|---|---|
| `workbuddy.svg` | `https://download.codebuddy.cn/web/workbuddy/39aa83f6eb1effda3c999259e7db691102ff873f/assets/logo.svg` | WorkBuddy 官方站当前首页引用的 40×40 原始矢量图标 | `10ccbc099e7bc3d5ff12c315c4edd7b21386aa7166c1ec19997b3d55010d5c7d` |
| `workbuddy.png` | 由上述 `workbuddy.svg` 使用 Microsoft Edge SVG 渲染器按 4× 比例机械栅格化 | 应用运行时使用的 160×160 PNG | `cc719e9df3edbcc09c0ea3f29fc22e18308d955299d9d36b1c6bee43ed234a36` |
| `hermes.png` | `https://raw.githubusercontent.com/NousResearch/hermes-agent/main/apps/desktop/assets/icon.png` | Nous Research 官方仓库桌面客户端图标 | `d60d164e24fdcf6532133b8ea43c77a201e4b9e9dbc396187b58d51d8590ef52` |
| `cc-switch.png` | `https://raw.githubusercontent.com/farion1231/cc-switch/main/src-tauri/icons/icon.png` | CC Switch 官方仓库当前 Tauri 应用图标 | `04225b1b9c54569ec1ec850ad9f1c9f33ca4f286dab001a3392c0460deb342e5` |
| `claude.png` | 官方 Windows 包 `Claude_1.24012.1.0_x64__pzs8sxrjxfjjc/app/resources/ion-dist/images/claude_app_icon.png` | 从本机已验证的 Anthropic MSIX 包逐字节提取 | `c7b5642f810adfba78781592d9dec18d7eb376c7ebf403c4d882fb9d39f65408` |
| `chatgpt.png` | 官方 Windows 包 `OpenAI.Codex_26.721.11231.0_x64__2p2nqsd0c76g0/assets/Square44x44Logo.targetsize-256_altform-lightunplated.png` | 从本机已验证的新 ChatGPT/Codex 统一应用包逐字节提取 | `b45359d98553406d60c45e699cbe80de6fe733d51661a317ca37b41632b58823` |

更新图标时必须重新从对应官方源取证、更新哈希并进行界面截图复检。
