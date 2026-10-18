import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { type Analysis, Highlighted, Preview } from '../components/LiveDemo'
import { type Core, loadCore } from '../lib/core'
import { type Locale, localeOf, seo, useHref, useLocale } from '../lib/i18n'
import { type BuiltinTemplate, groupTemplates, templatesOf } from '../lib/templates'
import type { Route } from './+types/docs'

// 文档页（docs/decisions.md D29）：少写字，多用可以操作的界面。模板编辑器运行真实核心（WebAssembly）。

const T = {
  en: {
    title: 'Docs — commands, shortcuts and templates · Nekro Qantrip',
    description: 'How to install Qantrip on macOS, the command syntax, keyboard shortcuts, all built-in prompt templates, and how to write your own template in Markdown + MiniJinja, with a live editor.',
    h1: 'Docs',
    lead: 'Everything you need, mostly by trying it.',
    toc: [
      ['install', 'Install'],
      ['command', 'Commands'],
      ['keys', 'Shortcuts'],
      ['templates', 'Templates'],
      ['write', 'Write your own'],
      ['reference', 'Reference'],
    ],
    install: [
      { h: 'Download', p: 'Get the DMG for your Mac.', link: 'Downloads' },
      { h: 'Drag to Applications', p: 'Open the DMG and drag Qantrip into Applications.' },
      { h: 'Open it once', p: 'Qantrip is not notarized yet. Go to System Settings → Privacy & Security → Open Anyway.' },
      { h: 'Allow Accessibility', p: 'Optional. It lets Qantrip paste for you; without it, prompts are copied instead.' },
    ],
    commandH: 'Anatomy of a command',
    commandP: 'Click a part.',
    parts: [
      ['r', 't-command', 'Template name or alias. Prefixes and typos are fine: ↩ inserts what the preview shows, ⇥ completes the name.'],
      ['security', 't-positional', 'Positional value: type a choice directly, without the option name.'],
      ['--scope=staged', 't-optionname', 'Option with a value. Both name and value can be shortened: --sc=st. Separate multiple values with commas.'],
      ['-s', 't-shortflag', 'Short flag for a switch. -s is --strict.'],
      ['focus on the auth module', '', 'Free text. From the first word that is not an option, the rest is appended to the prompt as is.'],
    ] as [string, string, string][],
    keysH: 'Shortcuts',
    keys: [
      [['⌃', 'Q'], 'Summon the panel (configurable)'],
      [['⇥'], 'Accept the suggestion'],
      [['→'], 'Accept the grey inline hint'],
      [['↑', '↓'], 'Choose a suggestion'],
      [['↩'], 'Insert the prompt'],
      [['⌘', '↩'], 'Insert expanded in Claude Code'],
      [['--'], 'List every option'],
      [['esc'], 'Close'],
    ] as [string[], string][],
    templatesH: 'Built-in templates',
    params: 'Options',
    none: 'No options. Anything you type after the name is appended.',
    source: 'Source',
    positional: 'positional',
    writeH: 'Write your own',
    writeP: (
      <>
        A template is a Markdown file in <code>~/Library/Application Support/Qantrip/templates/</code>. The header declares options; the body is a MiniJinja template. Edit it here: this runs the real Qantrip core in your browser.
      </>
    ),
    tryLabel: 'Try it with',
    problems: 'Problems',
    loading: 'Loading the core…',
    ok: 'Valid',
    referenceH: 'Reference',
    typesH: 'Option types',
    types: [
      ['bool', 'A switch: --strict, --no-strict, or a short flag'],
      ['enum', 'One of the choices'],
      ['multi', 'Several choices, comma separated'],
      ['string', 'One line of text'],
      ['number', 'A number, with optional min / max'],
      ['text', 'Multi-line text'],
    ],
    fieldsH: 'Option fields',
    fields: [
      ['default', 'Value when the option is not given'],
      ['short', 'One-letter flag (bool only)'],
      ['positional', 'Accept the value without the option name (one per template)'],
      ['required', 'Must be given before inserting'],
      ['requires / conflicts', 'Other options that must / must not be set together'],
      ['choices', 'Values for enum / multi; each can have a label and aliases'],
    ],
    notes: [
      <>
        In the body, <code>-</code> in an option name becomes <code>_</code>, and the free text is <code>extra</code>.
      </>,
      'A file with the same name as a built-in template replaces it.',
      'Qantrip reloads the folder automatically.',
    ],
  },
  zh: {
    title: '文档 — 命令、快捷键与模板 · Nekro Qantrip',
    description: '在 macOS 上安装 Qantrip、命令语法、快捷键、全部内置 Prompt 模板，以及如何用 Markdown + MiniJinja 编写自己的模板（带在线编辑器）。',
    h1: '文档',
    lead: '大部分内容，动手试一下就明白。',
    toc: [
      ['install', '安装'],
      ['command', '命令'],
      ['keys', '快捷键'],
      ['templates', '模板'],
      ['write', '编写模板'],
      ['reference', '参考'],
    ],
    install: [
      { h: '下载', p: '下载对应芯片的 DMG。', link: '下载页' },
      { h: '拖进「应用程序」', p: '打开 DMG，把 Qantrip 拖进「应用程序」。' },
      { h: '首次打开', p: 'Qantrip 尚未经过 Apple 公证：到 系统设置 → 隐私与安全性，点「仍要打开」。' },
      { h: '授权辅助功能', p: '可选。授权后 Qantrip 可以替你粘贴；不授权时 Prompt 会复制到剪贴板。' },
    ],
    commandH: '一条命令的组成',
    commandP: '点击各部分查看说明。',
    parts: [
      ['r', 't-command', '模板名或别名。可以只输前缀，拼错也没关系：↩ 插入右侧预览的内容，⇥ 补全名称。'],
      ['security', 't-positional', '位置参数：直接输入候选值，不用写参数名。'],
      ['--scope=staged', 't-optionname', '带值的参数。参数名和取值都可以只输前缀，如 --sc=st；多个值用逗号分隔。'],
      ['-s', 't-shortflag', '开关参数的单字母简写，-s 就是 --strict。'],
      ['重点看认证和权限', '', '附加要求。从第一个不是参数的词开始，后面的文字原样附在 Prompt 末尾。'],
    ] as [string, string, string][],
    keysH: '快捷键',
    keys: [
      [['⌃', 'Q'], '唤出面板（可自定义）'],
      [['⇥'], '接受候选'],
      [['→'], '接受灰字提示'],
      [['↑', '↓'], '选择候选'],
      [['↩'], '插入 Prompt'],
      [['⌘', '↩'], '在 Claude Code 中展开插入'],
      [['--'], '列出全部参数'],
      [['esc'], '关闭'],
    ] as [string[], string][],
    templatesH: '内置模板',
    params: '参数',
    none: '没有参数。模板名后面输入的文字会附在末尾。',
    source: '源码',
    positional: '位置参数',
    writeH: '编写模板',
    writeP: (
      <>
        模板是 <code>~/Library/Application Support/Qantrip/templates/</code> 里的 Markdown 文件：头部声明参数，正文是 MiniJinja 模板。可以直接在这里改，下面运行的是真实的 Qantrip 核心。
      </>
    ),
    tryLabel: '试用输入',
    problems: '问题',
    loading: '正在加载核心…',
    ok: '格式正确',
    referenceH: '参考',
    typesH: '参数类型',
    types: [
      ['bool', '开关：--strict、--no-strict 或单字母简写'],
      ['enum', '从候选值中选一个'],
      ['multi', '选多个，用逗号分隔'],
      ['string', '一行文字'],
      ['number', '数字，可设 min / max'],
      ['text', '多行文字'],
    ],
    fieldsH: '参数字段',
    fields: [
      ['default', '未指定时的取值'],
      ['short', '单字母简写（仅 bool）'],
      ['positional', '不写参数名直接输入取值（每个模板至多一个）'],
      ['required', '插入前必须填写'],
      ['requires / conflicts', '必须同时设置 / 不能同时设置的参数'],
      ['choices', 'enum / multi 的候选值，可以带显示名和别名'],
    ],
    notes: [
      <>
        正文中参数名里的 <code>-</code> 写作 <code>_</code>，附加要求是变量 <code>extra</code>。
      </>,
      '与内置模板同名的文件会替换内置版本。',
      'Qantrip 会自动重新加载模板目录。',
    ],
  },
} satisfies Record<Locale, unknown>

