# Qantrip Cloud

Nekro Qantrip 的官网、账号与端到端加密同步服务，运行在一个 Cloudflare Worker 上（docs/decisions.md D23）。

- 官网：React Router（服务端渲染），`app/`
- API：Hono，`server/`；`/api/v1/*`、`/auth/*`、`/download/*`
- 数据：D1（`migrations/`），安装包镜像在 R2
- 安全设计：[`docs/sync-design.md`](../../docs/sync-design.md)。服务端只保存密文，不包含任何解密逻辑。

```sh
pnpm install
pnpm dev                 # 本地开发（http://localhost:5190）
pnpm test                # 契约测试（vitest-pool-workers）
pnpm deploy:staging      # 部署到 qantrip-staging.nekro.ai
pnpm deploy:production   # 部署到 qantrip.nekro.ai
```

GitHub OAuth 凭据通过 `wrangler secret put GITHUB_CLIENT_ID / GITHUB_CLIENT_SECRET [--env staging]` 设置，不进入仓库。

本目录以 [AGPL-3.0-or-later](LICENSE) 授权（docs/decisions.md D20）。
