// 搜索引擎与 AI 检索（docs/decisions.md D29）：robots.txt、sitemap.xml、llms.txt / llms-full.txt，
// 以及首次访问时按语言跳转。staging 环境一律不允许收录。
import templates from '../app/lib/templates.json'

const PAGES = ['/', '/docs', '/security', '/download'] as const
const REPO = 'https://github.com/NekroAI/qantrip'

const zhPath = (p: string) => (p === '/' ? '/zh' : `/zh${p}`)
const production = (env: Env) => env.QANTRIP_ENV === 'production'

const text = (body: string, type = 'text/plain; charset=utf-8') =>
  new Response(body, { headers: { 'content-type': type, 'cache-control': 'public, max-age=3600' } })

const robots = (env: Env) =>
  text(production(env) ? `User-agent: *\nAllow: /\nDisallow: /account\nDisallow: /admin\nDisallow: /oauth/\nDisallow: /auth/\nDisallow: /api/\n\nSitemap: ${env.SITE_ORIGIN}/sitemap.xml\n` : 'User-agent: *\nDisallow: /\n')

const sitemap = (env: Env) => {
  const o = env.SITE_ORIGIN
  const urls = PAGES.flatMap((p) =>
    [p, zhPath(p)].map(
      (loc) => `  <url>
    <loc>${o}${loc}</loc>
    <xhtml:link rel="alternate" hreflang="en" href="${o}${p}"/>
    <xhtml:link rel="alternate" hreflang="zh-Hans" href="${o}${zhPath(p)}"/>
    <xhtml:link rel="alternate" hreflang="x-default" href="${o}${p}"/>
  </url>`,
    ),
  )
  return text(
    `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n${urls.join('\n')}\n</urlset>\n`,
    'application/xml; charset=utf-8',
  )
}

interface Tpl {
  name: string
  aliases: string[]
  description: string
  category: string
  params: { name: string; kind: string; short: string | null; positional: boolean; description: string; choices: [string, string][] }[]
}

const SUMMARY = (o: string) => `# Nekro Qantrip

> Nekro Qantrip is a free, open-source prompt launcher for macOS and Windows, with a Linux preview. Press a global hotkey (⌃Q on macOS, Ctrl+Q on Windows and Linux), type a short command such as \`r security --scope=staged -s\`, and a full, parameterized prompt is inserted at the cursor in any app: Claude Code, Codex CLI, Cursor, ChatGPT or Claude in the browser, Slack, editors. Prompts are Markdown templates with typed options (MiniJinja). It works offline; optional sync is end-to-end encrypted.

- Website: ${o}/ (English), ${o}/zh (中文)
- Source code: ${REPO} (app and core GPL-3.0-or-later, sync server AGPL-3.0-or-later)
- Download (macOS 14+): ${REPO}/releases/latest/download/Qantrip-macOS-arm64.dmg (Apple silicon), ${REPO}/releases/latest/download/Qantrip-macOS-x86_64.dmg (Intel)
- Download (Windows 10/11 x64): ${REPO}/releases/latest/download/Qantrip-Windows-x64-Setup.exe (installer, no admin rights) or Qantrip-Windows-x64.zip (portable); shortcut Ctrl+Q
- Download (Linux x86_64 / ARM64, preview): ${REPO}/releases/latest/download/Qantrip-Linux-x86_64.AppImage or Qantrip-Linux-aarch64.AppImage; also .deb and .rpm (Qantrip-Linux-<arch>.deb / .rpm); needs GTK 4.14+ and libadwaita 1.5+; shortcut Ctrl+Q
- Platforms: macOS, Windows, Linux (preview).
- Made by NekroAI (https://github.com/NekroAI). Pronounced like "cantrip".

## Docs

- [Docs](${o}/docs): install, command syntax, shortcuts, built-in templates, writing templates, with a live template editor
- [Security](${o}/security): how end-to-end encrypted sync works; the server stores only ciphertext
- [Download](${o}/download): direct DMG links
- [Full text for LLMs](${o}/llms-full.txt)
- [Input grammar spec](${REPO}/blob/main/docs/input-grammar.md)
- [Sync security design](${REPO}/blob/main/docs/sync-design.md)

## Key facts

- How it inserts: clipboard + paste, then the previous clipboard is restored; the prompt is marked transient so clipboard managers skip it. Without Accessibility permission it falls back to copy-only.
- Claude Code: pastes over 800 characters or 3 lines are folded into "[Pasted text]"; ⌘↩ pastes twice so Claude Code expands it.
- Templates live in ~/Library/Application Support/Qantrip/templates/*.md. A file named like a built-in template overrides it.
- Interface and built-in templates are available in English and Chinese, set separately.
`

