import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { type Core, loadCore } from '../lib/core'
import { useSummonKey } from '../lib/downloads'
import { type Locale, useLocale } from '../lib/i18n'
import snapshots from '../lib/demo-snapshot.json'

// 官网在线演示（docs/decisions.md D27 / D29）：一个模拟的 Claude Code 会话。访客在输入框里按唤起快捷键，
// Qantrip 面板浮出；面板运行的是真实核心（Rust → WebAssembly），解析、补全、预览与桌面应用一致。
// 回车把 Prompt 插入到输入框的光标处，再回车发送。首帧使用构建时生成的快照，空闲时自动演示。

export interface Token { kind: string; start: number; end: number }
interface Edit { start: number; end: number; text: string }
interface Candidate { kind: 'template' | 'param' | 'choice' | 'invocation'; key: string; title: string; subtitle: string; detail: string; edit: Edit; active: boolean; section: string }
interface ParamState { name: string; kind: string; display: string; is_set: boolean; choices: { value: string; label: string; on: boolean }[] }
interface Highlight { kind: 'param' | 'extra'; start: number; end: number }
export interface Analysis {
  tokens: Token[]
  phase: 'command' | 'params' | 'extra'
  template: { name: string; description: string } | null
  locked: boolean
  candidates: Candidate[]
  ghost: string | null
  diagnostics: { severity: 'error' | 'warning'; message: string; at_cursor: boolean }[]
  params: ParamState[]
  preview: { text: string; highlights: Highlight[]; chars: number; lines: number; collapses_in_claude_code: boolean } | null
  canonical: string | null
  enter: { kind: 'insert' } | { kind: 'complete'; edit: Edit } | { kind: 'blocked' }
  hint: string
}
interface Message { id: number; role: 'user' | 'assistant'; text: string }

const T = {
  en: {
    script: ['r security --scope=staged -s focus on the auth module', 'test unit edge the parse_config function', 'commit', 'explain beginner -d this function'],
    caption: (key: string) => `A simulated Claude Code session. Click the input and press ${key}.`,
    loading: 'Loading the core…',
    placeholder: (key: string) => `Type here, or press ${key} to summon Qantrip`,
    panelPlaceholder: 'Type a template, e.g. r security',
    send: '↩ send · ⇧↩ new line',
    summon: 'Summon Qantrip',
    reply: '(Demo) This Claude Code is simulated, so nothing runs here. Qantrip works the same way in any app you type into.',
    welcome: 'Claude Code (simulated)',
    noMatch: 'No matching template',
    noCandidates: 'Nothing to complete',
    extraPhase: 'Free text: everything from here is appended to the prompt as is.',
    preview: (c: number, l: number) => `Preview · ${c} chars · ${l} lines`,
    folds: 'Claude Code folds this paste; the app expands it with ⌘↩',
    insert: 'insert',
    complete: 'complete',
    close: 'close',
    command: 'Qantrip command',
    candidates: 'Suggestions',
    input: 'Message',
  },
  zh: {
    script: ['r security --scope=staged -s 重点看认证和权限', 'test unit edge parse_config 函数', 'commit --lang=zh', 'explain beginner -d 这个函数'],
    caption: (key: string) => `这是一个模拟的 Claude Code。点进输入框，按 ${key} 试试。`,
    loading: '正在加载核心…',
    placeholder: (key: string) => `在这里输入，或按 ${key} 唤出 Qantrip`,
    panelPlaceholder: '输入模板名，例如 r security',
    send: '↩ 发送 · ⇧↩ 换行',
    summon: '唤出 Qantrip',
    reply: '（演示）这里的 Claude Code 是模拟的，不会真的执行。Qantrip 在任何能输入文字的应用里都是同样的用法。',
    welcome: 'Claude Code（模拟）',
    noMatch: '没有匹配的模板',
    noCandidates: '没有可补全的内容',
    extraPhase: '正在输入附加要求：从这里开始的文字会原样附在 Prompt 末尾。',
    preview: (c: number, l: number) => `预览 · ${c} 字 · ${l} 行`,
    folds: 'Claude Code 会折叠这段粘贴，应用里用 ⌘↩ 展开',
    insert: '插入',
    complete: '补全',
    close: '关闭',
    command: 'Qantrip 命令',
    candidates: '候选',
    input: '消息',
  },
} satisfies Record<Locale, unknown>

const replace = (s: string, e: Edit) => s.slice(0, e.start) + e.text + s.slice(e.end)

