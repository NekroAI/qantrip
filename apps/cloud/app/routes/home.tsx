import { Link } from 'react-router'
import { Spellboard } from '../components/Spellboard'
import { BUILTIN_TEMPLATES } from '../lib/templates'
import type { Route } from './+types/home'

export const meta: Route.MetaFunction = () => [
  { title: 'Nekro Qantrip · 按 Q 施放一个戏法' },
  { name: 'description', content: '一份模板库，在任何 AI 工具里用几个按键调出带参数提示的高质量 Prompt。开源、本地优先、端到端加密同步。' },
  { property: 'og:title', content: 'Nekro Qantrip' },
  { property: 'og:description', content: '按 Q 施放一个戏法：在任何 AI 工具里用几个按键调出高质量 Prompt。' },
  { property: 'og:image', content: '/media/icon-256.png' },
]

const FEATURES = [
  {
    title: '参数化模板',
    body: (
      <>
        开关、单选、多选、文本、数字。<code>r security --scope=staged -s</code> 与在面板里点选参数得到完全相同的结果。
      </>
    ),
  },
  { title: '看得见的组装', body: <>命令行实时着色，灰字给出补全，右侧预览把每个参数带来的变化标亮。回车之前就知道会插入什么。</> },
  { title: '可靠插入', body: <>剪贴板加粘贴，多行不会被终端误提交；粘贴后还原剪贴板，Prompt 不进入剪贴板管理器历史。没有权限时退回只复制。</> },
  { title: '本地优先', body: <>模板是普通的 Markdown 文件，离线可用；用任何编辑器修改，应用自动重新加载。</> },
  {
    title: (
      <>
        端到端加密同步<span className="tag">即将推出</span>
      </>
    ),
    body: (
      <>
        加密只在你的设备上完成，服务端只保存无法解密的密文。<Link to="/security">查看安全设计</Link>
      </>
    ),
  },
  { title: '为 AI 编程代理而生', body: <>Claude Code、Codex、Cursor、终端、浏览器都能用。⌘↩ 让 Claude Code 中被折叠的长粘贴自动展开。</> },
]

export default function Home() {
  return (
    <>
      <section className="wrap hero">
        <div className="hero-copy">
          <span className="eyebrow">Nekro Qantrip · Prompt 戏法</span>
          <h1 className="display">
            按 <em>Q</em> 施放一个戏法
          </h1>
          <p className="lead">
            一份模板库，在任何 AI 工具里用几个按键调出带参数提示的高质量 Prompt。Qantrip 读作 cantrip——D&amp;D 里不耗资源、随手就能施放的小法术。
          </p>
          <div className="hero-actions">
            <Link className="btn primary" to="/download">
              下载 macOS 版
            </Link>
            <a className="btn" href="#demo">
              观看演示
            </a>
            <span className="muted">macOS 14+ · Windows 与 Linux 筹备中</span>
          </div>
        </div>
        <Spellboard />
      </section>

      <section id="demo" className="wrap section">
        <div className="section-head">
          <span className="eyebrow">实机录屏</span>
          <h2 className="display">从唤起到插入，十几秒</h2>
          <p className="lead">在终端里按下 ⌃Q，输入命令、点选参数、写一句附加要求，回车后面板化作星光淡出，Prompt 落进终端。</p>
        </div>
        <div className="media-frame">
          <video src="/media/cast.mp4" poster="/media/cast-poster.jpg" muted loop playsInline controls preload="metadata" aria-label="唤起面板、输入命令并插入到终端" />
        </div>
      </section>

      <section className="wrap section">
        <div className="section-head">
          <span className="eyebrow">能力</span>
          <h2 className="display">快，而且每一步都看得清</h2>
        </div>
        <div className="features">
          {FEATURES.map((f, i) => (
            <div className="feature" key={i}>
              <h3>{f.title}</h3>
              <p>{f.body}</p>
            </div>
          ))}
        </div>
      </section>

      <section className="wrap section">
        <div className="section-head">
          <span className="eyebrow">内置模板</span>
          <h2 className="display">{BUILTIN_TEMPLATES.length} 个开箱即用的戏法</h2>
          <p className="lead">覆盖代码审查、调试、测试、重构、提交信息与翻译。每个都可以自定义，也可以写自己的。</p>
        </div>
        <div className="templates">
          {BUILTIN_TEMPLATES.map((t) => (
            <article key={t.name}>
              <h3>
                {t.name}
                {t.aliases.length > 0 && <span>{t.aliases.join(' · ')}</span>}
              </h3>
              <p>{t.description}</p>
              {t.params.length > 0 && <div className="params">{t.params.map((p) => `--${p}`).join('  ')}</div>}
            </article>
          ))}
        </div>
      </section>

      <section className="wrap section">
        <div className="section-head">
          <span className="eyebrow">隐私</span>
          <h2 className="display">你的模板，只有你的设备能读</h2>
          <p className="lead">
            同步采用端到端加密：密钥在设备上生成，服务端只保存密文。即使数据库泄漏或服务端被控制，也无法还原模板内容。服务端代码与客户端一起开源，你也可以部署自己的后端。
          </p>
        </div>
        <p>
          <Link className="btn" to="/security">
            阅读安全设计
          </Link>
        </p>
      </section>
    </>
  )
}
