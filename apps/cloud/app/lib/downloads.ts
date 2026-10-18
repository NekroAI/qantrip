// 下载（docs/decisions.md D38）：链接一律指向官网的固定地址 /dl/<平台标识>，由服务端跳转到最新版本的发布文件。
import { useEffect, useState } from 'react'
import { type ClientId, downloadPath } from './assets'
import type { Locale } from './i18n'
import { REPO_URL } from './site'

export type { ClientId }

export interface Client {
  id: ClientId
  os: 'macos' | 'windows' | 'linux'
  url: string
  name: Record<Locale, string>
  detail: Record<Locale, string>
  /** 首页下载按钮的下拉菜单中显示（其余只在下载页列出） */
  featured: boolean
}

export const CLIENTS: Client[] = [
  {
    id: 'macos-arm64',
    os: 'macos',
    url: downloadPath('macos-arm64'),
    name: { en: 'macOS · Apple silicon', zh: 'macOS · Apple 芯片' },
    detail: { en: 'M1 and later · macOS 14+', zh: 'M1 及更新 · macOS 14+' },
    featured: true,
  },
  {
    id: 'macos-x64',
    os: 'macos',
    url: downloadPath('macos-x64'),
    name: { en: 'macOS · Intel', zh: 'macOS · Intel 芯片' },
    detail: { en: 'Intel Macs · macOS 14+', zh: 'Intel 芯片的 Mac · macOS 14+' },
    featured: true,
  },
  {
    id: 'windows-x64',
    os: 'windows',
    url: downloadPath('windows-x64'),
    name: { en: 'Windows · x64', zh: 'Windows · x64' },
    detail: { en: 'Windows 10 2004+ · installer, no admin rights', zh: 'Windows 10 2004+ · 安装程序，无需管理员权限' },
    featured: true,
  },
  {
    id: 'windows-zip-x64',
    os: 'windows',
    url: downloadPath('windows-zip-x64'),
    name: { en: 'Windows · x64 portable', zh: 'Windows · x64 便携版' },
    detail: { en: 'Unzip and run Qantrip.exe', zh: '解压后运行 Qantrip.exe' },
    featured: true,
  },
  ...linux('x64'),
  ...linux('arm64'),
]

/** Linux 预览版（docs/decisions.md D37）：AppImage 通用，另有 deb（Debian / Ubuntu）与 rpm（Fedora 等） */
function linux(arch: 'x64' | 'arm64'): Client[] {
  const cpu = arch === 'x64' ? 'x64' : 'ARM64'
  const req = { en: 'GTK 4.14+ and libadwaita 1.5+ (Ubuntu 24.04, Debian 13, Fedora 40 or later)', zh: '需要 GTK 4.14+ 与 libadwaita 1.5+（Ubuntu 24.04、Debian 13、Fedora 40 或更新）' }
  const pkg = (id: ClientId, en: string, zh: string, featured: boolean): Client => ({
    id,
    os: 'linux',
    url: downloadPath(id),
    name: { en: `Linux · ${cpu} ${en} (preview)`, zh: `Linux · ${cpu} ${zh}（预览版）` },
    detail: req,
    featured,
  })
  return [
    pkg(`linux-${arch}`, 'AppImage', 'AppImage', true),
    pkg(`linux-deb-${arch}`, '.deb', '.deb', false),
    pkg(`linux-rpm-${arch}`, '.rpm', '.rpm', false),
  ]
}

export const RELEASES_URL = `${REPO_URL}/releases`
export const client = (id: ClientId) => CLIENTS.find((c) => c.id === id)!

/** 在浏览器中识别访客的系统与芯片。Safari 不暴露 CPU 架构，用 WebGL 渲染器名称判断 Apple 芯片；判断不了时按 Apple 芯片处理。 */
export function detectClient(): ClientId {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } }
  const ua = navigator.userAgent
  const platform = nav.userAgentData?.platform ?? navigator.platform ?? ''
  if (/Win/i.test(platform) || /Windows/i.test(ua)) return 'windows-x64'
  if (/Linux|X11/i.test(platform) && !/Android/i.test(ua)) {
    // 浏览器只在 UA 中透露 CPU：aarch64 / arm64；判断不了时按 x64 处理
    return /aarch64|arm64|armv8/i.test(`${platform} ${ua}`) ? 'linux-arm64' : 'linux-x64'
  }
  if (/Mac/i.test(platform) || /Mac OS X/i.test(ua)) {
    try {
      const gl = document.createElement('canvas').getContext('webgl')
      const ext = gl?.getExtension('WEBGL_debug_renderer_info')
      const renderer = ext && gl ? String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)) : ''
      if (/Intel|AMD|Radeon|NVIDIA/i.test(renderer)) return 'macos-x64'
    } catch {
      // 拿不到渲染器信息时按 Apple 芯片处理
    }
  }
  return 'macos-arm64'
}

/** 首次渲染（含服务端渲染）用默认值，挂载后换成识别结果，避免水合不一致 */
export function useDetectedClient(): { id: ClientId; detected: boolean } {
  const [state, setState] = useState<{ id: ClientId; detected: boolean }>({ id: 'macos-arm64', detected: false })
  useEffect(() => setState({ id: detectClient(), detected: true }), [])
  return state
}

/** 演示中的唤起快捷键：Mac 上 ⌃Q，Windows 上 Ctrl+Q（与应用一致），Linux 上 Alt+Q（Linux 浏览器中 Ctrl+Q 是退出） */
export function useSummonKey(): 'mac' | 'ctrl' | 'alt' {
  const [kind, setKind] = useState<'mac' | 'ctrl' | 'alt'>('mac')
  useEffect(() => {
    const p = navigator.platform || navigator.userAgent
    setKind(/Mac|iPhone|iPad/i.test(p) ? 'mac' : /Linux|X11|CrOS/i.test(p) && !/Android/i.test(navigator.userAgent) ? 'alt' : 'ctrl')
  }, [])
  return kind
}
