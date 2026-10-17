import { REPO_URL } from '../lib/site'
import type { Route } from './+types/security'

export const meta: Route.MetaFunction = () => [
  { title: '安全设计 · Nekro Qantrip' },
  { name: 'description', content: 'Qantrip 同步的端到端加密设计：密钥层级、设备加入、服务端能看到与看不到什么。' },
]

const VISIBLE = [
  ['账号', 'GitHub 用户 ID、用户名、头像、登录时间与 IP'],
  ['设备', '数量、公钥、加入与最后活跃时间（设备名称加密存储）'],
  ['花名册', '设备加入、撤销、密钥轮换等事件的类型与时间'],
  ['记录', '数量、密文长度（已填充到固定档位）、写入时间、写入设备、删除标记'],
  ['同步', '版本检查与拉取的时间和频率（与唤起面板解耦）'],
]

export default function Security() {
  return (
    <section className="wrap page">
      <header>
        <h1>同步的安全设计</h1>
        <p>同步功能正在开发。这里先公开它的设计：加密只在你的设备上完成，服务端不持有任何能解密的密钥。服务端代码与客户端在同一个仓库中开源，你也可以部署自己的后端。</p>
      </header>
      <div className="prose">
        <h2>密钥在哪里</h2>
        <ul>
          <li>
            <strong>Vault Key</strong>：加密所有同步记录的密钥，在你的第一台设备上随机生成，以 XChaCha20-Poly1305 加密每条记录。
          </li>
          <li>
            <strong>设备密钥</strong>：每台设备生成自己的密钥对，私钥存放在系统钥匙串里，从不上传。Vault Key 用设备公钥包装后才发给它（HPKE，RFC 9180）。
          </li>
          <li>
            <strong>恢复短语</strong>：24 个词，只在你的设备上生成和显示。所有设备都丢失时，用它找回数据。
          </li>
        </ul>
        <div className="callout">
          <strong>没有后门</strong>
          <span>如果所有设备和恢复短语都丢失，任何人（包括我们）都无法恢复你的同步数据。本地模板文件不受影响。</span>
        </div>

        <h2>新设备怎么加入</h2>
        <p>
          两台电脑之间无法扫码。新设备会显示一串 8 位校验码，你需要在已登录的设备上<strong>手动输入</strong>这串校验码，它才会把密钥发给新设备。校验码由双方的公钥共同算出：即使服务端试图冒充其中一方，校验码也对不上。
        </p>

        <h2>服务端能看到什么</h2>
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>类别</th>
                <th>服务端可见</th>
              </tr>
            </thead>
            <tbody>
              {VISIBLE.map(([k, v]) => (
                <tr key={k}>
                  <td>{k}</td>
                  <td>{v}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p>
          服务端<strong>看不到</strong>：模板名、别名、标签、正文、参数、设置内容、AI 配置与 API Key、设备名称、恢复短语。
        </p>

        <h2>能防住什么</h2>
        <ul>
          <li>数据库泄漏：只有密文。</li>
          <li>服务端篡改、挪用或回滚记录：认证加密与每条记录的版本计数会察觉并拒绝。</li>
          <li>服务端伪造设备或塞入自己的密钥：设备花名册由你的设备签名，密钥带有承诺，伪造会被发现。</li>
          <li>被撤销的设备：撤销后立即轮换密钥并重新加密，它拿不到之后的数据。</li>
        </ul>
        <p className="muted">
          服务端仍然可以拒绝服务或删除数据；本地数据始终完整，随时可以重新上传。完整设计见仓库中的{' '}
          <a href={`${REPO_URL}/blob/main/docs/sync-design.md`}>docs/sync-design.md</a>，包括威胁模型、协议细节与对抗性审查记录。
        </p>
      </div>
    </section>
  )
}
