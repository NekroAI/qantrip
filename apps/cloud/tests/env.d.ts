// 测试环境额外绑定：由 vitest.config.ts 读取 migrations 目录后注入。
import type { D1Migration } from '@cloudflare/vitest-pool-workers'

declare global {
  namespace Cloudflare {
    interface Env {
      TEST_MIGRATIONS: D1Migration[]
    }
  }
}
