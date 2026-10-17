import { Link } from 'react-router'
import type { Route } from './+types/docs'

export const meta: Route.MetaFunction = () => [{ title: '文档 · Nekro Qantrip' }]

export default function Docs() {
  return (
    <section className="wrap page">
      <header>
        <h1>使用 Qantrip</h1>
        <p>安装、输入语法与模板格式。更多设计文档见仓库中的 docs 目录。</p>
      </header>
      <div className="prose">
        <h2 id="install">安装（macOS）</h2>
        <p>
          从 <Link to="/download">下载页</Link> 获取 DMG（Apple 芯片选 <code>arm64</code>，Intel 芯片选 <code>x86_64</code>），打开后把 Qantrip 拖进「应用程序」。Qantrip 尚未经过 Apple 公证，首次打开时到 <strong>系统设置 → 隐私与安全性</strong> 点 <strong>仍要打开</strong>。
        </p>
        <p>首次启动会引导你确认唤起快捷键（默认 <code>⌃Q</code>），授予「辅助功能」权限后即可自动粘贴；不授权也能用，Prompt 会复制到剪贴板。</p>

        <h2 id="grammar">输入语法</h2>
        <pre className="code">{`r security --scope=staged -s 重点看认证和权限
│ │        │              │  └ 附加要求：第一个不是参数的词起，原文保留
│ │        │              └ 短参数：开关的单字母简写
│ │        └ 长参数：--名称=值，多选用逗号分隔
│ └ 位置参数：直接输入候选值
└ 命令：模板名或别名`}</pre>
        <ul>
          <li>
            匹配分三层：精确、前缀、模糊。模糊匹配时第一次回车只补全名称，第二次才插入，拼错不会误插入。
          </li>
          <li>
            输入 <code>r --</code> 查看全部参数；<code>⇥</code> 接受补全，<code>→</code> 接受灰字提示。
          </li>
          <li>
            中文输入法打出的 <code>——</code>、<code>＝</code>、<code>，</code> 会被正确识别；面板唤起时默认切到英文输入法。
          </li>
          <li>
            <code>-- </code> 之后的内容一律作为附加要求，例如 <code>r -- security 是重点</code>。
          </li>
          <li>
            <code>⌘↩</code> 展开粘贴：Claude Code 会把超过 800 字符或多于 3 行的粘贴折叠起来，⌘↩ 连续粘贴两次让它展开。
          </li>
        </ul>

        <h2 id="templates">模板格式</h2>
        <p>
          每个模板是一个 Markdown 文件，放在 <code>~/Library/Application Support/Qantrip/templates/</code>。头部用 YAML 声明名称、别名与参数，正文是 MiniJinja 模板：
        </p>
        <pre className="code">{`---
name: review
aliases: [r]
description: 代码审查
params:
  focus:  { type: multi, positional: true, choices: [security, performance] }
  scope:  { type: enum, default: changes, choices: [changes, staged, branch] }
  strict: { type: bool, short: s, description: 严格模式 }
---
请审查{% if scope == "staged" %}暂存区的{% endif %}改动。
{% if strict %}严格模式：不确定的地方也列出来。{% endif %}
{% include "_extra" %}`}</pre>
        <ul>
          <li>
            参数类型：<code>bool</code>、<code>enum</code>、<code>multi</code>、<code>string</code>、<code>number</code>、<code>text</code>；可设默认值、必填、短名、取值范围与位置参数。
          </li>
          <li>
            参数名中的 <code>-</code> 在正文里写作 <code>_</code>；附加要求为变量 <code>extra</code>。
          </li>
          <li>与内置模板同名的文件会覆盖内置版本；在应用的模板库里可以直接编辑、试用与预览。</li>
        </ul>
      </div>
    </section>
  )
}
