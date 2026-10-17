import { useEffect, useState } from 'react'
import { type Locale, localeOf, seo, useLocale } from '../lib/i18n'
import { REPO_URL } from '../lib/site'
import type { Route } from './+types/security'

// 安全页（docs/decisions.md D29）：用动画和对照说明端到端加密，细节放在 docs/sync-design.md。

const T = {
  en: {
    title: 'Security — end-to-end encrypted sync · Nekro Qantrip',
    description: "Qantrip sync is end-to-end encrypted. Keys stay on your devices; the server only stores ciphertext. Even on our public service, we can't read your data.",
    h1: 'Only you can read your templates',
    claims: ["When you use our public sync service, even we can't read your data.", 'Everything stored in the cloud is ciphertext. Without your key, it cannot be decrypted.'],
    mac: 'Your Mac',
    cloud: 'Sync server',
    other: 'Your other Mac',
    hasKey: 'has the key',
    noKey: 'no key',
    viewH: 'What each side sees',
    viewYou: 'On your device',
    viewServer: 'On the server',
    cards: [
      { h: 'Keys never leave your devices', p: 'Encryption happens on your Mac. The server never sees a key.' },
      { h: 'New devices need your approval', p: 'Type the code shown on the new device into one you already use. Only then does it get the key.' },
      { h: 'A recovery phrase, just for you', p: '24 words, shown once. Lost every device? They bring your data back. Nobody else can.' },
    ],
    more: 'Want the details?',
    links: [
      ['Full security design', `${REPO_URL}/blob/main/docs/sync-design.md`],
      ['Server source code', `${REPO_URL}/tree/main/apps/cloud`],
      ['Report a vulnerability', `${REPO_URL}/blob/main/SECURITY.md`],
    ],
    selfHost: "Don't want to trust anyone? Deploy the server to your own Cloudflare account and point the app at it.",
    templates: [
      ['review', 'Review the staged changes…'],
      ['commit', 'Write a commit message…'],
      ['explain', 'Explain this code…'],
      ['my-standup', 'Summarize what I did yesterday…'],
    ],
  },
  zh: {
    title: '安全 — 端到端加密同步 · Nekro Qantrip',
    description: 'Qantrip 的同步采用端到端加密：密钥只在你的设备上，服务端只保存密文。即使使用我们的公共服务，我们也无法读取你的数据。',
    h1: '你的模板，只有你能读',
    claims: ['使用我们的公共同步服务时，即使是我们也无法获取你的数据。', '云端保存的一切都是密文，没有你的密钥就无法解密。'],
    mac: '你的 Mac',
    cloud: '同步服务器',
    other: '你的另一台 Mac',
    hasKey: '持有密钥',
    noKey: '没有密钥',
    viewH: '两边各自看到什么',
    viewYou: '你的设备上',
    viewServer: '服务器上',
    cards: [
      { h: '密钥不离开你的设备', p: '加密在你的 Mac 上完成，服务器从来接触不到密钥。' },
      { h: '新设备要你亲自批准', p: '在已有设备上输入新设备显示的校验码，新设备才能拿到密钥。' },
      { h: '只属于你的恢复短语', p: '24 个单词，只显示一次。设备全丢了也能用它找回数据，别人做不到。' },
    ],
    more: '想了解细节？',
    links: [
      ['完整的安全设计', `${REPO_URL}/blob/main/docs/sync-design.md`],
      ['服务端源代码', `${REPO_URL}/tree/main/apps/cloud`],
      ['报告安全漏洞', `${REPO_URL}/blob/main/SECURITY.md`],
    ],
    selfHost: '谁都不想信任？把服务端部署到你自己的 Cloudflare 账号，在应用里切换地址即可。',
    templates: [
      ['review', '请审查暂存区的改动……'],
      ['commit', '根据改动写一条提交信息……'],
      ['explain', '请解释这段代码……'],
      ['my-standup', '总结我昨天做了什么……'],
    ],
  },
} satisfies Record<Locale, unknown>

export const meta: Route.MetaFunction = ({ location }) => {
  const locale = localeOf(location.pathname)
  return seo({ locale, path: '/security', title: T[locale].title, description: T[locale].description })
}

const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'

/** 伪密文：服务端渲染时用固定种子（避免水合不一致），挂载后不断变化 */
function cipher(len: number, seed: number) {
  let s = seed | 0 || 1
  let out = ''
  for (let i = 0; i < len; i++) {
    s ^= s << 13
    s ^= s >>> 17
    s ^= s << 5
    out += ALPHABET[(s >>> 0) % 64]
  }
  return out
}

