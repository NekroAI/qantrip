import { Link } from 'react-router'
import { GitHubIcon } from '../components/GitHubIcon'
import { LiveDemo } from '../components/LiveDemo'
import { REPO_URL } from '../lib/site'
import { BUILTIN_TEMPLATES } from '../lib/templates'
import type { Route } from './+types/home'

export const meta: Route.MetaFunction = () => [
  { title: 'Nekro Qantrip — 开源的 Prompt 启动器' },
  { name: 'description', content: '按下 ⌃Q，输入模板名和参数，完整的 Prompt 就出现在 Claude Code、Codex、浏览器或任何输入框里。开源、本地优先。' },
  { property: 'og:title', content: 'Nekro Qantrip' },
  { property: 'og:description', content: '开源的 Prompt 启动器：几个按键调出带参数的 Prompt，插入任何应用。' },
  { property: 'og:image', content: '/media/icon-256.png' },
]

export default function Home() {
  return (
    <>
      <section className="wrap hero">
        <div className="hero-meta">
          <span>开源</span>
          <span>macOS 14+</span>
          <span>本地优先</span>
        </div>
        <h1>
          <span>在任何 AI 工具里，</span>
          <span>用几个按键调出你的 Prompt</span>
        </h1>
        <p>
          Qantrip 是一个开源的 Prompt 启动器。按下 <kbd>⌃Q</kbd> 唤起面板，输入模板名和参数，回车后完整的 Prompt 就出现在 Claude Code、Codex、浏览器或任何输入框里。
        </p>
        <div className="hero-actions">
          <a className="btn primary" href={REPO_URL}>
            <GitHubIcon /> 在 GitHub 上查看
          </a>
          <Link className="btn" to="/download">
            下载 macOS 版
          </Link>
        </div>
      </section>

      <section className="wrap" id="demo">
        <LiveDemo />
      </section>

      <section className="wrap section">
        <header>
          <h2>一条命令，一段完整的 Prompt</h2>
          <p>命令由模板名、参数和附加要求组成。不记得参数也没关系：输入 <code>--</code> 就会列出全部参数，灰字会提示下一步可以输入什么。</p>
        </header>
        <div className="anatomy">
          <div className="anatomy-line" aria-label="命令示例">
            <span className="t-command">r</span> <span className="t-positional">security</span> <span className="t-optionname">--scope=</span>
            <span className="t-optionvalue">staged</span> <span className="t-shortflag">-s</span> 重点看认证和权限
          </div>
          <ul className="anatomy-parts">
            <li>
              <code className="t-command">r</code>
              模板名或别名。拼错时会给出最接近的模板，确认后才插入。
            </li>
            <li>
              <code className="t-positional">security</code>
              位置参数：直接输入候选值，不必写参数名。
            </li>
            <li>
              <code className="t-optionname">--scope=staged</code>
              长参数。多选用逗号分隔，参数名和取值都可以只输前缀。
            </li>
            <li>
              <code className="t-shortflag">-s</code>
              开关参数的单字母简写。
            </li>
            <li>
              <code>重点看认证和权限</code>
              第一个不是参数的词起，后面的文字原样附在 Prompt 末尾。
            </li>
          </ul>
        </div>
      </section>

      <section className="wrap section">
        <header>
          <h2>模板就是一个 Markdown 文件</h2>
          <p>参数在文件头部声明，正文用 MiniJinja 编写。可以用任何编辑器修改，Qantrip 会自动重新加载；应用内的模板库也提供表单编辑、试用和预览。</p>
        </header>
        <div className="cols">
          <div>
            <span className="label">~/Library/Application Support/Qantrip/templates/review.md</span>
            <pre className="code">
              <span className="c">---</span>
              {'\n'}
              <span className="k">name</span>: review{'\n'}
              <span className="k">aliases</span>: [r]{'\n'}
              <span className="k">params</span>:{'\n'}
              {'  '}
              <span className="k">focus</span>:{'\n'}
              {'    '}type: multi{'\n'}
              {'    '}positional: true{'\n'}
              {'    '}choices: [<span className="s">security</span>, <span className="s">performance</span>]{'\n'}
              {'  '}
              <span className="k">strict</span>: {'{ '}type: bool, short: s {'}'}
              {'\n'}
              <span className="c">---</span>
              {'\n'}
              请审查当前改动。{'\n'}
              <span className="j">{'{% if strict %}'}</span>不确定的地方也列出来。<span className="j">{'{% endif %}'}</span>
              {'\n'}
              <span className="j">{'{% include "_extra" %}'}</span>
            </pre>
          </div>
          <div>
            <span className="label">输入 r security -s 重点看认证 得到</span>
            <pre className="code" style={{ whiteSpace: 'pre-wrap' }}>
              {'请审查当前改动。\n重点关注：安全（权限、输入校验、敏感信息、注入）。\n不确定的地方也列出来。\n\n补充要求：重点看认证'}
            </pre>
          </div>
        </div>
      </section>

      <section className="wrap section">
        <header>
          <h2>插入时不打扰你</h2>
        </header>
        <ul className="facts two">
          <li>
            <b>剪贴板会还原</b>
            <span>通过剪贴板粘贴，完成后恢复你原来复制的内容；Prompt 标记为临时内容，不进入剪贴板管理器的历史。</span>
          </li>
          <li>
            <b>多行不会被提交</b>
            <span>以粘贴而不是逐字键入的方式插入，终端和 AI 编程代理不会把换行当作回车。</span>
          </li>
          <li>
            <b>不授权也能用</b>
            <span>没有辅助功能权限时，Prompt 复制到剪贴板，由你手动粘贴。</span>
          </li>
          <li>
            <b>Claude Code 长粘贴</b>
            <span>超过 800 字或 3 行的粘贴会被折叠，<kbd>⌘↩</kbd> 连续粘贴两次让它展开。</span>
          </li>
        </ul>
      </section>

      <section className="wrap section">
        <header>
          <h2>{BUILTIN_TEMPLATES.length} 个内置模板</h2>
          <p>覆盖审查、调试、测试、重构、提交与翻译。每个都可以自定义，也可以照着写自己的。</p>
        </header>
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>命令</th>
                <th>用途</th>
                <th>参数</th>
              </tr>
            </thead>
            <tbody>
              {BUILTIN_TEMPLATES.map((t) => (
                <tr key={t.name}>
                  <td>
                    <code className="name">{t.name}</code>
                    {t.aliases.length > 0 && <span className="muted"> · {t.aliases.join(' · ')}</span>}
                  </td>
                  <td>{t.description}</td>
                  <td>
                    <code className="params">{t.params.map((p) => `--${p}`).join(' ')}</code>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section className="wrap section">
        <div className="oss">
          <header style={{ display: 'grid', gap: 8 }}>
            <h2>开源，也可以自己部署</h2>
            <p className="muted" style={{ maxWidth: '64ch' }}>
              客户端、核心与同步服务端都在同一个仓库里。同步采用端到端加密：密钥只在你的设备上，服务端只保存无法解密的密文；你也可以把服务端部署到自己的 Cloudflare 账号，在客户端里切换后端地址。
            </p>
          </header>
          <ul className="facts">
            <li>
              <b>客户端与核心</b>
              <span>Rust 核心 + 各平台原生界面，GPL-3.0-or-later。</span>
            </li>
            <li>
              <b>同步服务端</b>
              <span>Cloudflare Workers + D1，AGPL-3.0-or-later，不含任何解密逻辑。</span>
            </li>
            <li>
              <b>安全设计公开</b>
              <span>
                威胁模型、密钥层级与服务端可见的元数据都写在 <Link to="/security">安全设计</Link> 里。
              </span>
            </li>
          </ul>
          <div className="hero-actions">
            <a className="btn primary" href={REPO_URL}>
              <GitHubIcon /> 源代码
            </a>
            <a className="btn" href={`${REPO_URL}/issues`}>
              问题反馈
            </a>
          </div>
        </div>
      </section>
    </>
  )
}
