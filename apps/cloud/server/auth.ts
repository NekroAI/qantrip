// 访问控制（docs/sync-design.md 第 9 节）：登录只决定能否调用 API，与能否解密无关。
// 官网：GitHub OAuth + 会话 Cookie；桌面端：浏览器授权码 + PKCE（S256），每台设备一条授权。
import { all, audit, first, run } from './db.js'
import { HttpError, b64u, now, randomToken, safeReturnPath, sha256, tokenHash } from './http.js'
import { HARD, effectiveLimits, type Limits } from './limits.js'

const DAY = 24 * 60 * 60 * 1000
const SESSION_TTL = 30 * DAY
const LOGIN_STATE_TTL = 10 * 60 * 1000
const CODE_TTL = 5 * 60 * 1000
const ACCESS_TTL = 72 * 60 * 60 * 1000
const REFRESH_TTL = 90 * DAY
/** 最后活跃时间最多每 10 分钟写一次，节省 D1 写入 */
const TOUCH_INTERVAL = 10 * 60 * 1000

export const SESSION_COOKIE = 'qt_session'
export const DESKTOP_CLIENT_ID = 'qantrip-desktop'
const ACCESS_PREFIX = 'qta_'
const REFRESH_PREFIX = 'qtr_'

export interface UserRow extends Record<string, unknown> {
  id: string
  github_id: number
  login: string
  avatar_url: string | null
}

export interface GrantRow extends Record<string, unknown> {
  id: string
  user_id: string
  device_id: string
  device_name: string | null
  platform: string | null
  app_version: string | null
  limited: number
  created_at: number
  last_used_at: number
  revoked_at: number | null
}

/** 有效授权：未吊销且刷新令牌未过期（SQL 条件，参数为当前时间） */
const ACTIVE = 'revoked_at IS NULL AND refresh_expires_at > ?'

/** 桌面端上报的设备信息（明文元数据，docs/sync-design.md 第 10 节） */
export interface DeviceMeta {
  device_name: string | null
  platform: string | null
  app_version: string | null
}

/** 去掉控制字符与格式字符（如双向覆盖、零宽字符），首尾去空白，按字符数截断；为空时返回 null。 */
export const cleanLabel = (value: unknown, max: number): string | null => {
  if (typeof value !== 'string') return null
  const text = [...value.replace(/[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]/gu, '').trim()].slice(0, max).join('').trim()
  return text === '' ? null : text
}

export const deviceMeta = (body: Record<string, unknown>): DeviceMeta => ({
  device_name: cleanLabel(body.device_name, 64),
  platform: cleanLabel(body.platform, 32),
  app_version: cleanLabel(body.app_version, 32),
})

export const loginAvailable = (env: Env): boolean => Boolean(env.GITHUB_CLIENT_ID && env.GITHUB_CLIENT_SECRET)

// MARK: 官网登录

export const startGitHubLogin = async (env: Env, returnTo: string | null): Promise<string> => {
  if (!loginAvailable(env)) throw new HttpError(503, 'login_unavailable', '登录即将开放')
  const state = randomToken(24)
  await run(env.DB, 'INSERT INTO login_states (state, return_to, expires_at) VALUES (?, ?, ?)', state, safeReturnPath(returnTo), now() + LOGIN_STATE_TTL)
  const url = new URL('https://github.com/login/oauth/authorize')
  url.searchParams.set('client_id', env.GITHUB_CLIENT_ID)
  url.searchParams.set('redirect_uri', `${env.SITE_ORIGIN}/auth/github/callback`)
  url.searchParams.set('state', state)
  url.searchParams.set('allow_signup', 'true')
  return url.toString()
}

interface GitHubUser {
  id: number
  login: string
  avatar_url?: string
}