function useScramble(enabled = true) {
  const [seed, setSeed] = useState(7)
  useEffect(() => {
    if (!enabled || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    const id = setInterval(() => setSeed((s) => (Math.imul(s, 48271) >>> 0) % 2147483647 || 1), 140)
    return () => clearInterval(id)
  }, [enabled])
  return seed
}

const KeyIcon = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
    <circle cx="8" cy="15" r="4" />
    <path d="m11 12 9-9m-4 4 3 3m-6 0 2 2" strokeLinecap="round" />
  </svg>
)
const LockIcon = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
    <rect x="5" y="11" width="14" height="10" rx="2" />
    <path d="M8 11V8a4 4 0 0 1 8 0v3" />
  </svg>
)
const MacIcon = () => (
  <svg width="40" height="40" viewBox="0 0 48 48" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
    <rect x="6" y="9" width="36" height="24" rx="3" />
    <path d="M2 38h44" strokeLinecap="round" />
  </svg>
)
const CloudIcon = () => (
  <svg width="40" height="40" viewBox="0 0 48 48" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
    <path d="M14 36h22a8 8 0 0 0 0-16 12 12 0 0 0-23-2A9 9 0 0 0 14 36Z" strokeLinejoin="round" />
  </svg>
)

function Flow({ t }: { t: (typeof T)[Locale] }) {
  const seed = useScramble()
  const plain = t.templates[Math.floor(seed / 97) % t.templates.length]!
  return (
    <div className="flow" aria-hidden="true">
      <div className="flow-node">
        <span className="flow-icon">
          <MacIcon />
        </span>
        <b>{t.mac}</b>
        <span className="flow-key">
          <KeyIcon /> {t.hasKey}
        </span>
        <code className="flow-text">{plain[1]}</code>
      </div>
      <div className="flow-track">
        <span className="packet" />
        <span className="packet" />
        <span className="packet" />
      </div>
      <div className="flow-node server">
        <span className="flow-icon">
          <CloudIcon />
        </span>
        <b>{t.cloud}</b>
        <span className="flow-key off">
          <LockIcon /> {t.noKey}
        </span>
        <code className="flow-text cipher">{cipher(28, seed)}</code>
      </div>
      <div className="flow-track">
        <span className="packet" />
        <span className="packet" />
        <span className="packet" />
      </div>
      <div className="flow-node">
        <span className="flow-icon">
          <MacIcon />
        </span>
        <b>{t.other}</b>
        <span className="flow-key">
          <KeyIcon /> {t.hasKey}
        </span>
        <code className="flow-text">{plain[1]}</code>
      </div>
    </div>
  )
}

function Views({ t }: { t: (typeof T)[Locale] }) {
  const [server, setServer] = useState(true)
  const seed = useScramble(server)
  return (
    <div className="views">
      <div className="seg" role="tablist">
        <button type="button" role="tab" aria-selected={!server} className={!server ? 'on' : ''} onClick={() => setServer(false)}>
          {t.viewYou}
        </button>
        <button type="button" role="tab" aria-selected={server} className={server ? 'on' : ''} onClick={() => setServer(true)}>
          {t.viewServer}
        </button>
      </div>
      <ul className={server ? 'vlist server' : 'vlist'}>
        {t.templates.map(([name, body], i) => (
          <li key={name}>
            {server ? (
              <>
                <code className="cipher">{cipher(10, seed + i * 31)}</code>
                <span className="cipher">{cipher(36, seed * 3 + i)}</span>
              </>
            ) : (
              <>
                <code>{name}</code>
                <span>{body}</span>
              </>
            )}
          </li>
        ))}
      </ul>
    </div>
  )
}

export default function Security() {
  const locale = useLocale()
  const t = T[locale]
  return (
    <section className="wrap page">
      <header>
        <h1>{t.h1}</h1>
      </header>

      <div className="claims">
        {t.claims.map((c) => (
          <p key={c}>
            <LockIcon />
            {c}
          </p>
        ))}
      </div>

      <Flow t={t} />

      <div className="section-lite">
        <h2>{t.viewH}</h2>
        <Views t={t} />
      </div>

      <ul className="cards3">
        <li>
          <span className="card-visual">
            <KeyIcon />
          </span>
          <b>{t.cards[0]!.h}</b>
          <span>{t.cards[0]!.p}</span>
        </li>
        <li>
          <span className="card-visual sas" aria-hidden="true">
            {'K7Q2 9XMP'.split('').map((c, i) => (
              <i key={i} className={c === ' ' ? 'gap' : undefined}>
                {c === ' ' ? '' : c}
              </i>
            ))}
          </span>
          <b>{t.cards[1]!.h}</b>
          <span>{t.cards[1]!.p}</span>
        </li>
        <li>
          <span className="card-visual words" aria-hidden="true">
            {Array.from({ length: 12 }, (_, i) => (
              <i key={i} />
            ))}
          </span>
          <b>{t.cards[2]!.h}</b>
          <span>{t.cards[2]!.p}</span>
        </li>
      </ul>

      <div className="callout">
        <span>{t.selfHost}</span>
      </div>

      <div className="more">
        <b>{t.more}</b>
        {t.links.map(([label, url]) => (
          <a key={url} href={url}>
            {label} →
          </a>
        ))}
      </div>
    </section>
  )
}
