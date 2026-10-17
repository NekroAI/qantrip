# Nekro Qantrip

跨平台（macOS / Windows / Linux）Prompt 快捷输入与管理工具：全局快捷键唤起命令面板，搜索并参数化生成 Prompt，插入到当前应用；支持 AI 辅助编辑模板和 Cloudflare 端到端加密同步。开源，计划发布到 `NekroAI/nekro-qantrip`。

## 当前阶段

**Demo 阶段 2（macOS）。** 阶段 1 可行性 Spike 已通过（`docs/spike-report.md`），技术路线为 Rust 核心 + UniFFI + 各平台原生外壳（D6）。当前目标是 P0 纵向切片：输入语法解析、MiniJinja 渲染、内置模板、面板体验改版（D7 / D9 / D10）。

构建：`scripts/build-macos.sh`（产物在 `dist/`）。插入链路回归测试：`tests/spike/run_insert_tests.py`。

## 文档

| 文件 | 内容 |
|---|---|
| `docs/PRD.md` | 产品需求文档 v0.3。各章节的修订标记说明了改动来源 |
| `docs/input-grammar.md` | 面板 / CLI 共用的输入语法规范草案 |
| `docs/decisions.md` | 已做决定及理由、待决事项、未验证假设、可复用的现有 Cloudflare 代码位置 |
| `docs/spike-report.md` | 阶段 1 macOS 可行性报告与实现要点 |

开始任何设计或实现工作前，先读 `docs/decisions.md`，避免重新讨论已经定下的事项。

## 关键约束速览

- Local-first：搜索、渲染、插入必须离线可用，同步和 AI 都不能阻塞主流程。
- 端到端加密是强制要求：服务端不得持有可解密密钥。**不要复用** `nekro-nxt-community` 的 `server/crypto.ts`（服务端持有密钥模型）。
- 插入用剪贴板 + 粘贴，默认不自动提交；必须有不需要任何系统权限的"只复制"模式。
- macOS 暂无 Developer ID：用固定自签名证书签名。
- 模板渲染具有确定性，不依赖 AI。
- 技术选型：Rust 核心 + 原生外壳，不以 Tauri 为主框架（D6）。