const fullText = (env: Env) => {
  const list = (templates as unknown as { en: Tpl[] }).en
  const tpl = list
    .map((t) => {
      const params = t.params
        .map((p) => {
          const flags = [`--${p.name}`, p.short ? `-${p.short}` : '', p.positional ? '(positional)' : ''].filter(Boolean).join(' ')
          const choices = p.choices.length ? `: ${p.choices.map(([v]) => v).join(', ')}` : ''
          return `  - ${flags} [${p.kind}]${p.description ? ` ${p.description}` : ''}${choices}`
        })
        .join('\n')
      return `- \`${t.name}\`${t.aliases.length ? ` (alias ${t.aliases.join(', ')})` : ''}: ${t.description} [${t.category}]${params ? `\n${params}` : ''}`
    })
    .join('\n')
  return `${SUMMARY(env.SITE_ORIGIN)}
## Command syntax

\`\`\`text
r security --scope=staged -s focus on the auth module
│ │        │              │  └ free text, appended to the prompt as is
│ │        │              └ short flag (-s = --strict)
│ │        └ option with a value (prefixes work: --sc=st; multiple values comma separated)
│ └ positional value for the template's main option
└ template name or alias (exact, prefix or fuzzy; Enter inserts what the preview shows, Tab completes)
\`\`\`

Shortcuts: ⌃Q summon · Tab accept suggestion · → accept inline hint · ↑↓ choose · Enter insert · ⌘Enter insert expanded in Claude Code · \`--\` list all options · Esc close.

## Template format

\`\`\`markdown
---
name: review
aliases: [r]
description: Code review
params:
  focus: { type: multi, positional: true, choices: [security, performance] }
  scope: { type: enum, default: changes, choices: [changes, staged, branch] }
  strict: { type: bool, short: s, description: Report every doubt }
---
Review the {% if scope == "staged" %}staged {% endif %}changes.
{% if strict %}List anything uncertain, with a confidence level.{% endif %}
{% include "_extra" %}
\`\`\`

Option types: bool, enum, multi, string, number, text. Fields: default, short, positional, required, requires, conflicts, choices (value / label / aliases), min, max. In the body, "-" in option names becomes "_", and the free text is \`extra\`.

## Built-in templates

${tpl}

## Sync and security

Sync is optional. Each device generates its own keys; the vault key never leaves your devices in plaintext. Records are encrypted with XChaCha20-Poly1305 before upload. A new device gets the key only after you type the code it shows into a device you already use. A 24-word recovery phrase restores access if every device is lost. The server (Cloudflare Workers + D1, open source) stores only ciphertext and cannot decrypt it, even on the public service at ${env.SITE_ORIGIN}. You can deploy the server yourself.
`
}

export const seoResponse = (pathname: string, env: Env): Response | null => {
  switch (pathname) {
    case '/robots.txt':
      return robots(env)
    case '/sitemap.xml':
      return sitemap(env)
    case '/llms.txt':
      return text(SUMMARY(env.SITE_ORIGIN), 'text/markdown; charset=utf-8')
    case '/llms-full.txt':
      return text(fullText(env), 'text/markdown; charset=utf-8')
    default:
      return null
  }
}

/** 首次访问英文页面时，按 Cookie / Accept-Language 跳到中文版；手动切换过语言（Cookie）则以 Cookie 为准 */
export const localeRedirect = (request: Request): Response | null => {
  if (request.method !== 'GET') return null
  const url = new URL(request.url)
  if (!(PAGES as readonly string[]).includes(url.pathname)) return null
  const cookie = request.headers.get('cookie') ?? ''
  const chosen = /(?:^|;\s*)lang=(en|zh)\b/.exec(cookie)?.[1]
  const accept = (request.headers.get('accept-language') ?? '').split(',')[0]?.trim().toLowerCase() ?? ''
  const wantsZh = chosen ? chosen === 'zh' : accept.startsWith('zh')
  if (!wantsZh) return null
  return new Response(null, { status: 302, headers: { location: zhPath(url.pathname) + url.search, vary: 'accept-language, cookie', 'cache-control': 'private, no-store' } })
}

export const noindex = (env: Env) => !production(env)