const SAMPLE: Record<Locale, { source: string; input: string }> = {
  en: {
    source: `---
name: standup
aliases: [su]
description: Daily standup summary
params:
  tone:
    type: enum
    positional: true
    default: brief
    choices: [brief, detailed]
  blockers: { type: bool, short: b, description: Ask about blockers }
---
Summarize what I did yesterday{% if tone == "detailed" %}, with details for each item{% endif %}.
{% if blockers %}
Then list anything that is blocking me.
{% endif %}
{% include "_extra" %}
`,
    input: 'su detailed -b use the git log',
  },
  zh: {
    source: `---
name: standup
aliases: [su]
description: 每日站会总结
params:
  tone:
    type: enum
    positional: true
    default: brief
    choices: [brief, detailed]
  blockers: { type: bool, short: b, description: 询问阻碍 }
---
总结我昨天做了什么{% if tone == "detailed" %}，每一项都写清细节{% endif %}。
{% if blockers %}
然后列出阻碍我推进的问题。
{% endif %}
{% include "_extra" %}
`,
    input: 'su detailed -b 参考 git log',
  },
}

export const meta: Route.MetaFunction = ({ location }) => {
  const locale = localeOf(location.pathname)
  return seo({ locale, path: '/docs', title: T[locale].title, description: T[locale].description })
}

