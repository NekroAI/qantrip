import { env } from 'cloudflare:workers'
import { Link } from 'react-router'
import { downloadsPublic, latestRelease, type LatestRelease } from '../../server/releases.js'
import { REPO_URL } from '../lib/site'
import type { Route } from './+types/download'

export const meta: Route.MetaFunction = () => [{ title: '下载 · Nekro Qantrip' }]

export const loader = async () => {
  if (!downloadsPublic(env)) return { release: null as LatestRelease | null, open: false }
  try {
    return { release: await latestRelease(env), open: true }
  } catch {
    return { release: null, open: true }
  }
}

const SIZE = (n: number) => `${(n / 1048576).toFixed(1)} MB`

export default function Download({ loaderData }: Route.ComponentProps) {
  const { release, open } = loaderData
  return (
    <section className="wrap page">
      <header>
        <h1>下载 Qantrip for macOS</h1>
        <p>需要 macOS 14 或更新版本。Apple 芯片与 Intel 芯片分别提供安装包。</p>
      </header>
      {release ? (
        <div className="card">
          <div className="row">
            <strong>版本 {release.version}</strong>
            <a href={release.notes_url}>更新说明</a>
          </div>
          <div className="list">
            {release.files.map((f) => (
              <div className="list-item" key={f.name}>
                <span>
                  {f.arch === 'arm64' ? 'Apple 芯片（M1 及以后）' : 'Intel 芯片'} · <span className="muted">{SIZE(f.size)}</span>
                </span>
                <a className="btn primary small" href={`/download/${f.name}`}>
                  下载 {f.arch}
                </a>
              </div>
            ))}
          </div>
        </div>
      ) : (
        <div className="callout">
          <strong>{open ? '暂无可下载的版本' : '公开下载尚未开放'}</strong>
          <span className="muted">
            Qantrip 目前处于内部测试阶段。你可以先从 <a href={REPO_URL}>源代码</a> 构建；Windows 与 Linux 版本正在开发。
          </span>
        </div>
      )}
      <div className="prose">
        <h2>首次打开</h2>
        <p>
          Qantrip 使用项目自己的证书签名，尚未经过 Apple 公证。首次打开会被系统拦截：在 <strong>系统设置 → 隐私与安全性</strong> 页面底部点 <strong>仍要打开</strong>。不要为此关闭 Gatekeeper。
        </p>
        <p>
          完整步骤见 <Link to="/docs#install">安装说明</Link>。
        </p>
      </div>
    </section>
  )
}
