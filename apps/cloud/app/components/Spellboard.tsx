import { useEffect, useState } from 'react'

// 首页"咒文台"：模拟面板中逐字输入命令，命令行着色、灰字补全、参数标签与预览随之组装。
// 文本取自真实的内置模板渲染结果；服务端渲染输出最终状态，客户端加载后循环播放。

interface Step {
  text: string
  ghost?: string
}

interface Scenario {
  title: string
  description: string
  positional: string[]
  steps: Step[]
  list: (text: string) => { label: string; hint: string }[]
  chips: (text: string) => { name: string; value: string; set: boolean }[]
  preview: (text: string) => { text: string; mark?: 'param' | 'extra' }[]
}

/** 把若干段落"打字"成逐字的步骤 */
const typed = (from: string, to: string, ghost?: (s: string) => string | undefined): Step[] => {
  const out: Step[] = []
  for (let i = from.length + 1; i <= to.length; i++) {
    const text = to.slice(0, i)
    const g = ghost?.(text)
    out.push(g === undefined ? { text } : { text, ghost: g })
  }
  return out
}

const extraOf = (text: string, known: RegExp): string => {
  const words = text.split(' ')
  const idx = words.findIndex((w, i) => i > 0 && w !== '' && !w.startsWith('-') && !known.test(w))
  return idx < 0 ? '' : words.slice(idx).join(' ').trim()
}

const REVIEW_KNOWN = /^(security|performance|correctness|tests)$/

const review: Scenario = {
  title: 'review',
  description: '代码审查',
  positional: ['security', 'performance'],
  steps: [
    ...typed('', 'r', () => 'eview'),
    { text: 'r ' },
    ...typed('r ', 'r sec', (s) => ({ 'r s': 'ecurity', 'r se': 'curity', 'r sec': 'urity' })[s]),
    { text: 'r security ' },
    ...typed('r security ', 'r security --sc', (s) => (s.endsWith('--sc') ? 'ope=' : undefined)),
    { text: 'r security --scope=' },
    ...typed('r security --scope=', 'r security --scope=st', (s) => (s.endsWith('=st') ? 'aged' : undefined)),
    { text: 'r security --scope=staged ' },
    ...typed('r security --scope=staged ', 'r security --scope=staged -s '),
    ...typed('r security --scope=staged -s ', 'r security --scope=staged -s 重点看认证和权限'),
  ],
  list: (t) =>
    t.includes('--scope=') && !t.includes('staged ')
      ? [{ label: 'changes', hint: '所有未提交' }, { label: 'staged', hint: '暂存区' }, { label: 'branch', hint: '当前分支' }, { label: 'last', hint: '最近提交' }]
      : t.trim() === 'r'
        ? [{ label: 'review', hint: '代码审查' }, { label: 'refactor', hint: '代码重构' }, { label: 'arch', hint: '架构评估' }]
        : [{ label: '--focus', hint: '审查重点' }, { label: '--scope', hint: '审查范围' }, { label: '--strict', hint: '严格模式' }, { label: 'security', hint: '安全' }, { label: 'performance', hint: '性能' }],
  chips: (t) => [
    { name: 'focus', value: / security( |$)/.test(t) ? '安全' : '—', set: / security( |$)/.test(t) },
    { name: 'scope', value: t.includes('staged ') ? '暂存区' : '所有未提交', set: t.includes('staged ') },
    { name: 'strict', value: t.includes(' -s ') ? '开' : '关', set: t.includes(' -s ') },
  ],
  preview: (t) => {
    const focus = / security( |$)/.test(t)
    const staged = t.includes('staged ')
    const strict = t.includes(' -s ')
    const extra = extraOf(t, REVIEW_KNOWN)
    return [
      { text: '请审查' },
      staged ? { text: '暂存区的改动（git diff --cached）', mark: 'param' } : { text: '当前所有未提交的改动（git diff HEAD）' },
      { text: '。\n\n先理解改动的意图，再找问题。\n' },
      focus ? { text: '重点关注：安全（权限、输入校验、敏感信息、注入）。', mark: 'param' } : { text: '关注正确性、边界条件、安全和可维护性。' },
      { text: '\n\n输出要求：\n1. 按严重程度排序列出问题，给出文件与行号、原因和修改建议。\n2. 区分"确认的问题"和"需要验证的疑点"。\n' },
      strict ? { text: '3. 严格模式：不确定的地方也列出来并标注置信度。', mark: 'param' } : { text: '3. 只报告真正值得修改的问题。' },
      ...(extra ? [{ text: '\n\n' }, { text: `补充要求：${extra}`, mark: 'extra' as const }] : []),
    ]
  },
}