type Text = (typeof T)[Locale]

function Anatomy({ t }: { t: Text }) {
  const [active, setActive] = useState(0)
  const [auto, setAuto] = useState(true)
  useEffect(() => {
    if (!auto || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    const id = setInterval(() => setActive((a) => (a + 1) % t.parts.length), 2800)
    return () => clearInterval(id)
  }, [auto, t.parts.length])
  const pick = (i: number) => {
    setAuto(false)
    setActive(i)
  }
  return (
    <div className="anat">
      <div className="anat-line" role="tablist">
        {t.parts.map(([text, cls], i) => (
          <button key={text} type="button" role="tab" aria-selected={i === active} className={`${cls} ${i === active ? 'on' : ''}`} onClick={() => pick(i)} onMouseEnter={() => pick(i)}>
            {text}
          </button>
        ))}
      </div>
      <div className="anat-note" role="tabpanel">
        {t.parts.map(([text, , note], i) => (
          <p key={text} hidden={i !== active}>
            <code className={t.parts[i]![1]}>{text}</code>
            {note}
          </p>
        ))}
      </div>
    </div>
  )
}

function TemplateBrowser({ t, list }: { t: Text; list: BuiltinTemplate[] }) {
  const [current, setCurrent] = useState(list[0]!.name)
  const section = useRef<HTMLDivElement>(null)

  // 首页模板链接：#t-<模板名>
  useEffect(() => {
    const fromHash = () => {
      const m = /^#t-(.+)$/.exec(window.location.hash)
      if (m && list.some((x) => x.name === m[1])) {
        setCurrent(m[1]!)
        section.current?.scrollIntoView({ block: 'start' })
      }
    }
    fromHash()
    window.addEventListener('hashchange', fromHash)
    return () => window.removeEventListener('hashchange', fromHash)
  }, [list])

  return (
    <div className="tb" ref={section}>
      <nav className="tb-list" aria-label={t.templatesH}>
        {groupTemplates(list).map((g) => (
          <div key={g.category}>
            <span className="tb-cat">{g.category}</span>
            {g.items.map((x) => (
              <button key={x.name} id={`t-${x.name}`} type="button" className={x.name === current ? 'on' : ''} onClick={() => setCurrent(x.name)}>
                <code>{x.name}</code>
                <span>{x.description}</span>
              </button>
            ))}
          </div>
        ))}
      </nav>
      <div className="tb-detail">
        {list.map((x) => (
          <article key={x.name} hidden={x.name !== current}>
            <header>
              <h3>
                <code>{x.name}</code>
                {x.aliases.map((a) => (
                  <code key={a} className="alias">
                    {a}
                  </code>
                ))}
              </h3>
              <p>{x.description}</p>
            </header>
            <span className="label">{t.params}</span>
            {x.params.length === 0 ? (
              <p className="muted">{t.none}</p>
            ) : (
              <ul className="tb-params">
                {x.params.map((p) => (
                  <li key={p.name}>
                    <span className="tb-pname">
                      <code className="t-optionname">--{p.name}</code>
                      {p.short && <code className="t-shortflag">-{p.short}</code>}
                      <span className="tag">{p.kind}</span>
                      {p.positional && <span className="tag">{t.positional}</span>}
                    </span>
                    {p.description && <span className="muted">{p.description}</span>}
                    {p.choices.length > 0 && (
                      <span className="tb-choices">
                        {p.choices.map(([v, l]) => (
                          <span key={v}>
                            <code className="t-optionvalue">{v}</code>
                            {l !== v && <span className="muted"> {l}</span>}
                          </span>
                        ))}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            )}
            <details>
              <summary>{t.source}</summary>
              <pre className="code">{x.source}</pre>
            </details>
          </article>
        ))}
      </div>
    </div>
  )
}

interface Check {
  name: string | null
  problems: string[]
  analysis: Analysis | null
}

function Playground({ t, locale }: { t: Text; locale: Locale }) {
  const [source, setSource] = useState(SAMPLE[locale].source)
  const [input, setInput] = useState(SAMPLE[locale].input)
  const [core, setCore] = useState<Core | null>(null)
  const [check, setCheck] = useState<Check | null>(null)

  useEffect(() => {
    setSource(SAMPLE[locale].source)
    setInput(SAMPLE[locale].input)
  }, [locale])

  useEffect(() => {
    let alive = true
    loadCore()
      .then((c) => alive && setCore(c))
      .catch(() => undefined)
    return () => {
      alive = false
    }
  }, [])

  useEffect(() => {
    if (!core) return
    core.set_locale(locale)
    setCheck(JSON.parse(core.check_source(source, input)) as Check)
  }, [core, locale, source, input])

  const a = check?.analysis
  const errors = a?.diagnostics.filter((d) => d.severity === 'error').map((d) => d.message) ?? []
  const problems = [...(check?.problems ?? []), ...errors]

  return (
    <div className="pg">
      <label className="pg-editor">
        <span className="label">{check?.name ? `${check.name}.md` : 'template.md'}</span>
        <textarea value={source} onChange={(e) => setSource(e.target.value)} spellCheck={false} autoCapitalize="off" autoComplete="off" rows={18} />
      </label>
      <div className="pg-out">
        <label className="pg-try">
          <span className="label">{t.tryLabel}</span>
          <span className="pg-field">
            {a && (
              <span className="pg-hl" aria-hidden="true">
                <Highlighted input={input} tokens={a.tokens} ghost={null} />
              </span>
            )}
            <input value={input} onChange={(e) => setInput(e.target.value)} spellCheck={false} autoCapitalize="off" autoComplete="off" className={a ? 'clear' : ''} />
          </span>
        </label>
        {!core ? (
          <p className="muted">{t.loading}</p>
        ) : problems.length > 0 ? (
          <div className="pg-problems">
            <span className="label">{t.problems}</span>
            {problems.map((p) => (
              <p key={p}>{p}</p>
            ))}
          </div>
        ) : (
          <span className="pg-ok">✓ {t.ok}</span>
        )}
        {a?.preview && <Preview preview={a.preview} />}
      </div>
    </div>
  )
}

export default function Docs() {
  const locale = useLocale()
  const t = T[locale]
  const href = useHref()
  const list = templatesOf(locale)

  return (
    <section className="wrap page docs">
      <header>
        <h1>{t.h1}</h1>
        <p>{t.lead}</p>
      </header>

      <nav className="toc" aria-label={t.h1}>
        {t.toc.map(([id, label]) => (
          <a key={id} href={`#${id}`}>
            {label}
          </a>
        ))}
      </nav>

      <section id="install" className="dsec">
        <h2>{t.toc[0]![1]}</h2>
        <ol className="install">
          {t.install.map((s, i) => (
            <li key={s.h}>
              <span className="step-n">{i + 1}</span>
              <b>{s.h}</b>
              <span>{s.p}</span>
              {s.link && <Link to={href('/download')}>{s.link} →</Link>}
            </li>
          ))}
        </ol>
      </section>

      <section id="command" className="dsec">
        <h2>{t.commandH}</h2>
        <p className="muted">{t.commandP}</p>
        <Anatomy t={t} />
      </section>

      <section id="keys" className="dsec">
        <h2>{t.keysH}</h2>
        <ul className="keys">
          {t.keys.map(([keys, label]) => (
            <li key={label}>
              <span>
                {keys.map((k) => (
                  <kbd key={k}>{k}</kbd>
                ))}
              </span>
              {label}
            </li>
          ))}
        </ul>
      </section>

      <section id="templates" className="dsec">
        <h2>
          {t.templatesH} <span className="muted">· {list.length}</span>
        </h2>
        <TemplateBrowser t={t} list={list} />
      </section>

      <section id="write" className="dsec">
        <h2>{t.writeH}</h2>
        <p className="dsec-p">{t.writeP}</p>
        <Playground t={t} locale={locale} />
      </section>

      <section id="reference" className="dsec">
        <h2>{t.referenceH}</h2>
        <div className="cols">
          <div className="table-wrap">
            <table>
              <caption>{t.typesH}</caption>
              <tbody>
                {t.types.map(([k, v]) => (
                  <tr key={k}>
                    <td>
                      <code>{k}</code>
                    </td>
                    <td>{v}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="table-wrap">
            <table>
              <caption>{t.fieldsH}</caption>
              <tbody>
                {t.fields.map(([k, v]) => (
                  <tr key={k}>
                    <td>
                      <code>{k}</code>
                    </td>
                    <td>{v}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
        <ul className="notes">
          {t.notes.map((n, i) => (
            <li key={i}>{n}</li>
          ))}
        </ul>
      </section>
    </section>
  )
}
