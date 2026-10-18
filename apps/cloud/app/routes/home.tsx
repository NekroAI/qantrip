import { Link } from 'react-router'
import { DownloadButton } from '../components/DownloadButton'
import { GitHubIcon } from '../components/GitHubIcon'
import { LiveDemo } from '../components/LiveDemo'
import { type Locale, localeOf, SITE_ORIGIN, seo, useHref, useLocale } from '../lib/i18n'
import { REPO_URL } from '../lib/site'
import { groupTemplates, templatesOf } from '../lib/templates'
import type { Route } from './+types/home'

const T = {
  en: {
    title: 'Nekro Qantrip — open-source prompt launcher for Claude Code, Codex and any app',
    description:
      'Press ⌃Q, type a short command, and a full prompt lands at your cursor. Parameterized prompt templates for Claude Code, Codex, Cursor and any text field. Open source, works offline, end-to-end encrypted sync.',
    tags: ['Open source', 'macOS · Windows · Linux', 'Works offline'],
    h1: 'Your best prompts, a few keystrokes away',
    lead: (
      <>
        Press <kbd>⌃Q</kbd>, type a template and a few options, hit <kbd>↩</kbd>. The full prompt appears right at your cursor, in Claude Code, Codex, Cursor, your browser, or any text field.
      </>
    ),
    github: 'View on GitHub',
    steps: [
      { keys: ['⌃', 'Q'], title: 'Summon', body: 'From any app.' },
      { keys: ['r sec -s'], title: 'Type', body: 'A template, a few options.' },
      { keys: ['↩'], title: 'Insert', body: 'The prompt lands at the cursor.' },
    ],
    templatesH: (n: number) => `${n} built-in templates`,
    templatesP: 'Each one is a Markdown file you can change, or use as a starting point for your own.',
    browse: 'Browse and edit them live',
    syncH: 'Sync across devices, end-to-end encrypted',
    syncP: "Even on our public sync service, we can't read your templates.",
    syncLink: 'See how',
    ossH: 'Open source',
    ossP: 'The app, the core and the sync server are all on GitHub. Run your own server if you prefer.',
    issues: 'Report an issue',
    faqH: 'Questions',
    faq: [
      ['What is Qantrip?', 'An open-source prompt launcher. You write your prompts as templates with parameters, then call them up from any app with a global hotkey and a short command.'],
      ['Which apps does it work with?', 'Anything you can paste into: Claude Code, Codex CLI, Cursor, ChatGPT or Claude in the browser, Slack, your editor.'],
      ['Is it free?', 'Yes. The app is GPL-3.0 and the sync server is AGPL-3.0.'],
      ['Which systems are supported?', 'macOS 14 or later (Apple silicon and Intel) and Windows 10/11 x64. Linux (x86_64 and ARM64; GNOME, KDE Plasma and X11 desktops) is available as a preview.'],
      ['Does it need special permissions?', 'On macOS, auto-paste needs Accessibility access; Windows needs none; on Linux (Wayland) you allow it once. Without it, Qantrip copies the prompt and you paste it yourself.'],
      ['Where are my templates stored?', 'On your computer, as plain Markdown files. Sync is optional and end-to-end encrypted.'],
    ] as [string, string][],
  },
  zh: {
    title: 'Nekro Qantrip — 开源的 Prompt 启动器，适用于 Claude Code、Codex 与任何应用',
    description: '按下 ⌃Q，输入一条短命令，完整的 Prompt 就出现在光标处。为 Claude Code、Codex、Cursor 和任何输入框准备的参数化 Prompt 模板。开源、离线可用、端到端加密同步。',
    tags: ['开源', 'macOS · Windows · Linux', '离线可用'],
    h1: '常用的 Prompt，敲几个键就到',
    lead: (
      <>
        按 <kbd>⌃Q</kbd>，输入模板名和参数，回车。完整的 Prompt 就出现在光标处，不管你在用 Claude Code、Codex、Cursor、浏览器还是任何输入框。
      </>
    ),
    github: '在 GitHub 上查看',
    steps: [
      { keys: ['⌃', 'Q'], title: '唤出', body: '在任何应用里。' },
      { keys: ['r sec -s'], title: '输入', body: '模板名和几个参数。' },
      { keys: ['↩'], title: '插入', body: 'Prompt 出现在光标处。' },
    ],
    templatesH: (n: number) => `${n} 个内置模板`,
    templatesP: '每个模板都是一个 Markdown 文件，可以直接改，也可以照着写自己的。',
    browse: '在线浏览和编辑',
    syncH: '多设备同步，端到端加密',
    syncP: '即使使用我们的公共同步服务，我们也无法读取你的模板。',
    syncLink: '看看是怎么做到的',
    ossH: '开源',
    ossP: '应用、核心和同步服务端都在 GitHub 上。也可以部署自己的服务端。',
    issues: '问题反馈',
    faqH: '常见问题',
    faq: [
      ['Qantrip 是什么？', '一个开源的 Prompt 启动器。把常用的 Prompt 写成带参数的模板，在任何应用里用全局快捷键和一条短命令调出来。'],
      ['能在哪些应用里用？', '任何能粘贴文字的地方：Claude Code、Codex CLI、Cursor、网页版 ChatGPT 或 Claude、Slack、编辑器。'],
      ['收费吗？', '免费。应用以 GPL-3.0 开源，同步服务端以 AGPL-3.0 开源。'],
      ['支持哪些系统？', 'macOS 14 及更新版本（Apple 芯片和 Intel 芯片）与 Windows 10/11 x64；Linux（x86_64、ARM64；GNOME、KDE Plasma 与 X11 桌面）提供预览版。'],
      ['需要什么权限？', 'macOS 上自动粘贴需要「辅助功能」权限；Windows 不需要；Linux（Wayland）上授权一次即可。不授权也能用：Prompt 会复制到剪贴板，由你手动粘贴。'],
      ['模板存在哪里？', '存在你的电脑上，是普通的 Markdown 文件。同步是可选的，并且端到端加密。'],
    ] as [string, string][],
  },
} satisfies Record<Locale, unknown>

