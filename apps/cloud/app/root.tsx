import { isRouteErrorResponse, Link, Links, Meta, NavLink, Outlet, Scripts, ScrollRestoration, useLocation } from 'react-router'
import type { Route } from './+types/root'
import './app.css'
import { GitHubIcon } from './components/GitHubIcon'
import { detectLocale, localePath, rememberLocale, stripLocale, useCommon, useHref, useLocale, LOCALIZED_PATHS } from './lib/i18n'
import { REPO_URL } from './lib/site'

export const links: Route.LinksFunction = () => [
  { rel: 'icon', href: '/favicon.png', type: 'image/png' },
  { rel: 'apple-touch-icon', href: '/apple-touch-icon.png' },
  { rel: 'preload', href: '/fonts/MonaspaceNeon-latin.woff2', as: 'font', type: 'font/woff2', crossOrigin: 'anonymous' },
]

/** 没有语言前缀的页面（账号、授权）按 Cookie / Accept-Language 选择语言 */
export const loader = ({ request }: Route.LoaderArgs) => ({ detected: detectLocale(request) })

function LangSwitch() {
  const locale = useLocale()
  const t = useCommon()
  const { pathname } = useLocation()
  const other = locale === 'zh' ? 'en' : 'zh'
  const bare = stripLocale(pathname)
  // 只有中英两版的页面才有对应地址；其他页面切换后回到对应语言的首页
  const target = (LOCALIZED_PATHS as readonly string[]).includes(bare) ? localePath(other, bare) : localePath(other, '/')
  return (
    <Link className="lang" to={target} hrefLang={other === 'zh' ? 'zh-Hans' : 'en'} aria-label={t.switchLabel} onClick={() => rememberLocale(other)}>
      {t.switchTo}
    </Link>
  )
}

export function Layout({ children }: { children: React.ReactNode }) {
  const locale = useLocale()
  const t = useCommon()
  const href = useHref()
  return (
    <html lang={locale === 'zh' ? 'zh-CN' : 'en'}>
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
            <Link to={href('/')} className="brand" aria-label={t.home}>
              <img src="/media/icon-256.png" alt="" width={24} height={24} />
              Qantrip
            </Link>
            <nav className="nav-links" aria-label={t.mainNav}>
              <NavLink to={href('/docs')}>{t.nav.docs}</NavLink>
              <NavLink to={href('/security')}>{t.nav.security}</NavLink>
              <NavLink to={href('/download')}>{t.nav.download}</NavLink>
              <NavLink to="/account">{t.nav.account}</NavLink>
              <a className="gh" href={REPO_URL} rel="noopener">
                <GitHubIcon /> GitHub
              </a>
              <LangSwitch />
            </nav>
          </div>
        </header>
        <main>{children}</main>
        <footer className="footer">
          <div className="wrap">
            <div>
              {t.footerAbout.split('NekroAI').map((part, i, arr) => (
                <span key={i}>
                  {part}
                  {i < arr.length - 1 && <a href="https://nekro.ai">NekroAI</a>}
                </span>
              ))}
              <br />
              {t.footerLicense}
            </div>
            <nav aria-label={t.footerNav}>
              <a href={REPO_URL}>{t.source}</a>
              <a href={`${REPO_URL}/issues`}>{t.issues}</a>
              <Link to={href('/docs')}>{t.nav.docs}</Link>
              <Link to={href('/security')}>{t.securityDesign}</Link>
              <Link to={href('/download')}>{t.nav.download}</Link>
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
  const t = useCommon()
  const href = useHref()
  const notFound = isRouteErrorResponse(error) && error.status === 404
  const title = notFound ? t.notFound : isRouteErrorResponse(error) ? `${t.error} (${error.status})` : t.error
  return (
    <section className="wrap page">
      <header>
        <h1>{title}</h1>
        <p>{notFound ? t.notFoundDetail : t.errorDetail}</p>
      </header>
      <p>
        <Link className="btn" to={href('/')}>
          {t.backHome}
        </Link>
      </p>
    </section>
  )
}