export function Highlighted({ input, tokens, ghost }: { input: string; tokens: Token[]; ghost: string | null }) {
  const parts: React.ReactNode[] = []
  let at = 0
  for (const t of [...tokens].sort((a, b) => a.start - b.start)) {
    if (t.start < at) continue
    if (t.start > at) parts.push(input.slice(at, t.start))
    parts.push(<span key={t.start} className={`t-${t.kind.replace(/_/g, '')}`}>{input.slice(t.start, t.end)}</span>)
    at = t.end
  }
  if (at < input.length) parts.push(input.slice(at))
  return (
    <>
      {parts}
      {ghost && <span className="t-ghost">{ghost}</span>}
    </>
  )
}

export function Preview({ preview }: { preview: NonNullable<Analysis['preview']> }) {
  const out: React.ReactNode[] = []
  let at = 0
  for (const h of [...preview.highlights].sort((a, b) => a.start - b.start)) {
    if (h.start < at) continue
    if (h.start > at) out.push(preview.text.slice(at, h.start))
    out.push(<mark key={h.start} className={h.kind === 'extra' ? 'extra' : undefined}>{preview.text.slice(h.start, h.end)}</mark>)
    at = h.end
  }
  out.push(preview.text.slice(at))
  return <div className="pi-preview">{out}</div>
}

