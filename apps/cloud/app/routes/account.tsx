import { env } from 'cloudflare:workers'
import { Form, Link, redirect, useNavigation } from 'react-router'
import { deleteAccount, listGrants, loginAvailable, revokeGrant, sessionCookie, sessionUser, userLimits } from '../../server/auth.js'
import { HARD } from '../../server/limits.js'
import { isAdmin } from '../../server/admin.js'
import { deleteVault, vaultOf } from '../../server/sync.js'
import { type Locale, useHref, useLocale } from '../lib/i18n'
import type { Route } from './+types/account'

const T = {
  en: {
    signInH: 'Sign in to Qantrip',
    signInP: 'An account lets you sync templates between your devices. Sync is end-to-end encrypted; signing in does not let anyone decrypt anything.',
    failed: "Sign-in didn't complete. Please try again.",
    github: 'Sign in with GitHub',
    soonH: 'Sign-in is coming soon',
    soonP: 'The sync service is being prepared.',
    ghAccount: 'GitHub account',
    signOut: 'Sign out',
    admin: 'Admin',
    vault: 'Sync vault',
    records: 'encrypted records',
    total: (quota: string) => `ciphertext / ${quota}`,
    created: 'created',
    noVault: 'No vault yet. It is created when you turn on sync in the Qantrip app.',
    onlyCounts: 'Only counts and sizes are visible here: content is encrypted on your devices and the server cannot read it.',
    devices: 'Signed-in devices',
    noDevices: 'No devices signed in yet.',
    lastActive: 'last active',
    signedIn: 'signed in',
    revoke: 'Sign out this device',
    devicesNote: 'Device names, platforms and app versions are reported by the app when it signs in. Signing a device out only revokes its access; to remove it from the vault and rotate keys, use another signed-in device.',
    unnamed: 'Unnamed device',
    limited: 'over the device limit, cannot sync',
    deviceLimit: (n: number) => `Up to ${n} devices can sync with this account at the same time.`,
    danger: 'Danger zone',
    delVault: 'Delete vault',
    delVaultP: 'Deletes all ciphertext on the server. Local templates on your devices are not affected; you can turn sync on again later.',
    delAccount: 'Delete account',
    delAccountP: 'Deletes your account, device authorizations and vault. This cannot be undone.',
    confirm: (login: string) => `Type ${login} to confirm`,
    whatServer: 'What can the server see?',
    dateLocale: 'en-US',
  },
  zh: {
    signInH: '登录 Qantrip',
    signInP: '账号用于在多台设备之间同步模板。同步内容端到端加密，登录本身不能解密任何数据。',
    failed: '登录没有完成，请重试。',
    github: '使用 GitHub 登录',
    soonH: '登录即将开放',
    soonP: '同步服务正在准备中。',
    ghAccount: 'GitHub 账号',
    signOut: '退出登录',
    admin: '后台管理',
    vault: '同步保险库',
    records: '条加密记录',
    total: (quota: string) => `密文总量 / ${quota}`,
    created: '创建时间',
    noVault: '还没有保险库。在 Qantrip 应用的设置中开启同步后会自动创建。',
    onlyCounts: '这里只能看到数量与大小：内容由你的设备加密，服务端无法读取。',
    devices: '已登录的设备',
    noDevices: '还没有设备登录。',
    lastActive: '最后活跃',
    signedIn: '登录于',
    revoke: '退出此设备',
    devicesNote: '设备名称、平台与应用版本由应用登录时上报。退出设备只收回它的访问权；要把它移出保险库并轮换密钥，请在另一台已登录的设备上操作。',
    unnamed: '未命名设备',
    limited: '超出设备数量限制，无法同步',
    deviceLimit: (n: number) => `此账号最多同时在 ${n} 台设备上同步。`,
    danger: '危险操作',
    delVault: '删除保险库',
    delVaultP: '删除服务端保存的全部密文。所有设备上的本地模板不受影响，之后可以在设备上重新开启同步。',
    delAccount: '删除账号',
    delAccountP: '删除账号、所有设备授权与保险库。此操作无法撤销。',
    confirm: (login: string) => `输入 ${login} 确认`,
    whatServer: '服务端能看到哪些信息？',
    dateLocale: 'zh-CN',
  },
} satisfies Record<Locale, unknown>

export const meta: Route.MetaFunction = () => [{ title: 'Account · Nekro Qantrip' }, { name: 'robots', content: 'noindex' }]

