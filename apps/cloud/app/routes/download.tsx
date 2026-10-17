import { Link } from 'react-router'
import { REPO_URL } from '../lib/site'
import type { Route } from './+types/download'

export const meta: Route.MetaFunction = () => [{ title: '下载 · Nekro Qantrip' }]

const LATEST = `${REPO_URL}/releases/latest`

export default function Download() {
  return (
    <section className="wrap page">
      <header>
        <h1>下载 Qantrip for macOS</h1>
        <p>需要 macOS 14 或更新版本。下载 DMG，打开后把 Qantrip 拖进「应用程序」即可。</p>
      </header>
      <div className="card">
        <div className="list">
          <div className="list-item">
            <span>
              Apple 芯片（M 系列） <span className="muted">· Qantrip-…-arm64.dmg</span>
            </span>
            <a className="btn primary small" href={LATEST}>
              下载 arm64
            </a>
          </div>
          <div className="list-item">
            <span>
              Intel 芯片 <span className="muted">· Qantrip-…-x86_64.dmg</span>
            </span>
            <a className="btn small" href={LATEST}>
              下载 x86_64
            </a>
          </div>
        </div>
        <p className="muted" style={{ fontSize: 14 }}>
          不确定是哪种芯片：苹果菜单 → 关于本机，查看「芯片」一栏。Windows 与 Linux 版本正在开发。
        </p>
      </div>
      <div className="prose">
        <p>
          Qantrip 尚未经过 Apple 公证，首次打开会被系统拦截：到 <strong>系统设置 → 隐私与安全性</strong> 点 <strong>仍要打开</strong>。之后更新会保留权限与模板。
        </p>
        <p>
          使用方法见 <Link to="/docs">文档</Link>。
        </p>
      </div>
    </section>
  )
}