/** 完成 GitHub 登录：返回会话令牌（写入 Cookie）与登录后的去向。 */
export const finishGitHubLogin = async (env: Env, code: string, state: string): Promise<{ session: string; returnTo: string }> => {
  if (!loginAvailable(env)) throw new HttpError(503, 'login_unavailable', '登录即将开放')
  const row = await first<{ return_to: string; expires_at: number }>(env.DB, 'SELECT return_to, expires_at FROM login_states WHERE state = ?', state)
  await run(env.DB, 'DELETE FROM login_states WHERE state = ?', state)
  if (!row || row.expires_at < now()) throw new HttpError(400, 'bad_state', '登录已过期，请重新登录')

  const tokenRes = await fetch('https://github.com/login/oauth/access_token', {
    method: 'POST',
    headers: { accept: 'application/json', 'content-type': 'application/json' },
    body: JSON.stringify({ client_id: env.GITHUB_CLIENT_ID, client_secret: env.GITHUB_CLIENT_SECRET, code }),
  })
  const token = (await tokenRes.json()) as { access_token?: string }
  if (!token.access_token) throw new HttpError(400, 'github_denied', 'GitHub 登录失败，请重试')
  const userRes = await fetch('https://api.github.com/user', {
    headers: { authorization: `Bearer ${token.access_token}`, accept: 'application/vnd.github+json', 'user-agent': 'nekro-qantrip' },
  })
  if (!userRes.ok) throw new HttpError(502, 'github_error', '无法读取 GitHub 账号信息，请重试')
  const gh = (await userRes.json()) as GitHubUser

  const t = now()
  const existing = await first<{ id: string }>(env.DB, 'SELECT id FROM users WHERE github_id = ?', gh.id)
  const userId = existing?.id ?? `u_${randomToken(12)}`
  if (existing) {
    await run(env.DB, 'UPDATE users SET login = ?, avatar_url = ?, last_login_at = ? WHERE id = ?', gh.login, gh.avatar_url ?? null, t, userId)
  } else {
    await run(env.DB, 'INSERT INTO users (id, github_id, login, avatar_url, created_at, last_login_at) VALUES (?, ?, ?, ?, ?, ?)', userId, gh.id, gh.login, gh.avatar_url ?? null, t, t)
    await audit(env.DB, userId, 'account_created')
  }
  const session = randomToken(32)
  await run(env.DB, 'INSERT INTO web_sessions (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)', await tokenHash(session), userId, t, t + SESSION_TTL)
  return { session, returnTo: row.return_to }
}

export const sessionCookie = (token: string, maxAgeMs = SESSION_TTL): string =>
  `${SESSION_COOKIE}=${token}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=${Math.floor(maxAgeMs / 1000)}`

export const readCookie = (request: Request, name: string): string | null => {
  const header = request.headers.get('cookie') ?? ''
  for (const part of header.split(';')) {
    const [k, ...v] = part.trim().split('=')
    if (k === name) return v.join('=')
  }
  return null
}

export const sessionUser = async (env: Env, request: Request): Promise<UserRow | null> => {
  const token = readCookie(request, SESSION_COOKIE)
  if (!token) return null
  return first<UserRow>(
    env.DB,
    `SELECT u.id, u.github_id, u.login, u.avatar_url FROM web_sessions s JOIN users u ON u.id = s.user_id
     WHERE s.token_hash = ? AND s.expires_at > ?`,
    await tokenHash(token),
    now(),
  )
}

export const logout = async (env: Env, request: Request): Promise<void> => {
  const token = readCookie(request, SESSION_COOKIE)
  if (token) await run(env.DB, 'DELETE FROM web_sessions WHERE token_hash = ?', await tokenHash(token))
}

// MARK: 桌面端授权（PKCE）

export interface DesktopAuthorize {
  redirectUri: string
  state: string
  codeChallenge: string
  deviceId: string
}

const DEVICE_ID = /^[A-Z2-7]{26}$/
const CHALLENGE = /^[A-Za-z0-9_-]{43}$/

/** 只接受本机回环地址作为回调，桌面端在本机临时监听端口接收授权码。 */
const isLoopback = (value: string): boolean => {
  try {
    const u = new URL(value)
    return u.protocol === 'http:' && (u.hostname === '127.0.0.1' || u.hostname === 'localhost' || u.hostname === '[::1]') && u.port !== ''
  } catch {
    return false
  }
}

export const parseDesktopAuthorize = (params: URLSearchParams): DesktopAuthorize => {
  const get = (k: string) => params.get(k) ?? ''
  if (get('client_id') !== DESKTOP_CLIENT_ID) throw new HttpError(400, 'bad_client', '未知的客户端')
  if (get('response_type') !== 'code') throw new HttpError(400, 'bad_request', '不支持的授权类型')
  if (get('code_challenge_method') !== 'S256' || !CHALLENGE.test(get('code_challenge'))) throw new HttpError(400, 'bad_pkce', '缺少有效的 PKCE 参数')
  if (!isLoopback(get('redirect_uri'))) throw new HttpError(400, 'bad_redirect', '回调地址必须是本机地址')
  if (!DEVICE_ID.test(get('device_id'))) throw new HttpError(400, 'bad_device', '设备标识格式不正确')
  if (!get('state') || get('state').length > 200) throw new HttpError(400, 'bad_state', '缺少 state')
  return { redirectUri: get('redirect_uri'), state: get('state'), codeChallenge: get('code_challenge'), deviceId: get('device_id') }
}

