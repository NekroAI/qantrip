import { env } from 'cloudflare:workers'
import { Fragment } from 'react'
import { data, Form, Link, useNavigation, useSearchParams } from 'react-router'
import { isAdmin, listUsers, PAGE_SIZE, setUserLimits, stats, userDevices } from '../../server/admin.js'
import { sessionUser } from '../../server/auth.js'
import { defaultLimits, type Limits } from '../../server/limits.js'
import { type Locale, useLocale } from '../lib/i18n'
import type { Route } from './+types/admin'

// 运营后台（仅管理员）：统计、用户列表、按账号调整同步限制。

export const meta: Route.MetaFunction = () => [{ title: 'Admin · Nekro Qantrip' }, { name: 'robots', content: 'noindex' }]

const requireAdmin = async (request: Request) => {
  const user = await sessionUser(env, request)
  // 非管理员一律当作页面不存在
  if (!user || !isAdmin(env, user)) throw data(null, { status: 404 })
  return user
}

export const loader = async ({ request }: Route.LoaderArgs) => {
  await requireAdmin(request)
  const url = new URL(request.url)
  const q = url.searchParams.get('q') ?? ''
  const page = Math.max(0, Number(url.searchParams.get('page') ?? 0) || 0)
  const open = url.searchParams.get('user')
  const [s, list, devices] = await Promise.all([stats(env), listUsers(env, { q, page }), open ? userDevices(env, open) : Promise.resolve(null)])
  return { stats: s, ...list, page, q, open, devices, defaults: defaultLimits(env) }
}

const FIELDS: { key: keyof Limits; unit: 'count' | 'kb' | 'min' }[] = [
  { key: 'max_devices', unit: 'count' },
  { key: 'quota_bytes', unit: 'kb' },
  { key: 'check_interval_secs', unit: 'min' },
  { key: 'panel_check_secs', unit: 'min' },
  { key: 'min_push_interval_secs', unit: 'min' },
]

/** 表单单位 → 存储单位 */
const toStored = (unit: 'count' | 'kb' | 'min', v: number) => (unit === 'kb' ? Math.round(v * 1024) : unit === 'min' ? Math.round(v * 60) : Math.round(v))
const toForm = (unit: 'count' | 'kb' | 'min', v: number) => (unit === 'kb' ? +(v / 1024).toFixed(1) : unit === 'min' ? +(v / 60).toFixed(1) : v)

export const action = async ({ request }: Route.ActionArgs) => {
  const admin = await requireAdmin(request)
  const form = await request.formData()
  if (form.get('intent') !== 'limits') return { error: null }
  const userId = String(form.get('user'))
  const values: Partial<Record<keyof Limits, number | null>> = {}
  for (const f of FIELDS) {
    const raw = String(form.get(f.key) ?? '').trim()
    if (raw === '') {
      values[f.key] = null
      continue
    }
    const n = Number(raw)
    if (!Number.isFinite(n)) return { error: f.key }
    values[f.key] = toStored(f.unit, n)
  }
  const bad = await setUserLimits(env, admin, userId, values)
  return { error: bad, saved: bad ? null : userId }
}

