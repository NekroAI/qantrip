// 官网多语言（docs/decisions.md D29）：英文在根路径，中文在 /zh 下；每种语言有独立 URL，便于搜索引擎收录。
// 首次访问根路径时按 Cookie / Accept-Language 自动跳转（workers/app.ts），可在导航栏手动切换。
import { useLocation, useRouteLoaderData } from 'react-router'

export type Locale = 'en' | 'zh'
export const LOCALES: Locale[] = ['en', 'zh']
export const SITE_ORIGIN = 'https://qantrip.nekro.ai'
/** 有中英两个版本的页面（不含语言前缀） */
export const LOCALIZED_PATHS = ['/', '/docs', '/security', '/download'] as const
export const LANG_COOKIE = 'lang'

export const isZhPath = (pathname: string) => pathname === '/zh' || pathname.startsWith('/zh/')
export const stripLocale = (pathname: string) => (isZhPath(pathname) ? pathname.slice(3) || '/' : pathname)
/** 给不含语言前缀的路径加上 `locale` 的前缀 */
export const localePath = (locale: Locale, path: string) => (locale === 'zh' ? (path === '/' ? '/zh' : `/zh${path}`) : path)

/** 从 Cookie 与 Accept-Language 推断语言（用于根路径跳转和没有语言前缀的页面） */
export const detectLocale = (request: Request): Locale => {
  const cookie = request.headers.get('cookie') ?? ''
  const m = /(?:^|;\s*)lang=(en|zh)\b/.exec(cookie)
  if (m) return m[1] as Locale
  const accept = request.headers.get('accept-language') ?? ''
  const first = accept.split(',')[0]?.trim().toLowerCase() ?? ''
  return first.startsWith('zh') ? 'zh' : 'en'
}

export function useLocale(): Locale {
  const { pathname } = useLocation()
  const root = useRouteLoaderData('root') as { detected?: Locale } | undefined
  if (isZhPath(pathname)) return 'zh'
  if ((LOCALIZED_PATHS as readonly string[]).includes(pathname)) return 'en'
  return root?.detected ?? 'en'
}

/** 当前语言下的站内链接 */
export function useHref() {
  const locale = useLocale()
  return (path: string) => localePath(locale, path)
}

export const rememberLocale = (locale: Locale) => {
  try {
    document.cookie = `${LANG_COOKIE}=${locale}; path=/; max-age=31536000; samesite=lax`
  } catch {
    // Cookie 不可用时只影响下次访问的自动跳转
  }
}

/** 页面的 SEO 元信息：标题、描述、规范链接、各语言版本互链、Open Graph 与 Twitter 卡片 */
export function seo({ locale, path, title, description, noindex }: { locale: Locale; path: string; title: string; description: string; noindex?: boolean }) {
  const url = SITE_ORIGIN + localePath(locale, path)
  const image = `${SITE_ORIGIN}/media/og-${locale}.png`
  return [
    { title },
    { name: 'description', content: description },
    ...(noindex ? [{ name: 'robots', content: 'noindex' }] : []),
    { tagName: 'link', rel: 'canonical', href: url },
    { tagName: 'link', rel: 'alternate', hrefLang: 'en', href: SITE_ORIGIN + localePath('en', path) },
    { tagName: 'link', rel: 'alternate', hrefLang: 'zh-Hans', href: SITE_ORIGIN + localePath('zh', path) },
    { tagName: 'link', rel: 'alternate', hrefLang: 'x-default', href: SITE_ORIGIN + localePath('en', path) },
    { property: 'og:type', content: 'website' },
    { property: 'og:site_name', content: 'Nekro Qantrip' },
    { property: 'og:title', content: title },
    { property: 'og:description', content: description },
    { property: 'og:url', content: url },
    { property: 'og:image', content: image },
    { property: 'og:image:width', content: '1200' },
    { property: 'og:image:height', content: '630' },
    { property: 'og:locale', content: locale === 'zh' ? 'zh_CN' : 'en_US' },
    { property: 'og:locale:alternate', content: locale === 'zh' ? 'en_US' : 'zh_CN' },
    { name: 'twitter:card', content: 'summary_large_image' },
    { name: 'twitter:title', content: title },
    { name: 'twitter:description', content: description },
    { name: 'twitter:image', content: image },
  ]
}

/** 路由 meta 函数里取语言（meta 中不能用 hook） */
export const localeOf = (pathname: string): Locale => (isZhPath(pathname) ? 'zh' : 'en')

export const COMMON = {
  en: {
    nav: { docs: 'Docs', security: 'Security', download: 'Download', account: 'Account' },
    home: 'Nekro Qantrip home',
    mainNav: 'Main',
    switchTo: '中文',
    switchLabel: '切换到中文',
    footerAbout: 'Nekro Qantrip is an open-source project by NekroAI.',
    footerLicense: 'App: GPL-3.0-or-later · Server: AGPL-3.0-or-later',
    footerNav: 'Footer',
    source: 'Source code',
    issues: 'Issues',
    securityDesign: 'Security design',
    notFound: 'Page not found',
    notFoundDetail: "This page doesn't exist.",
    error: 'Something went wrong',
    errorDetail: 'Please try again later, or go back home.',
    backHome: 'Back to home',
  },
  zh: {
    nav: { docs: '文档', security: '安全', download: '下载', account: '账号' },
    home: 'Nekro Qantrip 首页',
    mainNav: '主导航',
    switchTo: 'English',
    switchLabel: 'Switch to English',
    footerAbout: 'Nekro Qantrip，由 NekroAI 开发的开源项目。',
    footerLicense: '客户端 GPL-3.0-or-later · 服务端 AGPL-3.0-or-later',
    footerNav: '页脚',
    source: '源代码',
    issues: '问题反馈',
    securityDesign: '安全设计',
    notFound: '页面不存在',
    notFoundDetail: '这个页面不存在。',
    error: '出错了',
    errorDetail: '请稍后重试，或返回首页。',
    backHome: '返回首页',
  },
} satisfies Record<Locale, unknown>

export const useCommon = () => COMMON[useLocale()]
