# 决策记录

记录 PRD 形成过程中的评审意见、已做决定、待决事项和未验证假设。PRD 写"是什么"，本文写"为什么"。

---

## 背景

- PRD v0.1 由项目负责人与 GPT 讨论形成，原名"Prompt Launcher"。
- 2026-10-17 经 Claude 评审，形成 v0.2（评审修订）与 v0.3（命名）。
- 当前阶段：**纯设计阶段，尚无任何代码**。

---

## 已做决定

### D1 暂不购买 Apple Developer ID（2026-10-17）

- **决定：** 项目成型后再考虑购买。
- **影响：** 未签名应用的辅助功能（TCC）授权与签名身份绑定，ad-hoc 签名每次构建都会变化，导致每次更新都要重新授权。
- **对策（已写入 PRD 12.2）：**
  - 用一个固定的自签名代码签名证书签名所有发布版本。
  - 提供不需要任何权限的"只复制"降级模式（PRD 7.2）。
  - 文档如实说明 Gatekeeper "仍要打开"的步骤。
  - 将来迁移到 Developer ID 时，用户需要重新授权一次。

### D2 Cloudflare 端到端加密同步保留在 P1（2026-10-17）

- **评审意见：** 建议推迟到 P2，P0/P1 先用纯文件 + Git / 网盘同步，因为 E2EE 客户端（密钥层级、配对、轮换、冲突）工作量大。
- **负责人决定：** 保留。理由是团队已有多个 Cloudflare 项目，服务端可以改造复用。
- **评审补充（已写入 PRD 9.2）：** 服务端确实可以复用，但主要工作量在客户端。因此 P1 拆成"同步服务端"和"E2EE 客户端"两个工作项分别估算。
- **可复用来源：** `nekro-nxt-community` 项目的 `apps/site`（Hono + D1 + Drizzle + React Router，部署在 Workers 上）

  | 文件 / 表 | 可复用为 |
  |---|---|
  | `server/oauth.ts`（PKCE 授权码流程）、`oauth_codes` 表 | 桌面端通过浏览器回调登录 |
  | `instance_grants` 表（access/refresh 令牌哈希轮换、`revoked_at`） | 设备授权与撤销 |
  | `wrangler.jsonc` 中的 `WRITE_LIMITER`、`audit_log` 表 | 限流、审计 |
  | `migrations/` + Drizzle | 迁移体系 |

- **禁止复用：** `server/crypto.ts`。它用服务端环境变量 `CONFIG_ENCRYPTION_KEY` 派生密钥加解密，服务端能解密，违反 E2EE 要求。
- 其他 Cloudflare 参考项目：`nekro-cloud`、`nekro-endpoint`（同一套 Hono/D1/Drizzle 技术栈）。

### D3 AI 配置由用户自行配置，可选加密同步（2026-10-17）

- **评审意见：** 建议不内置 AI 客户端，改为提供 CLI / MCP server，让用户已有的 AI 编程代理直接编辑模板。
- **负责人决定：** 保留内置 AI 编辑。配置由用户自己填写，并可选择和其他数据一起加密同步。
- **落地（PRD 6.3 / 6.4 / 9.5 / 9.7）：**
  - 每个供应商配置单独设置同步开关，默认关闭。
  - API Key 在本机存入系统安全存储；同步时走 E2EE。
  - Base URL 支持"同步默认值 + 本机覆盖"。
  - 撤销设备时，提示用户到供应商处作废已同步的 Key。
  - 只实现 OpenAI 兼容和 Anthropic 两种协议。
- CLI / MCP 作为补充，仍列在待决事项中。

### D4 产品定名 Nekro Qantrip（2026-10-17）

- **仓库：** `NekroAI/nekro-qantrip`
- **由来：** Q + cantrip（D&D 中不耗资源、可随手施放的"戏法"）。Q 呼应唤起快捷键，Prompt 在中文圈常被称为"咒语"。
- **考虑过的其他名字：**
  - Cantrip / Incant / Quickcast（不加 Nekro 前缀）
  - Nekro Grimoire / Scroll / Rune / Chant / Pounce
  - Nekro Quoth（备选第二）、Nekro Qast
  - 否决：Quiver（已有 3.6k★ 项目和同名 Mac 应用）、Summon / Mantra / Sigil（过于常见）、PromptKey（已被竞品使用）
- **检索结果（2026-10-17）：** GitHub 没有 `qantrip` 仓库，Homebrew cask / formula 未被占用。

### D5 默认快捷键方向：Q 系（2026-10-17，具体组合待验证）

- **负责人意向：** 结合 Ctrl+Q 设计，与产品名形成联动。
- **评审意见：** Windows / Linux 上 `Ctrl+Q` 在浏览器和多数 GTK/Qt 应用中是"退出"，热键注册失败时会直接关掉用户的应用。
- **当前方案（PRD 3.1）：** macOS 用 `⌃Q`，Windows / Linux 用 `Alt+Q`；不用 `Ctrl+Alt+Q`（等同 AltGr，部分布局用它输入 `@`）。用户可以改回 `Ctrl+Q`，此时提示风险。
- **待办：** 原型阶段做三平台冲突实测后定稿。**负责人尚未明确确认这一方案。**

### D6 技术路线：各平台原生外壳 + 共享 Rust 核心，不以 Tauri 为主框架（2026-10-17）