export const approveDesktop = async (env: Env, user: UserRow, req: DesktopAuthorize): Promise<string> => {
  const code = randomToken(32)
  await run(
    env.DB,
    'INSERT INTO oauth_codes (code_hash, user_id, device_id, code_challenge, redirect_uri, expires_at) VALUES (?, ?, ?, ?, ?, ?)',
    await tokenHash(code), user.id, req.deviceId, req.codeChallenge, req.redirectUri, now() + CODE_TTL,
  )
  const url = new URL(req.redirectUri)
  url.searchParams.set('code', code)
  url.searchParams.set('state', req.state)
  return url.toString()
}

export const denyDesktop = (req: DesktopAuthorize): string => {
  const url = new URL(req.redirectUri)
  url.searchParams.set('error', 'access_denied')
  url.searchParams.set('state', req.state)
  return url.toString()
}

export interface TokenResponse {
  access_token: string
  refresh_token: string
  token_type: 'Bearer'
  expires_in: number
  user: { login: string; avatar_url: string | null }
}

/** 账号的有效限制（默认值叠加运营者为该账号设置的覆盖值） */
export const userLimits = async (env: Env, userId: string): Promise<Limits> => {
  const row = await first<{ limits: string | null }>(env.DB, 'SELECT limits FROM users WHERE id = ?', userId)
  return effectiveLimits(env, row?.limits)
}

