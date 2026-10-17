import { createRequestHandler, type ServerBuild } from 'react-router'
import { api, handlesPath } from '../server/api.js'
import { dailyCleanup } from '../server/cleanup.js'

const pageHandler = createRequestHandler(
  () => import('virtual:react-router/server-build') as Promise<unknown> as Promise<ServerBuild>,
  import.meta.env.MODE,
)

const SECURITY_HEADERS: Readonly<Record<string, string>> = {
  'x-content-type-options': 'nosniff',
  'referrer-policy': 'strict-origin-when-cross-origin',
  'x-frame-options': 'DENY',
  'strict-transport-security': 'max-age=31536000; includeSubDomains',
  'permissions-policy': 'camera=(), microphone=(), geolocation=()',
}

const withSecurityHeaders = (response: Response): Response => {
  const headers = new Headers(response.headers)
  for (const [key, value] of Object.entries(SECURITY_HEADERS)) if (!headers.has(key)) headers.set(key, value)
  return new Response(response.body, { status: response.status, statusText: response.statusText, headers })
}

/** 页面表单提交只接受同源请求（会话 Cookie 已是 SameSite=Lax，这里再做一层 Origin 校验）。 */
const crossSiteForm = (request: Request, env: Env): boolean => {
  if (request.method === 'GET' || request.method === 'HEAD') return false
  const origin = request.headers.get('origin')
  return origin !== null && origin !== env.SITE_ORIGIN && !origin.startsWith('http://localhost')
}

export default {
  async fetch(request, env, ctx) {
    const { pathname } = new URL(request.url)
    if (!pathname.startsWith('/api/') && crossSiteForm(request, env)) return new Response('Forbidden', { status: 403 })
    const response = handlesPath(pathname) ? await api.fetch(request, env, ctx) : await pageHandler(request)
    return withSecurityHeaders(response)
  },

  async scheduled(_controller, env) {
    await dailyCleanup(env)
  },
} satisfies ExportedHandler<Env>
