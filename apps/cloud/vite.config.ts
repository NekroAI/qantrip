import { reactRouter } from '@react-router/dev/vite'
import { cloudflare } from '@cloudflare/vite-plugin'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [cloudflare({ viteEnvironment: { name: 'ssr' } }), reactRouter()],
  resolve: { tsconfigPaths: true },
  server: { port: 5190, strictPort: true },
  preview: { port: 5190, strictPort: true },
})
