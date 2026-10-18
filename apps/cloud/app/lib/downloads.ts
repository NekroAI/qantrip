// 下载直链（docs/decisions.md D29）：发布工作流在每个 Release 里上传固定文件名的副本，
// releases/latest/download/<文件名> 始终指向最新正式版本。
import { useEffect, useState } from 'react'
import type { Locale } from './i18n'
import { REPO_URL } from './site'

export type ClientId = 'macos-arm64' | 'macos-x86_64' | 'windows' | 'linux'

export interface Client {
  id: ClientId
  os: 'macos' | 'windows' | 'linux'
  file: string | null
  url: string | null
  name: Record<Locale, string>
  detail: Record<Locale, string>
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
  },
  {
    id: 'macos-x86_64',
    os: 'macos',
    file: 'Qantrip-macOS-x86_64.dmg',
    url: latest('Qantrip-macOS-x86_64.dmg'),
    name: { en: 'macOS · Intel', zh: 'macOS · Intel 芯片' },
    detail: { en: 'Intel Macs · macOS 14+', zh: 'Intel 芯片的 Mac · macOS 14+' },
  },
  {
    id: 'windows',
    os: 'windows',
    file: 'Qantrip-Windows-x64.zip',
    url: latest('Qantrip-Windows-x64.zip'),
    name: { en: 'Windows · x64 (preview)', zh: 'Windows · x64（预览版）' },
    detail: { en: 'Windows 10 2004+ · unzip and run', zh: 'Windows 10 2004+ · 解压即用' },
  },
  { id: 'linux', os: 'linux', file: null, url: null, name: { en: 'Linux', zh: 'Linux' }, detail: { en: 'Planned', zh: '计划中' } },
]

export const RELEASES_URL = `${REPO_URL}/releases`
export const client = (id: ClientId) => CLIENTS.find((c) => c.id === id)!

/** 在浏览器中识别访客的系统与芯片。Safari 不暴露 CPU 架构，用 WebGL 渲染器名称判断 Apple 芯片；判断不了时按 Apple 芯片处理。 */
export function detectClient(): ClientId {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } }
  const ua = navigator.userAgent
  const platform = nav.userAgentData?.platform ?? navigator.platform ?? ''
  if (/Win/i.test(platform) || /Windows/i.test(ua)) return 'windows'
  if (/Linux|X11/i.test(platform) && !/Android/i.test(ua)) return 'linux'
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