const T = {
  en: {
    h1: 'Admin',
    users: 'Users',
    newUsers: 'New users',
    active: 'Active users',
    devices: 'Signed-in devices',
    seen7: 'active in 7 days',
    limited: 'over the device limit',
    vaults: 'Vaults',
    storage: 'Cloud storage',
    records: 'records',
    signups: 'Sign-ups, last 30 days',
    search: 'Search by GitHub login',
    colUser: 'User',
    colJoined: 'Joined',
    colDevices: 'Devices',
    colStorage: 'Storage',
    colSeen: 'Last seen',
    never: '—',
    edit: 'Limits',
    labels: {
      max_devices: 'Devices',
      quota_bytes: 'Storage (KB)',
      check_interval_secs: 'Background check (min)',
      panel_check_secs: 'Check on panel open (min)',
      min_push_interval_secs: 'Min. interval between auto uploads (min)',
    } as Record<keyof Limits, string>,
    defaultHint: 'Empty = default',
    save: 'Save',
    saved: 'Saved',
    invalid: 'Value out of range',
    deviceList: 'Signed-in devices',
    noDevices: 'No signed-in devices',
    prev: 'Previous',
    next: 'Next',
    custom: 'custom',
    d1: '24 h',
    d7: '7 days',
    d30: '30 days',
    dateLocale: 'en-US',
  },
  zh: {
    h1: '后台管理',
    users: '用户',
    newUsers: '新注册',
    active: '活跃用户',
    devices: '已登录设备',
    seen7: '7 天内活跃',
    limited: '超出设备上限',
    vaults: '保险库',
    storage: '云端用量',
    records: '条记录',
    signups: '近 30 天注册',
    search: '按 GitHub 用户名搜索',
    colUser: '用户',
    colJoined: '注册',
    colDevices: '设备',
    colStorage: '用量',
    colSeen: '上次活跃',
    never: '—',
    edit: '限制',
    labels: {
      max_devices: '设备数',
      quota_bytes: '容量（KB）',
      check_interval_secs: '后台检查间隔（分钟）',
      panel_check_secs: '唤出面板时检查间隔（分钟）',
      min_push_interval_secs: '自动上传最小间隔（分钟）',
    } as Record<keyof Limits, string>,
    defaultHint: '留空为默认值',
    save: '保存',
    saved: '已保存',
    invalid: '数值超出允许范围',
    deviceList: '已登录的设备',
    noDevices: '没有已登录的设备',
    prev: '上一页',
    next: '下一页',
    custom: '已调整',
    d1: '24 小时',
    d7: '7 天',
    d30: '30 天',
    dateLocale: 'zh-CN',
  },
} satisfies Record<Locale, unknown>

