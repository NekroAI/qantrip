import { Link } from 'react-router'
import { CLIENTS, RELEASES_URL, client, useDetectedClient } from '../lib/downloads'
import { type Locale, localeOf, seo, useHref, useLocale } from '../lib/i18n'
import type { Route } from './+types/download'

const T = {
  en: {
    title: 'Download Qantrip for macOS · Nekro Qantrip',
    description: 'Download Nekro Qantrip, the open-source prompt launcher, for macOS 14+ (Apple silicon or Intel) and Windows 10/11 (preview). Free download from GitHub Releases.',
    h1: 'Download Qantrip',
    lead: 'Free and open source. The latest version, straight from GitHub Releases.',
    forYou: 'For your computer',
    detecting: 'Recommended',
    download: 'Download',
    soon: 'Not yet',
    all: 'All platforms',
    notAvailable: (os: string) => `Qantrip isn't available for ${os} yet. Here's the macOS version.`,
    stepsH: 'Then',
    steps: [
      'Open the DMG and drag Qantrip into Applications.',
      'First launch: System Settings → Privacy & Security → Open Anyway. (Qantrip is not notarized by Apple yet.)',
      'Press ⌃Q anywhere.',
    ],
    stepsWin: ['Run the installer. It isn\'t code-signed yet: if SmartScreen warns you, click More info → Run anyway.', 'Follow the steps; no admin rights needed.', 'Press Ctrl+Q anywhere.'],
    chip: 'Not sure which chip? Apple menu → About This Mac.',
    history: 'Release notes and older versions',
    docs: 'Read the docs',
  },
  zh: {
    title: '下载 Qantrip macOS 版 · Nekro Qantrip',
    description: '下载开源 Prompt 启动器 Nekro Qantrip，支持 macOS 14+（Apple 芯片与 Intel 芯片）与 Windows 10/11（预览版）。直接从 GitHub Releases 免费下载。',
    h1: '下载 Qantrip',
    lead: '免费、开源。最新版本，直接从 GitHub Releases 下载。',
    forYou: '适合你的电脑',
    detecting: '推荐',
    download: '下载',
    soon: '暂未推出',
    all: '全部平台',
    notAvailable: (os: string) => `Qantrip 还没有 ${os} 版本，这里先给出 macOS 版。`,
    stepsH: '下载之后',
    steps: ['打开 DMG，把 Qantrip 拖进「应用程序」。', '首次打开：系统设置 → 隐私与安全性 → 仍要打开（Qantrip 尚未经过 Apple 公证）。', '在任何地方按 ⌃Q。'],
    stepsWin: ['运行安装程序。程序暂未签名：如果 SmartScreen 拦截，点「更多信息 → 仍要运行」。', '按提示完成安装，不需要管理员权限。', '在任何地方按 Ctrl+Q。'],
    chip: '不确定是哪种芯片：苹果菜单 → 关于本机。',
    history: '更新记录与历史版本',
    docs: '阅读文档',
  },
} satisfies Record<Locale, unknown>

export const meta: Route.MetaFunction = ({ location }) => {
  const locale = localeOf(location.pathname)
  return seo({ locale, path: '/download', title: T[locale].title, description: T[locale].description })
}

export default function Download() {
  const locale = useLocale()
  const t = T[locale]
  const href = useHref()
  const { id } = useDetectedClient()
  const detected = client(id)
  const best = detected.url ? detected : client('macos-arm64')

  return (
    <section className="wrap page">
      <header>
        <h1>{t.h1}</h1>
        <p>{t.lead}</p>
      </header>

      <div className="dl-hero">
        <div>
          <span className="label">{t.forYou}</span>
          <b>{best.name[locale]}</b>
          <span className="muted">
            {best.detail[locale]} · {best.file}
          </span>
          {!detected.url && <span className="muted">{t.notAvailable(detected.name[locale])}</span>}
        </div>
        <a className="btn primary" href={best.url!} download>
          {t.download}
        </a>
      </div>

      <div className="dsec">
        <span className="label">{t.all}</span>
        <ul className="dl-list">
          {CLIENTS.map((c) => (
            <li key={c.id} className={c.url ? undefined : 'off'}>
              <span>
                <b>{c.name[locale]}</b>
                <span className="muted">{c.detail[locale]}</span>
              </span>
              {c.url ? (
                <a className="btn small" href={c.url} download>
                  {c.file}
                </a>
              ) : (
                <span className="muted">{t.soon}</span>
              )}
            </li>
          ))}
        </ul>
        {best.os === 'macos' && <p className="muted small">{t.chip}</p>}
      </div>

      <div className="dsec">
        <span className="label">{t.stepsH}</span>
        <ol className="install">
          {(best.os === 'windows' ? t.stepsWin : t.steps).map((s, i) => (
            <li key={s}>
              <span className="step-n">{i + 1}</span>
              <span>{s}</span>
            </li>
          ))}
        </ol>
      </div>

      <p className="links-row">
        <a href={RELEASES_URL}>{t.history} →</a>
        <Link to={href('/docs')}>{t.docs} →</Link>
      </p>
    </section>
  )
}
