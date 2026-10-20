// 版本信息与安装包下载（docs/decisions.md D23：官网公开，下载暂不公开，由 PUBLIC_DOWNLOADS 开关控制）。
// R2 中的约定：releases/latest.json（发布工作流写入）与 releases/<文件名>。
import { assetName, type ClientId } from '../app/lib/assets.js'
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

// 客户端的新版本提示与 /dl/ 跳转：代为读取 GitHub 最新正式版本并缓存，国内网络访问 api.github.com 经常失败。
// 缓存 10 分钟（新版本发布后很快可见）；GitHub 不可用时退回最近一次成功的结果（保留 7 天）。
// 缓存按 Cloudflare 机房各自独立，没有缓存的机房必须现查：先用网页的 /releases/latest 跳转取版本号——
// api.github.com 对未登录请求按出口 IP 每小时限 60 次，Workers 的出口 IP 由大量用户共用，经常被限流。
export interface PublicRelease {
  version: string
  tag: string
  url: string
  published_at: string
}

const GITHUB_LATEST = 'https://api.github.com/repos/NekroAI/qantrip/releases/latest'
/** GitHub 网页上的最新正式版本（302 跳到 /releases/tag/<标签>），也用作 /dl/ 的最后退路 */
export const RELEASES_LATEST_PAGE = 'https://github.com/NekroAI/qantrip/releases/latest'
const REPO_RELEASES = 'https://github.com/NekroAI/qantrip/releases'

/** 从 /releases/latest 的跳转地址中取出版本标签（只接受 vX.Y.Z 形式） */
export const tagFromLatestLocation = (location: string | null): string | null => {
  const m = location?.match(/\/NekroAI\/qantrip\/releases\/tag\/(v\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)$/)
  return m?.[1] ?? null
}

const fromWebRedirect = async (): Promise<PublicRelease> => {
  const res = await fetch(RELEASES_LATEST_PAGE, { redirect: 'manual', headers: { 'user-agent': 'nekro-qantrip' } })
  const tag = tagFromLatestLocation(res.headers.get('location'))
  if (!tag) throw new Error(`github web ${res.status}`)
  return { version: tag.replace(/^v/, ''), tag, url: `${REPO_RELEASES}/tag/${tag}`, published_at: '' }
}

const fromApi = async (): Promise<PublicRelease> => {
  const res = await fetch(GITHUB_LATEST, { headers: { accept: 'application/vnd.github+json', 'user-agent': 'nekro-qantrip' } })
  if (!res.ok) throw new Error(`github ${res.status}`)
  const j = (await res.json()) as { tag_name?: string; html_url?: string; published_at?: string; draft?: boolean; prerelease?: boolean }
  if (!j.tag_name || !j.html_url || j.draft || j.prerelease) throw new Error('no release')
  return { version: j.tag_name.replace(/^v/, ''), tag: j.tag_name, url: j.html_url, published_at: j.published_at ?? '' }
}
const FRESH_KEY = 'https://qantrip.internal/release/latest'
const STALE_KEY = 'https://qantrip.internal/release/latest-stale'

const cached = (body: PublicRelease, maxAge: number) =>
  new Response(JSON.stringify(body), { headers: { 'content-type': 'application/json', 'cache-control': `public, max-age=${maxAge}` } })

export const publicLatestRelease = async (ctx: { waitUntil(p: Promise<unknown>): void }): Promise<PublicRelease> => {
  const cache = (caches as unknown as { default: Cache }).default
  const fresh = await cache.match(FRESH_KEY)
  if (fresh) return (await fresh.json()) as PublicRelease
  try {
    const body = await fromWebRedirect().catch(fromApi)
    ctx.waitUntil(Promise.all([cache.put(FRESH_KEY, cached(body, 600)), cache.put(STALE_KEY, cached(body, 7 * 86400))]))
    return body
  } catch {
    const stale = await cache.match(STALE_KEY)
    if (stale) return (await stale.json()) as PublicRelease
    throw new HttpError(502, 'release_unavailable', '暂时无法获取最新版本')
  }
}

/** 最新正式版本中某个平台安装包的下载地址 */
export const releaseAssetUrl = (r: PublicRelease, id: ClientId) => `https://github.com/NekroAI/qantrip/releases/download/${r.tag}/${assetName(id, r.version)}`