const commit: Scenario = {
  title: 'commit',
  description: '生成提交信息',
  positional: ['conventional', 'plain'],
  steps: [
    ...typed('', 'ci', (s) => (s === 'c' ? 'ommit' : undefined)),
    { text: 'ci ' },
    ...typed('ci ', 'ci --la', (s) => (s.endsWith('--la') ? 'ng=' : undefined)),
    { text: 'ci --lang=' },
    ...typed('ci --lang=', 'ci --lang=z', (s) => (s.endsWith('=z') ? 'h' : undefined)),
    { text: 'ci --lang=zh ' },
    ...typed('ci --lang=zh ', 'ci --lang=zh 说明为什么改'),
  ],
  list: (t) =>
    t.includes('--lang=') && !t.includes('zh ')
      ? [{ label: 'en', hint: '英文' }, { label: 'zh', hint: '中文' }]
      : t.trim().length <= 2
        ? [{ label: 'commit', hint: 'ci · 提交信息' }, { label: 'arch', hint: '架构评估' }]
        : [{ label: '--style', hint: '格式' }, { label: '--lang', hint: '语言' }, { label: '--run', hint: '直接提交' }],
  chips: (t) => [
    { name: 'style', value: 'Conventional', set: false },
    { name: 'lang', value: t.includes('zh') ? '中文' : '英文', set: t.includes('lang=zh') },
    { name: 'run', value: '关', set: false },
  ],
  preview: (t) => {
    const zh = t.includes('lang=zh')
    const extra = extraOf(t, /^(conventional|plain)$/)
    return [
      { text: '请根据暂存区的改动（git diff --cached）写一条提交信息，使用' },
      zh ? { text: '中文', mark: 'param' } : { text: '英文' },
      { text: '。\n\n格式遵循 Conventional Commits：type(scope): subject。\n- 标题不超过 72 个字符，说明做了什么。\n- 正文说明为什么这样改。\n\n只输出提交信息，不要执行提交。' },
      ...(extra ? [{ text: '\n\n' }, { text: `补充要求：${extra}`, mark: 'extra' as const }] : []),
    ]
  },
}

const SCENARIOS = [review, commit]

const Tokens = ({ text, positional }: { text: string; positional: string[] }) => {
  const parts = text.split(/( )/)
  let index = 0
  let extra = false
  return (
    <>
      {parts.map((p, i) => {
        if (p === ' ' || p === '') return <span key={i}>{p}</span>
        const first = index++ === 0
        if (first) return <span key={i} className="tok-cmd">{p}</span>
        if (extra) return <span key={i}>{p}</span>
        if (p.startsWith('-')) {
          const eq = p.indexOf('=')
          return eq < 0 ? (
            <span key={i} className="tok-opt">{p}</span>
          ) : (
            <span key={i}>
              <span className="tok-opt">{p.slice(0, eq + 1)}</span>
              <span className="tok-val">{p.slice(eq + 1)}</span>
            </span>
          )
        }
        if (positional.includes(p)) return <span key={i} className="tok-val">{p}</span>
        extra = true
        return <span key={i}>{p}</span>
      })}
    </>
  )
}

export function Spellboard() {
  // 服务端与首帧：第一个场景的最终状态，页面在静止时也是完整的
  const [scene, setScene] = useState(0)
  const [step, setStep] = useState(review.steps.length - 1)

  useEffect(() => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    let s = 0
    let i = -1
    let timer: ReturnType<typeof setTimeout>
    const tick = () => {
      const sc = SCENARIOS[s]!
      i += 1
      if (i >= sc.steps.length) {
        // 停留在完成状态，再切换到下一个场景
        timer = setTimeout(() => {
          s = (s + 1) % SCENARIOS.length
          i = -1
          setScene(s)
          setStep(0)
          tick()
        }, 3200)
        return
      }
      setScene(s)
      setStep(i)
      const cur = sc.steps[i]!
      const pause = cur.text.endsWith(' ') ? 420 : cur.ghost ? 260 : 85
      timer = setTimeout(tick, pause)
    }
    timer = setTimeout(() => {
      i = -1
      setStep(0)
      tick()
    }, 2200)
    return () => clearTimeout(timer)
  }, [])

  const sc = SCENARIOS[scene]!
  const cur = sc.steps[Math.min(step, sc.steps.length - 1)]!
  const list = sc.list(cur.text)

  return (
    <div className="board" aria-label="Qantrip 面板演示：输入命令时实时组装 Prompt">
      <div className="board-input">
        <svg className="wand" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
          <path d="M4 20 15 9M15 9l2-2M18 3v3M21 6h-3M19.5 9.5 21 11M13 4.5 11.5 3" strokeLinecap="round" />
        </svg>
        <span>
          <Tokens text={cur.text} positional={sc.positional} />
          <span className="caret" />
          {cur.ghost && <span className="tok-ghost">{cur.ghost}</span>}
        </span>
      </div>
      <div className="board-body">
        <div className="board-list">
          {list.map((l, i) => (
            <div key={l.label} className={i === 0 ? 'on' : undefined}>
              <span>{l.label}</span>
              <small>{l.hint}</small>
            </div>
          ))}
        </div>
        <div className="board-preview">
          <div className="board-title">
            <b>{sc.title}</b>
            <span className="muted">{sc.description}</span>
          </div>
          <div className="chips">
            {sc.chips(cur.text).map((c) => (
              <span key={c.name} className={c.set ? 'chip set' : 'chip'}>
                <b>{c.name}</b> {c.value}
              </span>
            ))}
          </div>
          <div className="prompt">
            {sc.preview(cur.text).map((p, i) =>
              p.mark ? (
                <mark key={i} className={p.mark === 'extra' ? 'extra' : undefined}>
                  {p.text}
                </mark>
              ) : (
                <span key={i}>{p.text}</span>
              ),
            )}
          </div>
        </div>
      </div>
      <div className="board-foot">
        <span>
          <kbd>↩</kbd>插入 <kbd>⌘↩</kbd>展开粘贴 <kbd>⇥</kbd>补全
        </span>
        <span>预览即将插入的内容</span>
      </div>
    </div>
  )
}