export function LiveDemo() {
  const locale = useLocale()
  const t = T[locale]
  const keyKind = useSummonKey()
  const summonKey = keyKind === 'mac' ? '⌃Q' : keyKind === 'ctrl' ? 'Ctrl+Q' : 'Alt+Q'
  const snapshot = snapshots[locale]

  const core = useRef<Core | null>(null)
  const [ready, setReady] = useState(false)
  const [touched, setTouched] = useState(false)
  const touchedRef = useRef(false)
  const root = useRef<HTMLDivElement>(null)
  const visible = useRef(false)

  // 面板
  const [open, setOpen] = useState(true)
  const [input, setInput] = useState(snapshot.input)
  const [analysis, setAnalysis] = useState<Analysis>(snapshot.analysis as unknown as Analysis)
  const [selectedKey, setSelectedKey] = useState<string | undefined>()
  const [index, setIndex] = useState(0)
  const [shake, setShake] = useState(0)
  const field = useRef<HTMLInputElement>(null)
  const overlay = useRef<HTMLDivElement>(null)

  // 模拟的 Claude Code
  const [draft, setDraft] = useState('')
  const draftRef = useRef('')
  const [messages, setMessages] = useState<Message[]>([])
  const textarea = useRef<HTMLTextAreaElement>(null)
  const transcript = useRef<HTMLDivElement>(null)
  /** 唤出面板时输入框的选区；插入时替换这段文字 */
  const selection = useRef({ start: 0, end: 0 })
  const [keyFlash, setKeyFlash] = useState<{ id: number; keys: string } | null>(null)

  const analyze = useCallback((text: string, sel?: string) => {
    if (!core.current) return
    setInput(text)
    setAnalysis(JSON.parse(core.current.analyze(text, sel)) as Analysis)
  }, [])

  const updateDraft = (text: string) => {
    draftRef.current = text
    setDraft(text)
  }

  // 加载 WebAssembly 核心（不阻塞首屏）
  useEffect(() => {
    let alive = true
    loadCore()
      .then((mod) => {
        if (!alive) return
        core.current = mod
        setReady(true)
      })
      .catch(() => undefined)
    return () => {
      alive = false
    }
  }, [])

  // 语言切换：核心换成对应语言的提示与内置模板
  useEffect(() => {
    if (!ready || !core.current) return
    core.current.set_locale(locale)
    setSelectedKey(undefined)
    setIndex(0)
    analyze(touchedRef.current ? '' : snapshots[locale].input)
  }, [ready, locale, analyze])

  // 演示区在视口内时才响应唤起快捷键、才自动演示
  useEffect(() => {
    const el = root.current
    if (!el) return
    const io = new IntersectionObserver(([e]) => (visible.current = (e?.intersectionRatio ?? 0) > 0.35), { threshold: [0, 0.35, 0.6] })
    io.observe(el)
    return () => io.disconnect()
  }, [])

  const takeOver = useCallback(() => {
    if (touchedRef.current) return
    touchedRef.current = true
    setTouched(true)
    setKeyFlash(null)
  }, [])

  const flash = (keys: string) => setKeyFlash({ id: Date.now(), keys })

  const openPanel = useCallback(
    (focus: boolean) => {
      const ta = textarea.current
      const len = draftRef.current.length
      selection.current = ta && document.activeElement === ta ? { start: ta.selectionStart, end: ta.selectionEnd } : { start: len, end: len }
      setOpen(true)
      setSelectedKey(undefined)
      setIndex(0)
      analyze('')
      if (focus) requestAnimationFrame(() => field.current?.focus({ preventScroll: true }))
    },
    [analyze],
  )

  /** 关闭面板，焦点回到输入框并恢复光标 */
  const closePanel = useCallback((caret?: number) => {
    setOpen(false)
    if (!touchedRef.current) return
    requestAnimationFrame(() => {
      const ta = textarea.current
      if (!ta) return
      ta.focus({ preventScroll: true })
      const at = caret ?? selection.current.end
      ta.setSelectionRange(at, at)
    })
  }, [])

  const insert = useCallback(
    (text: string) => {
      const { start, end } = selection.current
      const d = draftRef.current
      const next = d.slice(0, start) + text + d.slice(end)
      updateDraft(next)
      const caret = start + text.length
      selection.current = { start: caret, end: caret }
      closePanel(caret)
    },
    [closePanel],
  )

  const send = useCallback(() => {
    const text = draftRef.current.trim()
    if (!text) return
    const id = Date.now()
    setMessages((m) => [...m.slice(-4), { id, role: 'user', text }])
    updateDraft('')
    selection.current = { start: 0, end: 0 }
    setTimeout(() => setMessages((m) => [...m.slice(-4), { id: id + 1, role: 'assistant', text: T[locale].reply }]), 650)
  }, [locale])

  // 新消息出现时滚动到底部（只滚动对话区域，不滚动页面）
  useEffect(() => {
    const el = transcript.current
    if (el) el.scrollTop = el.scrollHeight
  }, [messages])

  // 唤起快捷键：Mac 上 ⌃Q、Windows 上 Ctrl+Q（与应用一致）、Linux 上 Alt+Q（Linux 浏览器中 Ctrl+Q 会退出）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const hit = e.code === 'KeyQ' && (keyKind === 'alt' ? e.altKey && !e.ctrlKey && !e.metaKey : e.ctrlKey && !e.metaKey && !e.altKey)
      if (!hit || !ready) return
      const inside = root.current?.contains(document.activeElement)
      if (!inside && !visible.current) return
      e.preventDefault()
      takeOver()
      if (open) closePanel()
      else openPanel(true)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [keyKind, ready, open, takeOver, openPanel, closePanel])

  // 空闲时自动演示：唤出 → 逐字输入 → 回车插入 → 回车发送。访客一旦操作就停止，保留当前状态。
  useEffect(() => {
    if (!ready || touched || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    let cancelled = false
    let timer: ReturnType<typeof setTimeout>
    const wait = (ms: number) => new Promise<void>((r) => (timer = setTimeout(r, ms)))
    const waitVisible = async () => {
      while (!cancelled && !visible.current) await wait(400)
    }
    const run = async () => {
      const script = T[locale].script
      await wait(1600)
      for (let n = 0; !cancelled; n++) {
        await waitVisible()
        const cmd = script[n % script.length]!
        if (n > 0) {
          flash(summonKey)
          await wait(450)
          if (cancelled) return
          openPanel(false)
          await wait(500)
          for (let i = 1; i <= cmd.length && !cancelled; i++) {
            analyze(cmd.slice(0, i))
            await wait(cmd[i - 1] === ' ' ? 240 : 65)
          }
        }
        await wait(1300)
        if (cancelled) return
        flash('↩')
        const text = core.current!.render(cmd)
        if (text) insert(text)
        await wait(1400)
        if (cancelled) return
        flash('↩')
        send()
        await wait(3400)
      }
    }
    run()
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [ready, touched, locale, summonKey, analyze, openPanel, insert, send])

  /** 程序改写面板输入后要放置的光标位置；在 DOM 更新后、处理下一个按键之前同步设置 */
  const pendingCaret = useRef<number | null>(null)
  useLayoutEffect(() => {
    const el = field.current
    if (pendingCaret.current === null || !el) return
    el.focus({ preventScroll: true })
    el.setSelectionRange(pendingCaret.current, pendingCaret.current)
    if (overlay.current) overlay.current.scrollLeft = el.scrollLeft
    pendingCaret.current = null
  }, [input])

  const setText = (text: string, caret?: number) => {
    setSelectedKey(undefined)
    setIndex(0)
    pendingCaret.current = caret ?? text.length
    analyze(text)
  }

  const select = (i: number) => {
    const c = analysis.candidates[i]
    if (!c) return
    setIndex(i)
    if (!analysis.locked && (c.kind === 'template' || c.kind === 'invocation')) {
      setSelectedKey(c.key)
      setAnalysis(JSON.parse(core.current!.analyze(input, c.key)) as Analysis)
    }
  }

  const accept = (i = index) => {
    const c = analysis.candidates[i]
    if (c) setText(replace(input, c.edit), c.edit.start + c.edit.text.length)
    else if (analysis.ghost) setText(input + analysis.ghost)
  }

  const confirm = () => {
    const e = analysis.enter
    if (e.kind === 'complete') return setText(replace(input, e.edit))
    const text = e.kind === 'insert' ? core.current!.render(input, selectedKey) : undefined
    if (!text) return setShake((n) => n + 1)
    insert(text)
  }

  const onPanelKey = (ev: React.KeyboardEvent<HTMLInputElement>) => {
    if (!core.current || ev.nativeEvent.isComposing) return
    const atEnd = ev.currentTarget.selectionStart === input.length
    if (ev.key === 'Tab') {
      ev.preventDefault()
      accept()
    } else if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') {
      ev.preventDefault()
      const n = analysis.candidates.length
      if (n) select((index + (ev.key === 'ArrowDown' ? 1 : -1) + n) % n)
    } else if (ev.key === 'ArrowRight' && atEnd && analysis.ghost) {
      ev.preventDefault()
      setText(input + analysis.ghost)
    } else if (ev.key === 'Enter') {
      ev.preventDefault()
      confirm()
    } else if (ev.key === 'Escape') {
      ev.preventDefault()
      closePanel()
    }
  }

  const onDraftKey = (ev: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (ev.nativeEvent.isComposing) return
    if (ev.key === 'Enter' && !ev.shiftKey && !ev.altKey) {
      ev.preventDefault()
      send()
    }
  }

  const clickChip = (p: ParamState) => {
    if (!core.current) return
    let edit: object | null = null
    if (p.kind === 'bool') edit = { kind: 'toggle' }
    if (p.kind === 'enum' && p.choices.length) {
      const cur = p.choices.findIndex((c) => c.on)
      edit = { kind: 'set', value: p.choices[(cur + 1) % p.choices.length]!.value }
    }
    if (!edit) return
    const res = core.current.edit_param(input, selectedKey, p.name, JSON.stringify(edit))
    if (res) {
      const r = JSON.parse(res) as { text: string; caret: number }
      setText(r.text, r.caret)
    }
  }

  // 已完成的词上的错误醒目显示；正在输入的词上的错误淡色显示（与桌面应用一致）
  const err = analysis.diagnostics.find((d) => d.severity === 'error' && !d.at_cursor) ?? analysis.diagnostics.find((d) => d.severity === 'error')
  const rows = analysis.candidates
  let lastSection = ''

  return (
    <div className="demo" ref={root} onPointerDown={takeOver} onKeyDown={takeOver}>
      <div className="demo-caption">
        <span className="live">
          <span className={ready ? 'dot on' : 'dot'} />
          {ready ? t.caption(summonKey) : t.loading}
        </span>
      </div>

      <div className="cc">
        <div className="cc-bar">
          <i />
          <i />
          <i />
          <span>~/shop-api — claude</span>
          <button type="button" className="cc-summon" disabled={!ready} onMouseDown={(e) => e.preventDefault()} onClick={() => (open ? closePanel() : openPanel(true))}>
            <kbd>{summonKey}</kbd> {t.summon}
          </button>
        </div>

        <div className="cc-body" ref={transcript}>
          <div className="cc-welcome">
            <span className="cc-star">✻</span> {t.welcome}
            <span className="muted">cwd: ~/shop-api</span>
          </div>
          {messages.map((m) => (
            <div key={m.id} className={`cc-msg ${m.role}`}>
              <span className="cc-mark">{m.role === 'user' ? '>' : '⏺'}</span>
              <span>{m.text}</span>
            </div>
          ))}
        </div>

        <div className="cc-input" onClick={() => !open && textarea.current?.focus()}>
          <span className="cc-prompt">&gt;</span>
          <textarea
            ref={textarea}
            value={draft}
            rows={1}
            aria-label={t.input}
            placeholder={t.placeholder(summonKey)}
            spellCheck={false}
            onChange={(e) => updateDraft(e.target.value)}
            onKeyDown={onDraftKey}
            onSelect={(e) => (selection.current = { start: e.currentTarget.selectionStart, end: e.currentTarget.selectionEnd })}
          />
        </div>
        <div className="cc-status">
          <span>{t.send}</span>
          <span>shop-api · main</span>
        </div>

        {keyFlash && (
          <div className="keyflash" key={keyFlash.id} aria-hidden="true">
            {keyFlash.keys}
          </div>
        )}

        <div className={open ? 'cc-overlay on' : 'cc-overlay'} onMouseDown={(e) => e.target === e.currentTarget && closePanel()} aria-hidden={!open}>
          <div className={shake % 2 ? 'panel-ui shake' : 'panel-ui'} key={`p${shake}`} role="dialog" aria-label="Qantrip">
            <div className="pi-head">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
                <path d="M4 20 15 9M15 9l2-2M18 3v3M21 6h-3M19.5 9.5 21 11M13 4.5 11.5 3" strokeLinecap="round" />
              </svg>
              <div className="pi-field">
                <div className="pi-overlay" ref={overlay} aria-hidden="true">
                  <Highlighted input={input} tokens={analysis.tokens} ghost={analysis.ghost} />
                </div>
                <input
                  ref={field}
                  value={input}
                  disabled={!ready || !open}
                  aria-label={t.command}
                  placeholder={t.panelPlaceholder}
                  spellCheck={false}
                  autoComplete="off"
                  autoCapitalize="off"
                  onChange={(e) => {
                    setSelectedKey(undefined)
                    setIndex(0)
                    analyze(e.target.value)
                  }}
                  onScroll={(e) => {
                    if (overlay.current) overlay.current.scrollLeft = e.currentTarget.scrollLeft
                  }}
                  onKeyDown={onPanelKey}
                />
              </div>
              <span className="pi-target">→ Claude Code</span>
            </div>

            <div className="pi-body">
              <div className="pi-list" role="listbox" aria-label={t.candidates}>
                {rows.length === 0 ? (
                  <div className="pi-empty">{analysis.phase === 'extra' ? t.extraPhase : t.noCandidates}</div>
                ) : (
                  rows.map((c, i) => {
                    const header = c.section && c.section !== lastSection ? c.section : null
                    lastSection = c.section || lastSection
                    return (
                      <div key={`${c.section}-${c.kind}-${c.key}`}>
                        {header && <div className="pi-section">{header}</div>}
                        <button
                          type="button"
                          role="option"
                          aria-selected={i === index}
                          className={i === index ? 'pi-row sel' : 'pi-row'}
                          onMouseDown={(e) => e.preventDefault()}
                          onClick={() => accept(i)}
                        >
                          <span className={c.kind === 'param' ? 'param' : c.kind === 'choice' ? 'choice' : undefined}>
                            {c.title}
                            {c.active && <span className="check"> ✓</span>}
                          </span>
                          <small>{c.kind === 'template' ? c.detail : c.subtitle}</small>
                        </button>
                      </div>
                    )
                  })
                )}
              </div>

              <div className="pi-main">
                {analysis.template ? (
                  <>
                    <div className="pi-title">
                      <b>{analysis.template.name}</b>
                      <span>{analysis.template.description}</span>
                      {analysis.hint && <em>{analysis.hint}</em>}
                    </div>
                    {analysis.params.length > 0 && (
                      <div className="pi-chips">
                        {analysis.params.map((p) => {
                          const clickable = ready && (p.kind === 'bool' || p.kind === 'enum')
                          return (
                            <button
                              type="button"
                              key={p.name}
                              className={['pi-chip', p.is_set ? 'on' : '', clickable ? 'click' : ''].join(' ')}
                              onMouseDown={(e) => e.preventDefault()}
                              onClick={() => clickable && clickChip(p)}
                              tabIndex={clickable ? 0 : -1}
                            >
                              <b>{p.name}</b> {p.display}
                            </button>
                          )
                        })}
                      </div>
                    )}
                    {analysis.preview && (
                      <>
                        <div className="pi-meta">
                          <span>{t.preview(analysis.preview.chars, analysis.preview.lines)}</span>
                          {analysis.preview.collapses_in_claude_code && <span>{t.folds}</span>}
                        </div>
                        <Preview preview={analysis.preview} />
                      </>
                    )}
                  </>
                ) : (
                  <div className="pi-empty">{input.trim() ? t.noMatch : ''}</div>
                )}
              </div>
            </div>

            <div className="pi-foot">
              {err ? <span className={err.at_cursor ? 'err soft' : 'err'}>{err.message}</span> : <span className="canon">{analysis.locked ? analysis.canonical : ''}</span>}
              <span className="pi-keys">
                <span>
                  <kbd>↩</kbd> {t.insert}
                </span>
                <span>
                  <kbd>⇥</kbd> {t.complete}
                </span>
                <span>
                  <kbd>esc</kbd> {t.close}
                </span>
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
