import { type RouteConfig, index, route } from '@react-router/dev/routes'

export default [
  index('routes/home.tsx'),
  route('download', 'routes/download.tsx'),
  route('docs', 'routes/docs.tsx'),
  route('security', 'routes/security.tsx'),
  route('account', 'routes/account.tsx'),
  route('oauth/authorize', 'routes/oauth-authorize.tsx'),
  route('*', 'routes/not-found.tsx'),
] satisfies RouteConfig
