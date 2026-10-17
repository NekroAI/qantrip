import { useCallback, useEffect, useRef, useState } from 'react'
import snapshot from '../lib/demo-snapshot.json'

// 官网在线演示（docs/decisions.md D27）：浏览器中运行 Qantrip 的真实核心（Rust → WebAssembly），
// 解析、补全、预览与桌面应用完全一致。首帧使用构建时生成的快照，WebAssembly 加载后可以直接输入。

interface Token { kind: string; start: number; end: number }
interface Edit { start: number; end: number; text: string }
interface Candidate { kind: 'template' | 'param' | 'choice' | 'invocation'; key: string; title: string; subtitle: string; detail: string; edit: Edit; active: boolean; section: string }
interface ParamState { name: string; kind: string; display: string; is_set: boolean; choices: { value: string; label: string; on: boolean }[] }
interface Highlight { kind: 'param' | 'extra'; start: number; end: number }
interface Analysis {
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
interface Core {
  analyze(input: string, selected?: string): string
  render(input: string, selected?: string): string | undefined
  edit_param(input: string, selected: string | undefined, name: string, edit: string): string | undefined
}

/** 空闲时自动演示的命令 */
const SCRIPT = ['r security --scope=staged -s 重点看认证和权限', 'zh -k', 'commit --lang=zh 说明为什么改', 'explain beginner -d 这个函数']
const EXAMPLES = ['r security -s', 'zh -k', 'commit --lang=zh', 'r --', 'reveiw', 'test unit edge']

const replace = (s: string, e: Edit) => s.slice(0, e.start) + e.text + s.slice(e.end)

function Highlighted({ input, tokens, ghost }: { input: string; tokens: Token[]; ghost: string | null }) {
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

function Preview({ preview }: { preview: NonNullable<Analysis['preview']> }) {
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
  const core = useRef<Core | null>(null)
  const [ready, setReady] = useState(false)
  const [input, setInput] = useState(snapshot.input)
  const [analysis, setAnalysis] = useState<Analysis>(snapshot.analysis as unknown as Analysis)
  const [selectedKey, setSelectedKey] = useState<string | undefined>()
  const [index, setIndex] = useState(0)
  const [shake, setShake] = useState(0)
  const [inserted, setInserted] = useState<{ id: number; text: string }[]>([])
  const [touched, setTouched] = useState(false)
  const field = useRef<HTMLInputElement>(null)
  const overlay = useRef<HTMLDivElement>(null)

  const analyze = useCallback((text: string, sel?: string) => {
    if (!core.current) return
    setInput(text)
    setAnalysis(JSON.parse(core.current.analyze(text, sel)) as Analysis)
  }, [])

  // 加载 WebAssembly 核心（不阻塞首屏）
  useEffect(() => {
    let alive = true
    const load = async () => {
      const url = '/wasm/qantrip_wasm.js'
      const mod = (await import(/* @vite-ignore */ url)) as Core & { default: () => Promise<unknown> }
      await mod.default()
      if (!alive) return
      core.current = mod
      setReady(true)
    }
    load().catch(() => undefined)
    return () => {
      alive = false
    }
  }, [])

  // 空闲时自动演示：逐字输入、回车插入，用户一旦操作就停止
  useEffect(() => {
    if (!ready || touched || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    let cancelled = false
    let timer: ReturnType<typeof setTimeout>
    const wait = (ms: number) => new Promise<void>((r) => (timer = setTimeout(r, ms)))
    const run = async () => {
      await wait(1800)
      for (let n = 0; !cancelled; n++) {
        const cmd = SCRIPT[n % SCRIPT.length]!
        setSelectedKey(undefined)
        setIndex(0)
        for (let i = 0; i <= cmd.length && !cancelled; i++) {
          analyze(cmd.slice(0, i))
          await wait(cmd[i - 1] === ' ' ? 260 : 70)
        }
        await wait(1400)
        if (cancelled) return
        const text = core.current!.render(cmd)
        if (text) setInserted((list) => [...list.slice(-2), { id: Date.now(), text }])
        analyze('')
        await wait(2600)
      }
    }
    run()
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [ready, touched, analyze])

  /** 访客开始操作：停止自动演示，保留当前输入，在此基础上继续 */
  const takeOver = () => {
    if (!touched) setTouched(true)
  }

  const setText = (text: string, caret?: number) => {
    setSelectedKey(undefined)
    setIndex(0)
    analyze(text)
    requestAnimationFrame(() => {
      const el = field.current
      if (!el) return
      el.focus()
      const at = caret ?? text.length
      el.setSelectionRange(at, at)
      if (overlay.current) overlay.current.scrollLeft = el.scrollLeft
    })
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
    setInserted((list) => [...list.slice(-2), { id: Date.now(), text }])
    setText('')
  }

  const onKey = (ev: React.KeyboardEvent<HTMLInputElement>) => {
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
      setText('')
    }
  }

  const clickChip = (p: ParamState) => {
    if (!core.current) return
    takeOver()
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
    <div className="demo">
      <div className="demo-caption">
        <span className="live">
          <span className={ready ? 'dot on' : 'dot'} />
          {ready ? '这是 Qantrip 的真实核心，在你的浏览器里运行。点击输入框试试。' : '正在加载核心…'}
        </span>
        <span>
          <kbd>⇥</kbd> 补全 · <kbd>↑</kbd><kbd>↓</kbd> 选择 · <kbd>↩</kbd> 插入
        </span>
      </div>

      <div className={shake % 2 ? 'panel-ui shake' : 'panel-ui'} key={`p${shake}`}>
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
              disabled={!ready}
              aria-label="Qantrip 命令"
              placeholder="输入命令，例如 r security 重点检查认证"
              spellCheck={false}
              autoComplete="off"
              autoCapitalize="off"
              onFocus={takeOver}
              onChange={(e) => {
                setSelectedKey(undefined)
                setIndex(0)
                analyze(e.target.value)
              }}
              onScroll={(e) => {
                if (overlay.current) overlay.current.scrollLeft = e.currentTarget.scrollLeft
              }}
              onKeyDown={onKey}
            />
          </div>
          <span className="pi-target">→ Claude Code</span>
        </div>

        <div className="pi-body">
          <div className="pi-list" role="listbox" aria-label="候选">
            {rows.length === 0 ? (
              <div className="pi-empty">
                {analysis.phase === 'extra' ? '正在输入附加要求：从这里开始的文字会原样附在 Prompt 末尾。' : '没有可补全的内容'}
              </div>
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
                      onClick={() => {
                        takeOver()
                        accept(i)
                      }}
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
                      <span>
                        预览 · {analysis.preview.chars} 字 · {analysis.preview.lines} 行
                      </span>
                      {analysis.preview.collapses_in_claude_code && <span>Claude Code 中会折叠，桌面端可用 ⌘↩ 展开粘贴</span>}
                    </div>
                    <Preview preview={analysis.preview} />
                  </>
                )}
              </>
            ) : (
              <div className="pi-empty">没有匹配的模板</div>
            )}
          </div>
        </div>

        <div className="pi-foot">
          {err ? <span className={err.at_cursor ? 'err soft' : 'err'}>{err.message}</span> : <span className="canon">{analysis.locked ? analysis.canonical : ''}</span>}
          <span className="pi-keys">
            <span>
              <kbd>↩</kbd> 插入
            </span>
            <span>
              <kbd>⇥</kbd> 补全
            </span>
            <span>
              <kbd>esc</kbd> 清空
            </span>
          </span>
        </div>
      </div>

      <div className="term" aria-live="polite">
        <div className="term-head">
          <i /> <i /> <i /> <span>~/project — claude</span>
        </div>
        <div className="term-body">
          {inserted.length === 0 ? (
            <span className="muted">回车后，生成的 Prompt 会出现在这里。</span>
          ) : (
            inserted.map((x, i) => (
              <div key={x.id} className={i === inserted.length - 1 ? 'fresh' : undefined} style={{ marginBottom: 12 }}>
                <span className="p">›</span>
                {x.text}
              </div>
            ))
          )}
        </div>
      </div>

      <div className="try">
        试试：
        {EXAMPLES.map((e) => (
          <button
            key={e}
            type="button"
            disabled={!ready}
            onClick={() => {
              takeOver()
              setText(e)
            }}
          >
            {e}
          </button>
        ))}
      </div>
    </div>
  )
}
