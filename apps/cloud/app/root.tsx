import { isRouteErrorResponse, Link, Links, Meta, NavLink, Outlet, Scripts, ScrollRestoration } from 'react-router'
import type { Route } from './+types/root'
import './app.css'
import { GitHubIcon } from './components/GitHubIcon'
import { REPO_URL } from './lib/site'

export const links: Route.LinksFunction = () => [
  { rel: 'icon', href: '/favicon.png', type: 'image/png' },
  { rel: 'apple-touch-icon', href: '/apple-touch-icon.png' },
  { rel: 'preload', href: '/fonts/MonaspaceNeon-latin.woff2', as: 'font', type: 'font/woff2', crossOrigin: 'anonymous' },
]

const NAV = [
  { to: '/docs', label: '文档' },
  { to: '/security', label: '安全' },
  { to: '/download', label: '下载' },
  { to: '/account', label: '账号' },
] as const

export function Layout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="zh-CN">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <meta name="theme-color" content="#0b0b0c" />
        <Meta />
        <Links />
      </head>
      <body>
        <header className="nav">
          <div className="wrap">
            <Link to="/" className="brand" aria-label="Nekro Qantrip 首页">
              <img src="/media/icon-256.png" alt="" width={24} height={24} />
              Qantrip
            </Link>
            <nav className="nav-links" aria-label="主导航">
              {NAV.map((n) => (
                <NavLink key={n.to} to={n.to}>
                  {n.label}
                </NavLink>
              ))}
              <a className="gh" href={REPO_URL} rel="noopener">
                <GitHubIcon /> GitHub
              </a>
            </nav>
          </div>
        </header>
        <main>{children}</main>
        <footer className="footer">
          <div className="wrap">
            <div>
              Nekro Qantrip，由 NekroAI 开发的开源项目。
              <br />
              客户端 GPL-3.0-or-later，服务端 AGPL-3.0-or-later。
            </div>
            <nav aria-label="页脚">
              <a href={REPO_URL}>源代码</a>
              <a href={`${REPO_URL}/issues`}>问题反馈</a>
              <Link to="/docs">文档</Link>
              <Link to="/security">安全设计</Link>
              <Link to="/download">下载</Link>
            </nav>
          </div>
        </footer>
        <ScrollRestoration />
        <Scripts />
      </body>
    </html>
  )
}

export default function App() {
  return <Outlet />
}

export function ErrorBoundary({ error }: Route.ErrorBoundaryProps) {
  const title = isRouteErrorResponse(error) ? (error.status === 404 ? '页面不存在' : `出错了（${error.status}）`) : '出错了'
  const detail = isRouteErrorResponse(error) && typeof error.data === 'string' ? error.data : '请稍后重试，或返回首页。'
  return (
    <section className="wrap page">
      <header>
        <h1>{title}</h1>
        <p>{detail}</p>
      </header>
      <p>
        <Link className="btn" to="/">
          返回首页
        </Link>
      </p>
    </section>
  )
}