export const meta: Route.MetaFunction = ({ location }) => {
  const locale = localeOf(location.pathname)
  const t = T[locale]
  const app = {
    '@context': 'https://schema.org',
    '@type': 'SoftwareApplication',
    name: 'Nekro Qantrip',
    alternateName: 'Qantrip',
    applicationCategory: 'DeveloperApplication',
    operatingSystem: 'macOS 14+, Windows 10/11, Linux',
    description: t.description,
    url: SITE_ORIGIN + (locale === 'zh' ? '/zh' : '/'),
    downloadUrl: `${REPO_URL}/releases/latest`,
    license: 'https://www.gnu.org/licenses/gpl-3.0.html',
    isAccessibleForFree: true,
    offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
    codeRepository: REPO_URL,
    image: `${SITE_ORIGIN}/media/icon-256.png`,
    author: { '@type': 'Organization', name: 'NekroAI', url: 'https://github.com/NekroAI' },
    inLanguage: locale === 'zh' ? 'zh-CN' : 'en',
  }
  const faq = {
    '@context': 'https://schema.org',
    '@type': 'FAQPage',
    mainEntity: t.faq.map(([q, a]) => ({ '@type': 'Question', name: q, acceptedAnswer: { '@type': 'Answer', text: a } })),
  }
  return [...seo({ locale, path: '/', title: t.title, description: t.description }), { 'script:ld+json': app }, { 'script:ld+json': faq }]
}

export default function Home() {
  const locale = useLocale()
  const t = T[locale]
  const href = useHref()
  const templates = templatesOf(locale)

  return (
    <>
      <section className="wrap hero">
        <div className="hero-meta">
          {t.tags.map((x) => (
            <span key={x}>{x}</span>
          ))}
        </div>
        <h1>{t.h1}</h1>
        <p>{t.lead}</p>
        <div className="hero-actions">
          <DownloadButton />
          <a className="btn" href={REPO_URL}>
            <GitHubIcon /> {t.github}
          </a>
        </div>
      </section>

      <section className="wrap" id="demo">
        <LiveDemo />
      </section>

      <section className="wrap section">
        <ol className="steps">
          {t.steps.map((s, i) => (
            <li key={s.title}>
              <span className="step-n">{i + 1}</span>
              <span className="step-keys">
                {s.keys.map((k) => (
                  <kbd key={k}>{k}</kbd>
                ))}
              </span>
              <b>{s.title}</b>
              <span className="muted">{s.body}</span>
            </li>
          ))}
        </ol>
      </section>

      <section className="wrap section">
        <header>
          <h2>{t.templatesH(templates.length)}</h2>
          <p>{t.templatesP}</p>
        </header>
        <div className="tgroups">
          {groupTemplates(templates).map((g) => (
            <div className="tgroup" key={g.category}>
              <h3>{g.category}</h3>
              <ul>
                {g.items.map((x) => (
                  <li key={x.name}>
                    <Link to={`${href('/docs')}#t-${x.name}`}>
                      <code>{x.name}</code>
                      {x.aliases.length > 0 && <span className="alias">{x.aliases.join(' ')}</span>}
                    </Link>
                    <span>{x.description}</span>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
        <p>
          <Link to={`${href('/docs')}#templates`}>{t.browse} →</Link>
        </p>
      </section>

      <section className="wrap section split">
        <div>
          <h2>{t.syncH}</h2>
          <p>{t.syncP}</p>
          <Link className="btn" to={href('/security')}>
            {t.syncLink} →
          </Link>
        </div>
        <div>
          <h2>{t.ossH}</h2>
          <p>{t.ossP}</p>
          <div className="hero-actions">
            <a className="btn" href={REPO_URL}>
              <GitHubIcon /> GitHub
            </a>
            <a className="btn" href={`${REPO_URL}/issues`}>
              {t.issues}
            </a>
          </div>
        </div>
      </section>

      <section className="wrap section">
        <header>
          <h2>{t.faqH}</h2>
        </header>
        <dl className="faq">
          {t.faq.map(([q, a]) => (
            <div key={q}>
              <dt>{q}</dt>
              <dd>{a}</dd>
            </div>
          ))}
        </dl>
      </section>
    </>
  )
}