- **负责人决定：** 以体验为导向，允许各平台原生实现；负责人不倾向 Tauri。
- **架构：** Rust 核心（输入语法、渲染、存储，后续同步与加密）经 UniFFI 暴露给各平台外壳。macOS 外壳用 Swift（AppKit + SwiftUI），SwiftPM 构建、脚本组装 `.app`，不依赖完整 Xcode。Windows / Linux 外壳技术待定（候选：C# + uniffi-bindgen-cs、windows-rs、GTK4）。
- **评审意见：** Tauri 对本项目的主要价值只是三平台共用管理类界面（模板库、编辑器、设置）。如果日后三份原生管理界面成本过高，可以在原生窗口中嵌入 WebView 只承载这些页面，届时再议。
- 取代 PRD 10.2 中"Tauri 2 作为重点候选"的表述。

### D7 Demo 范围：先只做 macOS，分两阶段（2026-10-17）

- **阶段 1 可行性 Spike：** 验证项 S1–S10（热键、NSPanel、中文输入法、延迟、剪贴板保护、Maccy 排除、无权限降级、目标应用插入、UniFFI 构建、资源占用）。产出可行性报告，作为进入阶段 2 的关卡，须经负责人确认。
- **阶段 2 P0 纵向切片：** 语法解析器、MiniJinja 渲染 + 参数 schema、3–5 个内置模板、面板补全，本地存储暂定"一个模板一个文件"。
- Windows / Linux 在 Spike 通过后再做。
- 推进方式：以 Claude 自主推进为主，需要人工的验证集中成批交给负责人。

### D8 开发期使用本地自签名证书，发布签名延后（2026-10-17）

- **负责人决定：** 签名往后放，优先跑通功能和核心交互体验。
- **例外：** 为避免每次重新构建都丢失辅助功能授权，开发期使用本机自签名证书 `Qantrip Dev`。它只用于本地开发，不是发布签名，同时可以顺带提前验证 D1 的授权持久性。

### D9 面板输入法策略：唤起时切到英文（2026-10-17）

- **问题：** 中文输入法状态下唤起面板，命令词会进入拼音组字，必须删掉、切换输入法后重输。
- **负责人决定：** 打开面板时自动切到英文（ASCII）输入源，关闭时恢复原输入法；输入中文附加文本时由用户手动切换。设置中可关闭此行为。
- **否决：** 进入附加文本区时自动切回中文（无法提前判断用户要输入参数还是附加文本，误判代价高）；保留中文输入法并识别拼音字母（空格会被输入法当作选词）。

### D10 阶段 1 Spike 通过，进入阶段 2（2026-10-17）

- 报告见 `docs/spike-report.md`。macOS 技术选型确定为 D6 路线。
- **负责人反馈：** 手感尚可，但界面"简陋，和传统同类软件几乎一样，没有惊艳感"。阶段 2 直接在原生应用里迭代界面，不先做 HTML 原型。
- **Claude Code 折叠长粘贴**（`[Pasted text #1 +7 lines]`）记为已知限制，不改用逐字键入；以插入前预览解决"看不出插入了什么"。

---

## 待决事项

与 PRD 第 19 章一致：

1. 本地存储是否采用"一个模板一个纯文本文件"（Markdown + YAML frontmatter）。
2. 是否先做 1–2 周插入链路原型，再完成全部设计交付物。
3. 是否提供 CLI / MCP server。
4. 首批内置模板数量（建议 5–8 个，附对比验证记录）。
5. 默认快捷键最终取值（见 D5）。
6. 开源许可证。

---

## 未验证的假设（原型阶段必须实测）

| 假设 | 出处 | 验证方式 |
|---|---|---|
| 用固定自签名证书签名后，应用更新不会丢失辅助功能授权 | PRD 12.2 | 授权 → 用同一证书签名的新版本覆盖安装 → 检查授权 |
| 自有 Homebrew Tap 安装未公证应用的行为（`--no-quarantine` 已弃用） | PRD 12.2 | 用当前版本的 Homebrew 实测安装与升级 |
| Tauri 预创建隐藏窗口能达到约 150ms 的唤起目标 | PRD 14.1 | 三平台实测 |
| macOS non-activating `NSPanel` 能在 Tauri 中实现，且不打断原应用焦点 | PRD 3.2 | 原型 |
| 剪贴板临时标记能阻止主流剪贴板管理器记录 | PRD 7.2 | macOS（Raycast、Maccy、Paste）、Windows 剪贴板历史实测 |
| 粘贴到 Claude Code / Codex CLI 的多行文本不会被提交 | PRD 17.1 | 各终端实测 |
| `⌃Q` / `Alt+Q` 冲突少 | PRD 3.1 | 三平台常用软件冲突测试 |
| Wayland 下 GlobalShortcuts portal 可用性 | PRD 10.3 | GNOME / KDE 当前版本实测 |

---

## 评审中提出且已写入 PRD 的要点（v0.2）

- 输入语法独立成规范，定义位置参数、附加文本、全角规范化、匹配分层和 Enter 语义（`docs/input-grammar.md`）。
- 插入用"剪贴板 + 粘贴"，不逐字键入；剪贴板备份、临时标记、恢复；Windows UIPI 降级；无权限降级模式。
- 模板渲染用成熟引擎（MiniJinja 类）+ 声明式参数 schema，不自研 DSL，渲染在沙箱中执行。
- E2EE：服务端记录 ID 随机生成，元数据全部加密；用成熟密码库；采用 Vault Key / 设备密钥对 / 恢复密钥三层；配对防中间人；提供不依赖 OAuth 的自托管认证。
- 冲突按单条记录检测并保留双方版本，首版不引入 CRDT。
- Linux：首版承诺 X11 及 KDE / GNOME 下"热键 + 复制"，Wayland 自动粘贴标注为实验性。
- 竞品：Espanso（GPL-3.0，注意许可证）、Raycast / Alfred、Claude Code slash commands、PromptKey。
