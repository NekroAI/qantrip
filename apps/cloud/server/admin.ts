// 运营后台：管理员由 SUPER_ADMIN_GITHUB_IDS（逗号分隔的 GitHub 用户 ID）指定。
// 提供统计、用户列表与按账号覆盖限制（users.limits）。服务端看不到任何模板内容，这里只有元数据。
import { all, audit, first, run } from './db.js'
import { now } from './http.js'
import { defaultLimits, effectiveLimits, type Limits } from './limits.js'

export interface AdminUser {
  id: string
  github_id: number
  login: string
}

export const isAdmin = (env: Env, user: { github_id: number } | null): boolean => {
  if (!user) return false
  const ids = String(env.SUPER_ADMIN_GITHUB_IDS ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
  return ids.includes(String(user.github_id))
}

const DAY = 86_400_000

export interface Stats {
  users: number
  newUsers: { d1: number; d7: number; d30: number }
  activeUsers: { d1: number; d7: number; d30: number }
  devices: { active: number; seen7: number; limited: number }
  vaults: number
  records: number
  bytes: number
  /** 最近 30 天每天新注册的用户数（从旧到新） */
  signups: { day: string; count: number }[]
}

const count = async (db: D1Database, sql: string, ...params: unknown[]): Promise<number> =>
  (await first<{ n: number }>(db, sql, ...params))?.n ?? 0

export const stats = async (env: Env): Promise<Stats> => {
  const db = env.DB
  const t = now()
  const [users, n1, n7, n30, a1, a7, a30, active, seen7, limited, vault, daily] = await Promise.all([
    count(db, 'SELECT COUNT(*) AS n FROM users'),
    count(db, 'SELECT COUNT(*) AS n FROM users WHERE created_at > ?', t - DAY),
    count(db, 'SELECT COUNT(*) AS n FROM users WHERE created_at > ?', t - 7 * DAY),
    count(db, 'SELECT COUNT(*) AS n FROM users WHERE created_at > ?', t - 30 * DAY),
    count(db, 'SELECT COUNT(DISTINCT user_id) AS n FROM device_grants WHERE revoked_at IS NULL AND last_used_at > ?', t - DAY),
    count(db, 'SELECT COUNT(DISTINCT user_id) AS n FROM device_grants WHERE revoked_at IS NULL AND last_used_at > ?', t - 7 * DAY),
    count(db, 'SELECT COUNT(DISTINCT user_id) AS n FROM device_grants WHERE revoked_at IS NULL AND last_used_at > ?', t - 30 * DAY),
    count(db, 'SELECT COUNT(*) AS n FROM device_grants WHERE revoked_at IS NULL AND refresh_expires_at > ?', t),
    count(db, 'SELECT COUNT(*) AS n FROM device_grants WHERE revoked_at IS NULL AND last_used_at > ?', t - 7 * DAY),
    count(db, 'SELECT COUNT(*) AS n FROM device_grants WHERE revoked_at IS NULL AND limited = 1 AND refresh_expires_at > ?', t),
    first<{ n: number; records: number | null; bytes: number | null }>(db, 'SELECT COUNT(*) AS n, SUM(record_count) AS records, SUM(byte_count) AS bytes FROM vaults'),
    all<{ day: string; n: number }>(
      db,
      "SELECT strftime('%Y-%m-%d', created_at / 1000, 'unixepoch') AS day, COUNT(*) AS n FROM users WHERE created_at > ? GROUP BY day",
      t - 30 * DAY,
    ),
  ])
  const byDay = new Map(daily.map((r) => [r.day, r.n]))
  const signups = Array.from({ length: 30 }, (_, i) => {
    const day = new Date(t - (29 - i) * DAY).toISOString().slice(0, 10)
    return { day, count: byDay.get(day) ?? 0 }
  })
  return {
    users,
    newUsers: { d1: n1, d7: n7, d30: n30 },
    activeUsers: { d1: a1, d7: a7, d30: a30 },
    devices: { active, seen7, limited },
    vaults: vault?.n ?? 0,
    records: vault?.records ?? 0,
    bytes: vault?.bytes ?? 0,
    signups,
  }
}

export interface UserRowAdmin {
  id: string
  github_id: number
  login: string
  avatar_url: string | null
  created_at: number
  last_login_at: number
  devices: number
  last_seen: number | null
  bytes: number | null
  records: number | null
  limits: Limits
  overridden: (keyof Limits)[]
}

export const PAGE_SIZE = 50

export const listUsers = async (env: Env, opts: { q?: string; page?: number }): Promise<{ users: UserRowAdmin[]; total: number }> => {
  const page = Math.max(0, opts.page ?? 0)
  const q = (opts.q ?? '').trim()
  const where = q ? 'WHERE u.login LIKE ? ESCAPE \'\\\'' : ''
  const params = q ? [`%${q.replace(/[\\%_]/g, (c) => `\\${c}`)}%`] : []
  const t = now()
  const rows = await all<{
    id: string
    github_id: number
    login: string
    avatar_url: string | null
    created_at: number
    last_login_at: number
    limits: string | null
    devices: number
    last_seen: number | null
    bytes: number | null
    records: number | null
  }>(
    env.DB,
    `SELECT u.id, u.github_id, u.login, u.avatar_url, u.created_at, u.last_login_at, u.limits,
            (SELECT COUNT(*) FROM device_grants g WHERE g.user_id = u.id AND g.revoked_at IS NULL AND g.refresh_expires_at > ?) AS devices,
            (SELECT MAX(last_used_at) FROM device_grants g WHERE g.user_id = u.id AND g.revoked_at IS NULL) AS last_seen,
            v.byte_count AS bytes, v.record_count AS records
     FROM users u LEFT JOIN vaults v ON v.user_id = u.id
     ${where}
     ORDER BY u.created_at DESC LIMIT ? OFFSET ?`,
    t,
    ...params,
    PAGE_SIZE,
    page * PAGE_SIZE,
  )
  const total = await count(env.DB, `SELECT COUNT(*) AS n FROM users u ${where}`, ...params)
  return {
    total,
    users: rows.map((r) => ({
      ...r,
      limits: effectiveLimits(env, r.limits),
      overridden: overriddenKeys(env, r.limits),
    })),
  }
}

const overriddenKeys = (env: Env, raw: string | null): (keyof Limits)[] => {
  if (!raw) return []
  const eff = effectiveLimits(env, raw)
  const def = defaultLimits(env)
  return (Object.keys(def) as (keyof Limits)[]).filter((k) => eff[k] !== def[k])
}

export interface UserDevice {
  device_name: string | null
  platform: string | null
  app_version: string | null
  created_at: number
  last_used_at: number
  limited: number
}

export const userDevices = (env: Env, userId: string): Promise<UserDevice[]> =>
  all<UserDevice & Record<string, unknown>>(
    env.DB,
    'SELECT device_name, platform, app_version, created_at, last_used_at, limited FROM device_grants WHERE user_id = ? AND revoked_at IS NULL AND refresh_expires_at > ? ORDER BY last_used_at DESC',
    userId,
    now(),
  )

/**
 * 设置某个账号的限制覆盖。`values` 中为 null 或缺失的项恢复默认；不合法的值返回错误信息。
 * 只保存与默认值不同的项，便于以后调整默认值时自动生效。
 */
export const setUserLimits = async (env: Env, admin: AdminUser, userId: string, values: Partial<Record<keyof Limits, number | null>>): Promise<string | null> => {
  const def = defaultLimits(env)
  const next: Partial<Limits> = {}
  for (const key of Object.keys(def) as (keyof Limits)[]) {
    const v = values[key]
    if (v === null || v === undefined || v === def[key]) continue
    next[key] = v
  }
  const json = Object.keys(next).length ? JSON.stringify(next) : null
  // 校验：逐项检查是否被接受（不合法的项会被 effectiveLimits 忽略）
  const eff = effectiveLimits(env, json)
  for (const [k, v] of Object.entries(next) as [keyof Limits, number][]) {
    if (eff[k] !== v) return k
  }
  const res = await run(env.DB, 'UPDATE users SET limits = ? WHERE id = ?', json, userId)
  if (!res.meta.changes) return 'user'
  await audit(env.DB, userId, 'admin_set_limits', JSON.stringify({ by: admin.login, limits: next }))
  return null
}