const kb = (n: number | null) => (n == null ? '—' : n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(2)} MB`)

export default function Admin({ loaderData, actionData }: Route.ComponentProps) {
  const t = T[useLocale()]
  const { stats: s, users, total, page, q, open, devices, defaults } = loaderData
  const busy = useNavigation().state !== 'idle'
  const [params] = useSearchParams()
  const date = (ms: number | null) => (ms ? new Date(ms).toLocaleDateString(t.dateLocale, { month: 'short', day: 'numeric', year: 'numeric' }) : t.never)
  const max = Math.max(1, ...s.signups.map((d) => d.count))
  const link = (patch: Record<string, string | null>) => {
    const p = new URLSearchParams(params)
    for (const [k, v] of Object.entries(patch)) v === null ? p.delete(k) : p.set(k, v)
    return `?${p.toString()}`
  }

  return (
    <section className="wrap page admin">
      <header>
        <h1>{t.h1}</h1>
      </header>

      <div className="admin-stats">
        <div className="card">
          <span className="muted">{t.users}</span>
          <b>{s.users}</b>
          <span className="muted small">
            {t.newUsers} · {t.d1} {s.newUsers.d1} · {t.d7} {s.newUsers.d7} · {t.d30} {s.newUsers.d30}
          </span>
        </div>
        <div className="card">
          <span className="muted">{t.active}</span>
          <b>{s.activeUsers.d7}</b>
          <span className="muted small">
            {t.d1} {s.activeUsers.d1} · {t.d7} {s.activeUsers.d7} · {t.d30} {s.activeUsers.d30}
          </span>
        </div>
        <div className="card">
          <span className="muted">{t.devices}</span>
          <b>{s.devices.active}</b>
          <span className="muted small">
            {s.devices.seen7} {t.seen7} · {s.devices.limited} {t.limited}
          </span>
        </div>
        <div className="card">
          <span className="muted">{t.vaults}</span>
          <b>{s.vaults}</b>
          <span className="muted small">
            {kb(s.bytes)} · {s.records} {t.records}
          </span>
        </div>
      </div>

      <div className="card">
        <span className="label">{t.signups}</span>
        <div className="admin-bars" role="img" aria-label={t.signups}>
          {s.signups.map((d) => (
            <span key={d.day} title={`${d.day}: ${d.count}`} style={{ height: `${Math.max(2, (d.count / max) * 100)}%` }} />
          ))}
        </div>
      </div>

      <Form method="get" className="row" style={{ justifyContent: 'flex-start' }}>
        <input className="input" name="q" defaultValue={q} placeholder={t.search} style={{ minWidth: 260 }} />
      </Form>

      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>{t.colUser}</th>
              <th>{t.colJoined}</th>
              <th>{t.colDevices}</th>
              <th>{t.colStorage}</th>
              <th>{t.colSeen}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {users.map((u) => (
              <Fragment key={u.id}>
                <tr>
                  <td>
                    <span className="row" style={{ justifyContent: 'flex-start', gap: 8 }}>
                      {u.avatar_url && <img src={u.avatar_url} alt="" width={22} height={22} style={{ borderRadius: '50%' }} />}
                      <a href={`https://github.com/${u.login}`} rel="noopener">
                        {u.login}
                      </a>
                      {u.overridden.length > 0 && <span className="tag">{t.custom}</span>}
                    </span>
                  </td>
                  <td>{date(u.created_at)}</td>
                  <td>
                    {u.devices} / {u.limits.max_devices}
                  </td>
                  <td>
                    {kb(u.bytes)} / {kb(u.limits.quota_bytes)}
                  </td>
                  <td>{date(u.last_seen)}</td>
                  <td>
                    <Link to={link({ user: open === u.id ? null : u.id })}>{t.edit}</Link>
                  </td>
                </tr>
                {open === u.id && (
                  <tr>
                    <td colSpan={6}>
                      <div className="admin-edit">
                        <Form method="post" className="admin-limits">
                          <input type="hidden" name="intent" value="limits" />
                          <input type="hidden" name="user" value={u.id} />
                          {FIELDS.map((f) => (
                            <label key={f.key}>
                              <span>{t.labels[f.key]}</span>
                              <input
                                className="input"
                                name={f.key}
                                type="number"
                                step="any"
                                min={0}
                                defaultValue={u.overridden.includes(f.key) ? toForm(f.unit, u.limits[f.key]) : ''}
                                placeholder={`${toForm(f.unit, defaults[f.key])}`}
                              />
                            </label>
                          ))}
                          <span className="row" style={{ justifyContent: 'flex-start' }}>
                            <button className="btn small primary" type="submit" disabled={busy}>
                              {t.save}
                            </button>
                            <span className="muted small">{t.defaultHint}</span>
                            {actionData?.saved === u.id && <span className="small" style={{ color: 'var(--option)' }}>{t.saved}</span>}
                            {actionData?.error && <span className="small" style={{ color: 'var(--danger)' }}>{t.invalid}</span>}
                          </span>
                        </Form>
                        <div>
                          <span className="label">{t.deviceList}</span>
                          {devices && devices.length > 0 ? (
                            <ul className="admin-devices">
                              {devices.map((d, i) => (
                                <li key={i}>
                                  <b>{d.device_name ?? '—'}</b>
                                  <span className="muted small">
                                    {[d.platform, d.app_version && `Qantrip ${d.app_version}`, date(d.last_used_at)].filter(Boolean).join(' · ')}
                                    {d.limited ? ` · ${t.limited}` : ''}
                                  </span>
                                </li>
                              ))}
                            </ul>
                          ) : (
                            <p className="muted small">{t.noDevices}</p>
                          )}
                        </div>
                      </div>
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
          </tbody>
        </table>
      </div>

      <div className="row">
        <span className="muted small">
          {page * PAGE_SIZE + (users.length ? 1 : 0)}–{page * PAGE_SIZE + users.length} / {total}
        </span>
        <span className="row">
          {page > 0 && (
            <Link className="btn small" to={link({ page: String(page - 1), user: null })}>
              {t.prev}
            </Link>
          )}
          {(page + 1) * PAGE_SIZE < total && (
            <Link className="btn small" to={link({ page: String(page + 1), user: null })}>
              {t.next}
            </Link>
          )}
        </span>
      </div>
    </section>
  )
}
