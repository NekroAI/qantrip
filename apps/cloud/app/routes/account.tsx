import { env } from 'cloudflare:workers'
import { Form, Link, redirect, useNavigation } from 'react-router'
import { deleteAccount, listGrants, loginAvailable, revokeGrant, sessionCookie, sessionUser } from '../../server/auth.js'
import { deleteVault, vaultOf } from '../../server/sync.js'
import type { Route } from './+types/account'

export const meta: Route.MetaFunction = () => [{ title: '账号 · Nekro Qantrip' }, { name: 'robots', content: 'noindex' }]

export const loader = async ({ request }: Route.LoaderArgs) => {
  const url = new URL(request.url)
  const user = await sessionUser(env, request)
  const status = url.searchParams.get('login')
  if (!user) return { user: null, loginOpen: loginAvailable(env), status, grants: [], vault: null }
  const [grants, vault] = await Promise.all([listGrants(env, user.id), vaultOf(env.DB, user.id)])
  return {
    user: { login: user.login, avatar: user.avatar_url },
    loginOpen: true,
    status,
    grants: grants.map((g) => ({ id: g.id, device: g.device_id, created: g.created_at, lastUsed: g.last_used_at })),
    vault: vault ? { records: vault.record_count, bytes: vault.byte_count, created: vault.created_at as number } : null,
  }
}

export const action = async ({ request }: Route.ActionArgs) => {
  const user = await sessionUser(env, request)
  if (!user) throw redirect('/account')
  const form = await request.formData()
  const intent = String(form.get('intent'))
  if (intent === 'revoke') await revokeGrant(env, user.id, String(form.get('grant')))
  if (intent === 'delete-vault' && form.get('confirm') === user.login) await deleteVault(env, user.id)
  if (intent === 'delete-account' && form.get('confirm') === user.login) {
    await deleteAccount(env, user.id)
    throw redirect('/', { headers: { 'set-cookie': sessionCookie('', 0) } })
  }
  return null
}

const date = (ms: number) => new Date(ms).toLocaleString('zh-CN', { timeZone: 'Asia/Shanghai', dateStyle: 'medium', timeStyle: 'short' })
const size = (n: number) => (n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(2)} MB`)

export default function Account({ loaderData }: Route.ComponentProps) {
  const { user, loginOpen, status, grants, vault } = loaderData
  const busy = useNavigation().state !== 'idle'

  if (!user) {
    return (
      <section className="wrap page">
        <header>
          <h1>登录 Qantrip</h1>
          <p>账号用于在多台设备之间同步模板。同步内容端到端加密，登录本身不能解密任何数据。</p>
        </header>
        {status === 'failed' && <div className="callout">登录没有完成，请重试。</div>}
        {loginOpen ? (
          <p>
            <a className="btn primary" href="/auth/github?return=/account">
              使用 GitHub 登录
            </a>
          </p>
        ) : (
          <div className="callout">
            <strong>登录即将开放</strong>
            <span className="muted">同步服务正在准备中。</span>
          </div>
        )}
      </section>
    )
  }

  return (
    <section className="wrap page">
      <div className="row">
        <div className="row" style={{ justifyContent: 'flex-start' }}>
          {user.avatar && <img className="avatar" src={user.avatar} alt="" width={44} height={44} />}
          <div>
            <h1 style={{ fontSize: '1.6rem' }}>
              {user.login}
            </h1>
            <span className="muted">GitHub 账号</span>
          </div>
        </div>
        <form method="post" action="/auth/logout">
          <button className="btn small" type="submit">
            退出登录
          </button>
        </form>
      </div>

      <div className="card">
        <h2>
          同步保险库
        </h2>
        {vault ? (
          <div className="stats">
            <span>
              <b>{vault.records}</b>
              <span className="muted">条加密记录</span>
            </span>
            <span>
              <b>{size(vault.bytes)}</b>
              <span className="muted">密文总量 / 10 MB</span>
            </span>
            <span>
              <b>{date(vault.created)}</b>
              <span className="muted">创建时间</span>
            </span>
          </div>
        ) : (
          <p className="muted">还没有保险库。在 Qantrip 应用的设置中开启同步后会自动创建。</p>
        )}
        <p className="muted">这里只能看到数量与大小：内容由你的设备加密，服务端无法读取。</p>
      </div>

      <div className="card">
        <h2>
          已登录的设备
        </h2>
        {grants.length === 0 ? (
          <p className="muted">还没有设备登录。</p>
        ) : (
          <div className="list">
            {grants.map((g) => (
              <div className="list-item" key={g.id}>
                <span>
                  <span className="mono">{g.device.slice(0, 4)}…{g.device.slice(-4)}</span>
                  <span className="muted"> · 最后活跃 {date(g.lastUsed)} · 登录于 {date(g.created)}</span>
                </span>
                <Form method="post">
                  <input type="hidden" name="intent" value="revoke" />
                  <input type="hidden" name="grant" value={g.id} />
                  <button className="btn small danger" type="submit" disabled={busy}>
                    退出此设备
                  </button>
                </Form>
              </div>
            ))}
          </div>
        )}
        <p className="muted">设备名称加密存储，这里只显示设备标识。退出设备只收回它的访问权；要把它移出保险库并轮换密钥，请在另一台已登录的设备上操作。</p>
      </div>

      <div className="card">
        <h2>
          危险操作
        </h2>
        <Danger
          intent="delete-vault"
          login={user.login}
          title="删除保险库"
          body="删除服务端保存的全部密文。所有设备上的本地模板不受影响，之后可以在设备上重新开启同步。"
          busy={busy}
        />
        <Danger intent="delete-account" login={user.login} title="删除账号" body="删除账号、所有设备授权与保险库。此操作无法撤销。" busy={busy} />
      </div>
      <p>
        <Link to="/security">服务端能看到哪些信息？</Link>
      </p>
    </section>
  )
}

function Danger({ intent, login, title, body, busy }: { intent: string; login: string; title: string; body: string; busy: boolean }) {
  return (
    <Form method="post" className="list-item">
      <span style={{ flex: '1 1 320px' }}>
        <strong>{title}</strong>
        <br />
        <span className="muted">{body}</span>
      </span>
      <input type="hidden" name="intent" value={intent} />
      <span className="row">
        <label className="muted" htmlFor={`${intent}-confirm`}>
          输入 {login} 确认
        </label>
        <input id={`${intent}-confirm`} name="confirm" required pattern={login.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} autoComplete="off" className="input" />
        <button className="btn small danger" type="submit" disabled={busy}>
          {title}
        </button>
      </span>
    </Form>
  )
}
