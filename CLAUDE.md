# Nekro Qantrip

跨平台（macOS / Windows / Linux）Prompt 快捷输入与管理工具：全局快捷键唤起命令面板，搜索并参数化生成 Prompt，插入到当前应用；支持 AI 辅助编辑模板和 Cloudflare 端到端加密同步。开源，计划发布到 `NekroAI/qantrip`。

## 当前阶段

**macOS Demo 0.1.0 已发布（预发布，私有仓库 Release）。** 技术路线为 Rust 核心 + UniFFI + 各平台原生外壳（D6）。macOS P0 已完成：面板、语法、17 个内置模板、设置、模板库与编辑器、导入导出、引导、图标（D7–D21）。打包与发布流程见 D22。云服务（`apps/cloud/`，D23 / D24）：官网与同步服务端（C0、C1）已上线 `qantrip.nekro.ai`（staging：`qantrip-staging.nekro.ai`），安全设计见 `docs/sync-design.md` v0.3。C2 同步客户端（`core/src/sync/`）与 C3 macOS 同步界面（设置 › 同步）已完成，待负责人整体测试；官网与应用已中英双语（D29）。面板指令模式（D31）已完成，随 0.3.0 发布。Windows 外壳（C# + WinUI 3，D32）在 `apps/windows/`，可行性验证通过，见 `docs/windows-spike.md`。

- 同步联调：`cargo run --example sync_cli -- <server> <dir> <命令>` 在命令行扮演一台设备（见文件头注释）

- 开发构建：`scripts/build-macos.sh`（产物在 `dist/`，本机开发证书签名）
- 打包：`ARCH=arm64|x86_64 scripts/package-macos.sh`；正式发布：推送 `v*` 标签，由 GitHub Actions 签名并发布
- 云服务：`cd apps/cloud && pnpm test`（契约测试）、`pnpm deploy:staging` / `pnpm deploy:production`（从 Mac mini 部署，wrangler 已登录）
- Windows 构建（在 Windows 上）：`apps\windows\build.ps1`，说明见 `apps/windows/README.md`；回归脚本在 `tests/windows/`
- 测试：`cargo test --manifest-path core/Cargo.toml`；插入链路回归：以 `QANTRIP_DEBUG_HOOKS=1` 构建后运行 `tests/spike/run_insert_tests.py`

## 文档

| 文件 | 内容 |
|---|---|
| `docs/PRD.md` | 产品需求文档 v0.3。各章节的修订标记说明了改动来源 |
| `docs/input-grammar.md` | 面板 / CLI 共用的输入语法规范草案 |
| `docs/decisions.md` | 已做决定及理由、待决事项、未验证假设、可复用的现有 Cloudflare 代码位置 |
| `docs/spike-report.md` | 阶段 1 macOS 可行性报告与实现要点 |
| `docs/windows-spike.md` | Windows（WinUI 3）可行性报告：热键、面板、插入、输入法与绑定方式 |
| `docs/install.md` | macOS 与 Windows 安装说明（同时用作 Release 说明） |
| `docs/sync-design.md` | 同步与端到端加密安全设计 v0.3（含成本模型、两轮审查记录） |

开始任何设计或实现工作前，先读 `docs/decisions.md`，避免重新讨论已经定下的事项。

## 关键约束速览

- Local-first：搜索、渲染、插入必须离线可用，同步和 AI 都不能阻塞主流程。
- 端到端加密是强制要求：服务端不得持有可解密密钥。**不要复用** `nekro-nxt-community` 的 `server/crypto.ts`（服务端持有密钥模型）。
- 插入用剪贴板 + 粘贴，默认不自动提交；必须有不需要任何系统权限的"只复制"模式。
- macOS 暂无 Developer ID：发布版本一律用「Nekro Qantrip Release」自签名证书签名（存于 Actions Secrets），不得更换（D1 / D22）。
- 模板渲染具有确定性，不依赖 AI。
- 技术选型：Rust 核心 + 原生外壳，不以 Tauri 为主框架（D6）。