export const loader = async ({ request }: Route.LoaderArgs) => {
  const url = new URL(request.url)
  const user = await sessionUser(env, request)
  const status = url.searchParams.get('login')
  if (!user) return { user: null, loginOpen: loginAvailable(env), status, grants: [], vault: null, maxDevices: 0 }
  const [grants, vault, limits] = await Promise.all([listGrants(env, user.id), vaultOf(env.DB, user.id), userLimits(env, user.id)])
  return {
    user: { login: user.login, avatar: user.avatar_url },
    admin: isAdmin(env, user),
    loginOpen: true,
    status,
    grants: grants.map((g) => ({
      id: g.id,
      device: g.device_id,
      name: g.device_name,
      platform: g.platform,
      version: g.app_version,
      limited: g.limited === 1,
      created: g.created_at,
      lastUsed: g.last_used_at,
    })),
    vault: vault
      ? { records: vault.record_count, bytes: vault.byte_count, quota: Math.min(limits.quota_bytes, HARD.vaultBytes), created: vault.created_at as number }
      : null,
    maxDevices: limits.max_devices,
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

const date = (ms: number, locale: string) => new Date(ms).toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' })
const size = (n: number) => (n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(2)} MB`)

export default function Account({ loaderData }: Route.ComponentProps) {
  const { user, loginOpen, status, grants, vault, maxDevices } = loaderData
  const busy = useNavigation().state !== 'idle'
  const t = T[useLocale()]
  const href = useHref()
  const fmt = (ms: number) => date(ms, t.dateLocale)

  if (!user) {
    return (
      <section className="wrap page">
        <header>
          <h1>{t.signInH}</h1>
          <p>{t.signInP}</p>
        </header>
        {status === 'failed' && <div className="callout">{t.failed}</div>}
        {loginOpen ? (
          <p>
            <a className="btn primary" href="/auth/github?return=/account">
              {t.github}
            </a>
          </p>
        ) : (
          <div className="callout">
            <strong>{t.soonH}</strong>
            <span className="muted">{t.soonP}</span>
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
            <span className="muted">{t.ghAccount}</span>
          </div>
        </div>
        {loaderData.admin && (
          <Link className="btn small" to="/admin">
            {t.admin}
          </Link>
        )}
        <form method="post" action="/auth/logout">
          <button className="btn small" type="submit">
            {t.signOut}
          </button>
        </form>
      </div>

      <div className="card">
        <h2>
          {t.vault}
        </h2>
        {vault ? (
          <div className="stats">
            <span>
              <b>{vault.records}</b>
              <span className="muted">{t.records}</span>
            </span>
            <span>
              <b>{size(vault.bytes)}</b>
              <span className="muted">{t.total(size(vault.quota))}</span>
            </span>
            <span>
              <b>{fmt(vault.created)}</b>
              <span className="muted">{t.created}</span>
            </span>
          </div>
        ) : (
          <p className="muted">{t.noVault}</p>
        )}
        <p className="muted">{t.onlyCounts}</p>
      </div>

      <div className="card">
        <h2>
          {t.devices}
        </h2>
        {grants.length === 0 ? (
          <p className="muted">{t.noDevices}</p>
        ) : (
          <div className="list">
            {grants.map((g) => (
              <div className="list-item" key={g.id}>
                <span>
                  <strong>{g.name ?? t.unnamed}</strong>
                  {g.platform && <span className="muted"> · {g.platform}</span>}
                  {g.version && <span className="muted"> · {g.version}</span>}
                  <br />
                  <span className="mono muted">{g.device.slice(0, 4)}…{g.device.slice(-4)}</span>
                  <span className="muted"> · {t.lastActive} {fmt(g.lastUsed)} · {t.signedIn} {fmt(g.created)}</span>
                  {g.limited && <span className="muted"> · {t.limited}</span>}
                </span>
                <Form method="post">
                  <input type="hidden" name="intent" value="revoke" />
                  <input type="hidden" name="grant" value={g.id} />
                  <button className="btn small danger" type="submit" disabled={busy}>
                    {t.revoke}
                  </button>
                </Form>
              </div>
            ))}
          </div>
        )}
        <p className="muted">
          {t.deviceLimit(maxDevices)} {t.devicesNote}
        </p>
      </div>

      <div className="card">
        <h2>
          {t.danger}
        </h2>
        <Danger
          intent="delete-vault"
          login={user.login}
          title={t.delVault}
          body={t.delVaultP}
          confirmLabel={t.confirm(user.login)}
          busy={busy}
        />
        <Danger intent="delete-account" login={user.login} title={t.delAccount} body={t.delAccountP} confirmLabel={t.confirm(user.login)} busy={busy} />
      </div>
      <p>
        <Link to={href('/security')}>{t.whatServer}</Link>
      </p>
    </section>
  )
}

function Danger({ intent, login, title, body, confirmLabel, busy }: { intent: string; login: string; title: string; body: string; confirmLabel: string; busy: boolean }) {
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
          {confirmLabel}
        </label>
        <input id={`${intent}-confirm`} name="confirm" required pattern={login.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} autoComplete="off" className="input" />
        <button className="btn small danger" type="submit" disabled={busy}>
          {title}
        </button>
      </span>
    </Form>
  )
}
