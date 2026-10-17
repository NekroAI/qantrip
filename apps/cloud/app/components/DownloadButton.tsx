import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { CLIENTS, client, useDetectedClient } from '../lib/downloads'
import { useHref, useLocale } from '../lib/i18n'

const T = {
  en: { download: 'Download for', notYet: 'is not available yet', mac: 'Get the macOS version', others: 'Other platforms', all: 'All downloads' },
  zh: { download: '下载', notYet: '版本尚未推出', mac: '下载 macOS 版', others: '其他平台', all: '全部下载' },
}

/** 首页一键下载：识别访客系统给出直链，旁边的箭头展开其他客户端 */
export function DownloadButton() {
  const locale = useLocale()
  const t = T[locale]
  const href = useHref()
  const { id } = useDetectedClient()
  const [open, setOpen] = useState(false)
  const box = useRef<HTMLDivElement>(null)
  const current = client(id)

  useEffect(() => {
    if (!open) return
    const close = (e: MouseEvent | KeyboardEvent) => {
      if (e instanceof KeyboardEvent ? e.key === 'Escape' : !box.current?.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', close)
    document.addEventListener('keydown', close)
    return () => {
      document.removeEventListener('mousedown', close)
      document.removeEventListener('keydown', close)
    }
  }, [open])

  // 访客的系统还没有客户端：主按钮改为 macOS 版，并说明
  const primary = current.url ? current : client('macos-arm64')
  const label = current.url ? `${t.download} ${current.name[locale]}` : t.mac

  return (
    <div className="dl" ref={box}>
      <div className="dl-split">
        <a className="btn primary dl-main" href={primary.url!} download>
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
            <path d="M8 2v8m0 0 3-3M8 10 5 7M3 13h10" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          {label}
        </a>
        <button type="button" className="btn primary dl-more" aria-haspopup="true" aria-expanded={open} aria-label={t.others} onClick={() => setOpen((o) => !o)}>
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path d="m3 4.5 3 3 3-3" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
          </svg>
        </button>
      </div>
      {!current.url && (
        <span className="dl-note">
          {current.name[locale]} {t.notYet}
        </span>
      )}
      {open && (
        <div className="dl-menu" role="menu">
          {CLIENTS.map((c) =>
            c.url ? (
              <a key={c.id} role="menuitem" href={c.url} download className={c.id === id ? 'cur' : undefined}>
                <b>{c.name[locale]}</b>
                <span>{c.detail[locale]}</span>
              </a>
            ) : (
              <span key={c.id} role="menuitem" aria-disabled="true" className="off">
                <b>{c.name[locale]}</b>
                <span>{c.detail[locale]}</span>
              </span>
            ),
          )}
          <Link role="menuitem" to={href('/download')} className="all" onClick={() => setOpen(false)}>
            {t.all} →
          </Link>
        </div>
      )}
    </div>
  )
}
