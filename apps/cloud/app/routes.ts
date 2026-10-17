import { type RouteConfig, index, route } from '@react-router/dev/routes'

// 公开页面各有英文（根路径）与中文（/zh）两个版本，共用同一个组件（app/lib/i18n.ts）
const pages = (prefix: string, id: string) => [
  route(`${prefix}docs`, 'routes/docs.tsx', { id: `docs-${id}` }),
  route(`${prefix}security`, 'routes/security.tsx', { id: `security-${id}` }),
  route(`${prefix}download`, 'routes/download.tsx', { id: `download-${id}` }),
]

export default [
  index('routes/home.tsx', { id: 'home-en' }),
  route('zh', 'routes/home.tsx', { id: 'home-zh' }),
  ...pages('', 'en'),
  ...pages('zh/', 'zh'),
  route('account', 'routes/account.tsx'),
  route('oauth/authorize', 'routes/oauth-authorize.tsx'),
  route('*', 'routes/not-found.tsx'),
] satisfies RouteConfig
