import { Link } from 'react-router'
import { publicLatestRelease } from '../../server/releases.js'
import { assetName } from '../lib/assets'
import { CLIENTS, RELEASES_URL, client, useDetectedClient } from '../lib/downloads'
import { type Locale, localeOf, seo, useHref, useLocale } from '../lib/i18n'
import type { Route } from './+types/download'

const T = {
  en: {
    title: 'Download Qantrip for macOS, Windows and Linux · Nekro Qantrip',
    description: 'Download Nekro Qantrip, the open-source prompt launcher, for macOS 14+ (Apple silicon or Intel), Windows 10/11 and Linux (preview: AppImage, .deb, .rpm). Free download from GitHub Releases.',
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
    stepsLinux: [
      'AppImage: make it executable (chmod +x, or Properties → Allow executing) and open it. Or install the .deb / .rpm with your package manager.',
      'On GNOME or KDE (Wayland), confirm the shortcut when the system asks. The welcome guide lets you allow auto-paste once; without it, prompts are copied for you to paste.',
      'Press Ctrl+Q anywhere.',
    ],
    chip: 'Not sure which chip? Apple menu → About This Mac.',
    history: 'Release notes and older versions',
    docs: 'Read the docs',
  },
  zh: {
    title: '下载 Qantrip（macOS、Windows、Linux）· Nekro Qantrip',
    description: '下载开源 Prompt 启动器 Nekro Qantrip，支持 macOS 14+（Apple 芯片与 Intel 芯片）、Windows 10/11 与 Linux（预览版：AppImage、deb、rpm）。直接从 GitHub Releases 免费下载。',
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
    stepsLinux: [
      'AppImage：设为可执行（chmod +x，或在文件属性中允许执行）后打开；也可以用软件包管理器安装 deb / rpm。',
      'GNOME、KDE（Wayland）首次启动时系统会询问是否允许注册快捷键，确认即可。引导中可以授权一次自动粘贴；不授权时 Prompt 会复制到剪贴板。',
      '在任何地方按 Ctrl+Q。',
    ],
    chip: '不确定是哪种芯片：苹果菜单 → 关于本机。',
    history: '更新记录与历史版本',
    docs: '阅读文档',
  },
} satisfies Record<Locale, unknown>

export const meta: Route.MetaFunction = ({ location }) => {
  const locale = localeOf(location.pathname)
  return seo({ locale, path: '/download', title: T[locale].title, description: T[locale].description })
}

/** 最新版本号，用于显示文件名；取不到时只显示不带版本号的名称 */
export const loader = async () => {
  const pending: Promise<unknown>[] = []
  try {
    const release = await publicLatestRelease({ waitUntil: (p) => pending.push(p) })
    await Promise.all(pending)
    return { version: release.version }
  } catch {
    return { version: null }
  }
}

export default function Download({ loaderData }: Route.ComponentProps) {
  const { version } = loaderData
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
            {best.detail[locale]} · {assetName(best.id, version)}
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
                  {assetName(c.id, version)}
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
          {(best.os === 'windows' ? t.stepsWin : best.os === 'linux' ? t.stepsLinux : t.steps).map((s, i) => (
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
