# Nekro Qantrip

**Qantrip** 读作 /ˈkæntrɪp/，与 cantrip 同音：D&D 里不耗资源、可以随手反复施放的「戏法」。首字母换成 Q，呼应唤起快捷键——**按 Q 施放一个戏法**。不是拼写错误。

跨平台的 Prompt 快捷输入与管理工具。在任何应用里按下快捷键，输入几个字母和参数，就能把一段完整的 Prompt 插入当前输入框：

```
r security --scope=staged --strict 重点看认证和权限
```

- **参数化模板**：MiniJinja 渲染、声明式参数（开关、单选、多选、文本、数字），位置参数与附加要求。
- **可发现的命令行**：分层匹配、灰字补全、语法高亮、实时预览，鼠标也能点选参数。
- **可靠插入**：剪贴板 + 粘贴，粘贴后还原剪贴板，不进入剪贴板管理器历史；没有权限时退回只复制。
- **本地优先**：模板是普通的 Markdown 文件，离线可用。

## 状态

macOS Demo（P0）。Windows 与 Linux、端到端加密同步、AI 辅助编辑在规划中，见 [`docs/PRD.md`](docs/PRD.md) 与 [`docs/decisions.md`](docs/decisions.md)。

安装：[`docs/install-macos.md`](docs/install-macos.md)

## 构建

需要 Rust（stable）与 Xcode Command Line Tools，不需要完整的 Xcode。

```sh
scripts/build-macos.sh          # 开发构建，产物在 dist/
ARCH=x86_64 scripts/package-macos.sh   # 打包 DMG，产物在 dist/release/
cargo test --manifest-path core/Cargo.toml
```

目录结构：

| 路径 | 内容 |
|---|---|
| `core/` | Rust 核心：输入语法、模板库与渲染、使用记录，通过 UniFFI 导出 |
| `core/templates/` | 内置模板 |
| `apps/macos/` | macOS 外壳（Swift，AppKit + SwiftUI） |
| `tests/spike/` | 插入链路的自动化测试 |
| `docs/` | 产品需求、输入语法规范、决策记录、可行性报告 |

## 许可证

客户端与核心以 [GPL-3.0-or-later](LICENSE) 发布。内置字体 Monaspace Neon 与 Fraunces 以 SIL OFL 1.1 分发，许可证见 `apps/macos/Resources/Fonts/`。
