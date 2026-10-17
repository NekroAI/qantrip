import { cloudflareTest, readD1Migrations } from '@cloudflare/vitest-pool-workers'
import path from 'node:path'
import { defineProject } from 'vitest/config'

export default defineProject(async () => {
  const migrations = await readD1Migrations(path.join(import.meta.dirname, 'migrations'))
  return {
    plugins: [
      cloudflareTest({
        wrangler: { configPath: './wrangler.jsonc' },
        miniflare: {
          bindings: {
            TEST_MIGRATIONS: migrations,
            SITE_ORIGIN: 'https://qantrip.test',
            QANTRIP_ENV: 'test',
            PUBLIC_DOWNLOADS: '0',
            GITHUB_CLIENT_ID: '',
            GITHUB_CLIENT_SECRET: '',
          },
        },
      }),
    ],
    test: { name: 'cloud', include: ['tests/**/*.spec.ts'], setupFiles: ['./tests/setup.ts'] },
  }
})
