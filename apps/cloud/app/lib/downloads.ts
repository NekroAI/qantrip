// 下载直链（docs/decisions.md D29）：发布工作流在每个 Release 里上传固定文件名的副本，
// releases/latest/download/<文件名> 始终指向最新正式版本。
import { useEffect, useState } from 'react'
import type { Locale } from './i18n'
import { REPO_URL } from './site'

export type ClientId =
  | 'macos-arm64'
  | 'macos-x86_64'
  | 'windows'
  | 'windows-zip'
  | 'linux-x86_64'
  | 'linux-aarch64'
  | 'linux-deb-x86_64'
  | 'linux-deb-aarch64'
  | 'linux-rpm-x86_64'
  | 'linux-rpm-aarch64'

export interface Client {
  id: ClientId
  os: 'macos' | 'windows' | 'linux'
  file: string | null
  url: string | null
  name: Record<Locale, string>
  detail: Record<Locale, string>
  /** 首页下载按钮的下拉菜单中显示（其余只在下载页列出） */
  featured: boolean
}

const latest = (file: string) => `${REPO_URL}/releases/latest/download/${file}`

export const CLIENTS: Client[] = [
  {
    id: 'macos-arm64',
    os: 'macos',
    file: 'Qantrip-macOS-arm64.dmg',
    url: latest('Qantrip-macOS-arm64.dmg'),
    name: { en: 'macOS · Apple silicon', zh: 'macOS · Apple 芯片' },
    detail: { en: 'M1 and later · macOS 14+', zh: 'M1 及更新 · macOS 14+' },
    featured: true,
  },
  {
    id: 'macos-x86_64',
    os: 'macos',
    file: 'Qantrip-macOS-x86_64.dmg',
    url: latest('Qantrip-macOS-x86_64.dmg'),
    name: { en: 'macOS · Intel', zh: 'macOS · Intel 芯片' },
    detail: { en: 'Intel Macs · macOS 14+', zh: 'Intel 芯片的 Mac · macOS 14+' },
    featured: true,
  },
  {
    id: 'windows',
    os: 'windows',
    file: 'Qantrip-Windows-x64-Setup.exe',
    url: latest('Qantrip-Windows-x64-Setup.exe'),
    name: { en: 'Windows · x64', zh: 'Windows · x64' },
    detail: { en: 'Windows 10 2004+ · installer, no admin rights', zh: 'Windows 10 2004+ · 安装程序，无需管理员权限' },
    featured: true,
  },
  {
    id: 'windows-zip',
    os: 'windows',
    file: 'Qantrip-Windows-x64.zip',
    url: latest('Qantrip-Windows-x64.zip'),
    name: { en: 'Windows · x64 portable', zh: 'Windows · x64 便携版' },
    detail: { en: 'Unzip and run Qantrip.exe', zh: '解压后运行 Qantrip.exe' },
    featured: true,
  },
  ...linux('x86_64'),
  ...linux('aarch64'),
]

/** Linux 预览版（docs/decisions.md D37）：AppImage 通用，另有 deb（Debian / Ubuntu）与 rpm（Fedora 等） */
function linux(arch: 'x86_64' | 'aarch64'): Client[] {
  const cpu = arch === 'x86_64' ? 'x86_64' : 'ARM64'
  const req = { en: 'GTK 4.14+ and libadwaita 1.5+ (Ubuntu 24.04, Debian 13, Fedora 40 or later)', zh: '需要 GTK 4.14+ 与 libadwaita 1.5+（Ubuntu 24.04、Debian 13、Fedora 40 或更新）' }
  const pkg = (id: ClientId, ext: string, en: string, zh: string, featured: boolean): Client => ({
    id,
    os: 'linux',
    file: `Qantrip-Linux-${arch}.${ext}`,
    url: latest(`Qantrip-Linux-${arch}.${ext}`),
    name: { en: `Linux · ${cpu} ${en} (preview)`, zh: `Linux · ${cpu} ${zh}（预览版）` },
    detail: req,
    featured,
  })
  return [
    pkg(`linux-${arch}`, 'AppImage', 'AppImage', 'AppImage', true),
    pkg(`linux-deb-${arch}`, 'deb', '.deb', '.deb', false),
    pkg(`linux-rpm-${arch}`, 'rpm', '.rpm', '.rpm', false),
  ]
}

export const RELEASES_URL = `${REPO_URL}/releases`
export const client = (id: ClientId) => CLIENTS.find((c) => c.id === id)!

/** 在浏览器中识别访客的系统与芯片。Safari 不暴露 CPU 架构，用 WebGL 渲染器名称判断 Apple 芯片；判断不了时按 Apple 芯片处理。 */
export function detectClient(): ClientId {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } }
  const ua = navigator.userAgent
  const platform = nav.userAgentData?.platform ?? navigator.platform ?? ''
  if (/Win/i.test(platform) || /Windows/i.test(ua)) return 'windows'
  if (/Linux|X11/i.test(platform) && !/Android/i.test(ua)) {
    // 浏览器只在 UA 中透露 CPU：aarch64 / arm64；判断不了时按 x86_64 处理
    return /aarch64|arm64|armv8/i.test(`${platform} ${ua}`) ? 'linux-aarch64' : 'linux-x86_64'
  }
  if (/Mac/i.test(platform) || /Mac OS X/i.test(ua)) {
    try {
      const gl = document.createElement('canvas').getContext('webgl')
      const ext = gl?.getExtension('WEBGL_debug_renderer_info')
      const renderer = ext && gl ? String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)) : ''
      if (/Intel|AMD|Radeon|NVIDIA/i.test(renderer)) return 'macos-x86_64'
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
