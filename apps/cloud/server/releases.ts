// 版本信息与安装包下载（docs/decisions.md D23：官网公开，下载暂不公开，由 PUBLIC_DOWNLOADS 开关控制）。
// R2 中的约定：releases/latest.json（发布工作流写入）与 releases/<文件名>。
import { HttpError } from './http.js'

export interface LatestRelease {
  version: string
  published_at: string
  notes_url: string
  files: { arch: 'arm64' | 'x86_64'; name: string; size: number; sha256: string }[]
}

export const downloadsPublic = (env: Env): boolean => env.PUBLIC_DOWNLOADS === '1'

export const latestRelease = async (env: Env): Promise<LatestRelease> => {
  if (!downloadsPublic(env)) throw new HttpError(404, 'downloads_unavailable', '下载即将开放')
  const obj = await env.RELEASES.get('releases/latest.json')
  if (!obj) throw new HttpError(404, 'no_release', '暂无可下载的版本')
  return (await obj.json()) as LatestRelease
}

const FILE = /^Qantrip-\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?-(arm64|x86_64)\.dmg$/

export const downloadFile = async (env: Env, name: string): Promise<Response> => {
  if (!downloadsPublic(env)) throw new HttpError(404, 'downloads_unavailable', '下载即将开放')
  if (!FILE.test(name)) throw new HttpError(404, 'not_found', '文件不存在')
  const obj = await env.RELEASES.get(`releases/${name}`)
  if (!obj) throw new HttpError(404, 'not_found', '文件不存在')
  return new Response(obj.body, {
    headers: {
      'content-type': 'application/x-apple-diskimage',
      'content-length': String(obj.size),
      'content-disposition': `attachment; filename="${name}"`,
      'cache-control': 'public, max-age=86400, immutable',
    },
  })
}