const issue = async (env: Env, userId: string, deviceId: string, grantId: string | null, meta: DeviceMeta): Promise<TokenResponse> => {
  const access = ACCESS_PREFIX + randomToken(32)
  const refresh = REFRESH_PREFIX + randomToken(32)
  const t = now()
  if (grantId) {
    // 刷新：只在客户端上报了新值时更新设备信息
    await run(
      env.DB,
      `UPDATE device_grants SET access_hash = ?, access_expires_at = ?, refresh_hash = ?, refresh_expires_at = ?, last_used_at = ?,
         device_name = COALESCE(?, device_name), platform = COALESCE(?, platform), app_version = COALESCE(?, app_version) WHERE id = ?`,
      await tokenHash(access), t + ACCESS_TTL, await tokenHash(refresh), t + REFRESH_TTL, t, meta.device_name, meta.platform, meta.app_version, grantId,
    )
  } else {
    const limits = await userLimits(env, userId)
    // 同一台设备重新登录：保留它原有的名额（沿用旧授权的 limited），旧授权作废
    const previous = await first<{ limited: number }>(
      env.DB, `SELECT limited FROM device_grants WHERE user_id = ? AND device_id = ? AND ${ACTIVE} ORDER BY limited LIMIT 1`, userId, deviceId, t,
    )
    await run(env.DB, 'UPDATE device_grants SET revoked_at = ? WHERE user_id = ? AND device_id = ? AND revoked_at IS NULL', t, userId, deviceId)
    const others = await first<{ n: number; slots: number }>(
      env.DB,
      `SELECT COUNT(*) AS n, COALESCE(SUM(limited = 0), 0) AS slots FROM device_grants WHERE user_id = ? AND device_id != ? AND ${ACTIVE}`,
      userId, deviceId, t,
    )
    if ((others?.n ?? 0) >= HARD.grants) throw new HttpError(403, 'too_many_devices', `每个账号最多 ${HARD.grants} 台已登录的设备，请先在官网退出不用的设备`)
    // 新设备：其他设备已占满名额时，以受限状态登录（只能管理设备，不能访问保险库）
    const limited = previous ? previous.limited : (others?.slots ?? 0) >= limits.max_devices ? 1 : 0
    await run(
      env.DB,
      `INSERT INTO device_grants (id, user_id, device_id, access_hash, access_expires_at, refresh_hash, refresh_expires_at, created_at, last_used_at,
         device_name, platform, app_version, limited)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      `g_${randomToken(12)}`, userId, deviceId, await tokenHash(access), t + ACCESS_TTL, await tokenHash(refresh), t + REFRESH_TTL, t, t,
      meta.device_name, meta.platform, meta.app_version, limited,
    )
    await audit(env.DB, userId, 'device_authorized', deviceId)
  }
  const user = await first<{ login: string; avatar_url: string | null }>(env.DB, 'SELECT login, avatar_url FROM users WHERE id = ?', userId)
  return { access_token: access, refresh_token: refresh, token_type: 'Bearer', expires_in: ACCESS_TTL / 1000, user: user ?? { login: '', avatar_url: null } }
}

const pkceMatches = async (verifier: string, challenge: string): Promise<boolean> =>
  /^[A-Za-z0-9._~-]{43,128}$/.test(verifier) && b64u.encode(await sha256(verifier)) === challenge

export const tokenEndpoint = async (env: Env, body: Record<string, string>): Promise<TokenResponse> => {
  const meta = deviceMeta(body)
  if (body.grant_type === 'authorization_code') {
    const hash = await tokenHash(body.code ?? '')
    const row = await first<{ user_id: string; device_id: string; code_challenge: string; redirect_uri: string; expires_at: number }>(
      env.DB, 'SELECT user_id, device_id, code_challenge, redirect_uri, expires_at FROM oauth_codes WHERE code_hash = ?', hash,
    )
    await run(env.DB, 'DELETE FROM oauth_codes WHERE code_hash = ?', hash)
    if (!row || row.expires_at < now()) throw new HttpError(400, 'invalid_grant', '授权码无效或已过期，请重新登录')
    if (row.redirect_uri !== body.redirect_uri || row.device_id !== body.device_id) throw new HttpError(400, 'invalid_grant', '授权码与请求不匹配')
    if (!(await pkceMatches(body.code_verifier ?? '', row.code_challenge))) throw new HttpError(400, 'invalid_grant', 'PKCE 校验失败')
    return issue(env, row.user_id, row.device_id, null, meta)
  }
  if (body.grant_type === 'refresh_token') {
    const token = body.refresh_token ?? ''
    if (!token.startsWith(REFRESH_PREFIX)) throw new HttpError(400, 'invalid_grant', '刷新令牌无效')
    const grant = await first<{ id: string; user_id: string; device_id: string; refresh_expires_at: number; revoked_at: number | null }>(
      env.DB, 'SELECT id, user_id, device_id, refresh_expires_at, revoked_at FROM device_grants WHERE refresh_hash = ?', await tokenHash(token),
    )
    if (!grant || grant.revoked_at !== null || grant.refresh_expires_at < now()) throw new HttpError(400, 'invalid_grant', '登录已失效，请重新登录')
    return issue(env, grant.user_id, grant.device_id, grant.id, meta)
  }
  throw new HttpError(400, 'unsupported_grant_type', '不支持的授权类型')
}

export const revokeToken = async (env: Env, token: string): Promise<void> => {
  const hash = await tokenHash(token)
  await run(env.DB, 'UPDATE device_grants SET revoked_at = ? WHERE (access_hash = ? OR refresh_hash = ?) AND revoked_at IS NULL', now(), hash, hash)
}

export interface DeviceAuth {
  grantId: string
  userId: string
  deviceId: string
  /** 超出设备数量限制（见 enforceDeviceLimit） */
  limited: boolean
  limits: Limits
}

/** 校验 Authorization: Bearer 访问令牌。账号的覆盖值随授权一起读出，不额外查询。 */
export const bearerAuth = async (env: Env, request: Request): Promise<DeviceAuth> => {
  const header = request.headers.get('authorization') ?? ''
  const token = header.startsWith('Bearer ') ? header.slice(7) : ''
  if (!token.startsWith(ACCESS_PREFIX)) throw new HttpError(401, 'unauthorized', '需要登录')
  const t = now()
  const grant = await first<{ id: string; user_id: string; device_id: string; access_expires_at: number; revoked_at: number | null; last_used_at: number; limited: number; limits: string | null }>(
    env.DB,
    `SELECT g.id, g.user_id, g.device_id, g.access_expires_at, g.revoked_at, g.last_used_at, g.limited, u.limits
     FROM device_grants g JOIN users u ON u.id = g.user_id WHERE g.access_hash = ?`,
    await tokenHash(token),
  )
  if (!grant || grant.revoked_at !== null) throw new HttpError(401, 'unauthorized', '登录已失效，请重新登录')
  if (grant.access_expires_at < t) throw new HttpError(401, 'token_expired', '访问令牌已过期')
  if (t - grant.last_used_at > TOUCH_INTERVAL) await run(env.DB, 'UPDATE device_grants SET last_used_at = ? WHERE id = ?', t, grant.id)
  return { grantId: grant.id, userId: grant.user_id, deviceId: grant.device_id, limited: grant.limited === 1, limits: effectiveLimits(env, grant.limits) }
}

/**
 * 受限设备访问保险库前重新计数：其他设备腾出名额后自动解除受限（条件更新，在一条语句内完成计数与更新），
 * 否则返回 403 device_limit。
 */
export const enforceDeviceLimit = async (env: Env, auth: DeviceAuth): Promise<void> => {
  if (!auth.limited) return
  const res = await run(
    env.DB,
    `UPDATE device_grants SET limited = 0 WHERE id = ? AND limited = 1 AND
       (SELECT COUNT(*) FROM device_grants WHERE user_id = ? AND id != ? AND limited = 0 AND ${ACTIVE}) < ?`,
    auth.grantId, auth.userId, auth.grantId, now(), auth.limits.max_devices,
  )
  if (res.meta.changes === 1) {
    auth.limited = false
    return
  }
  throw new HttpError(403, 'device_limit', `此账号最多同时在 ${auth.limits.max_devices} 台设备上同步，请先移除不用的设备`, { max_devices: auth.limits.max_devices })
}

// MARK: 账号设备（桌面端）

export interface AccountDevice {
  grant_id: string
  device_id: string
  device_name: string | null
  platform: string | null
  app_version: string | null
  /** unix 秒 */
  created_at: number
  last_seen_at: number
  is_self: boolean
  limited: boolean
}

export const accountDevices = async (env: Env, auth: DeviceAuth): Promise<{ max_devices: number; devices: AccountDevice[] }> => {
  const rows = await all<GrantRow>(
    env.DB,
    `SELECT id, user_id, device_id, device_name, platform, app_version, limited, created_at, last_used_at, revoked_at FROM device_grants
     WHERE user_id = ? AND ${ACTIVE} ORDER BY last_used_at DESC LIMIT ${HARD.grants}`,
    auth.userId, now(),
  )
  return {
    max_devices: auth.limits.max_devices,
    devices: rows.map((g) => ({
      grant_id: g.id,
      device_id: g.device_id,
      device_name: g.device_name,
      platform: g.platform,
      app_version: g.app_version,
      created_at: Math.floor(g.created_at / 1000),
      last_seen_at: Math.floor(g.last_used_at / 1000),
      is_self: g.id === auth.grantId,
      limited: g.limited === 1,
    })),
  }
}

/** 吊销本账号的一个设备授权（可以是本机自己）。 */
export const releaseDevice = async (env: Env, auth: DeviceAuth, grantId: string): Promise<{ is_self: boolean }> => {
  const res = await run(env.DB, 'UPDATE device_grants SET revoked_at = ? WHERE id = ? AND user_id = ? AND revoked_at IS NULL', now(), grantId, auth.userId)
  if (res.meta.changes !== 1) throw new HttpError(404, 'device_not_found', '设备不存在或已退出')
  await audit(env.DB, auth.userId, 'device_released', grantId)
  return { is_self: grantId === auth.grantId }
}

// MARK: 账号页面

export const listGrants = (env: Env, userId: string): Promise<GrantRow[]> =>
  all<GrantRow>(
    env.DB,
    `SELECT id, user_id, device_id, device_name, platform, app_version, limited, created_at, last_used_at, revoked_at FROM device_grants
     WHERE user_id = ? AND ${ACTIVE} ORDER BY last_used_at DESC`,
    userId, now(),
  )

export const revokeGrant = async (env: Env, userId: string, grantId: string): Promise<void> => {
  await run(env.DB, 'UPDATE device_grants SET revoked_at = ? WHERE id = ? AND user_id = ? AND revoked_at IS NULL', now(), grantId, userId)
  await audit(env.DB, userId, 'device_revoked', grantId)
}

/** 删除账号：会话、授权、保险库全部级联删除。 */
export const deleteAccount = async (env: Env, userId: string): Promise<void> => {
  await audit(env.DB, null, 'account_deleted')
  await run(env.DB, 'DELETE FROM users WHERE id = ?', userId)
}
