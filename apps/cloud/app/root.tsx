import { isRouteErrorResponse, Link, Links, Meta, NavLink, Outlet, Scripts, ScrollRestoration } from 'react-router'
import type { Route } from './+types/root'
import './app.css'

export const links: Route.LinksFunction = () => [
  { rel: 'icon', href: '/favicon.png', type: 'image/png' },
  { rel: 'apple-touch-icon', href: '/apple-touch-icon.png' },
  { rel: 'preconnect', href: 'https://fonts.googleapis.com' },
  { rel: 'preconnect', href: 'https://fonts.gstatic.com', crossOrigin: 'anonymous' },
  {
    rel: 'stylesheet',
    href: 'https://fonts.googleapis.com/css2?family=Fraunces:ital,opsz,wght,SOFT,WONK@0,9..144,400..700,100,1;1,9..144,400..700,100,1&display=swap',
  },
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
        <meta name="theme-color" content="#0d0a15" />
        <Meta />
        <Links />
      </head>
      <body>
        <header className="nav">
          <div className="wrap">
            <Link to="/" className="brand" aria-label="Nekro Qantrip 首页">
              <img src="/media/icon-256.png" alt="" width={28} height={28} />
              Qantrip
            </Link>
            <nav className="nav-links" aria-label="主导航">
              {NAV.map((n) => (
                <NavLink key={n.to} to={n.to}>
                  {n.label}
                </NavLink>
              ))}
            </nav>
          </div>
        </header>
        <main>{children}</main>
        <footer className="footer">
          <div className="wrap">
            <div>
              Nekro Qantrip · 按 Q 施放一个戏法
              <br />
              客户端 GPL-3.0-or-later · 服务端 AGPL-3.0-or-later · NekroAI
            </div>
            <nav aria-label="页脚">
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
      <h1 className="display">{title}</h1>
      <p className="lead">{detail}</p>
      <p>
        <Link className="btn" to="/">
          返回首页
        </Link>
      </p>
    </section>
  )
}
