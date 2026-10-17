// 访问控制（docs/sync-design.md 第 9 节）：登录只决定能否调用 API，与能否解密无关。
// 官网：GitHub OAuth + 会话 Cookie；桌面端：浏览器授权码 + PKCE（S256），每台设备一条授权。
import { all, audit, first, run } from './db.js'
import { HttpError, b64u, now, randomToken, safeReturnPath, sha256, tokenHash } from './http.js'

const DAY = 24 * 60 * 60 * 1000
const SESSION_TTL = 30 * DAY
const LOGIN_STATE_TTL = 10 * 60 * 1000
const CODE_TTL = 5 * 60 * 1000
const ACCESS_TTL = 60 * 60 * 1000
const REFRESH_TTL = 90 * DAY
/** 最后活跃时间最多每 10 分钟写一次，节省 D1 写入 */
const TOUCH_INTERVAL = 10 * 60 * 1000
/** 每个账号同时有效的设备授权上限（docs/sync-design.md 第 11 节） */
export const MAX_DEVICES = 10

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
  created_at: number
  last_used_at: number
  revoked_at: number | null
}

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

const issue = async (env: Env, userId: string, deviceId: string, grantId: string | null): Promise<TokenResponse> => {
  const access = ACCESS_PREFIX + randomToken(32)
  const refresh = REFRESH_PREFIX + randomToken(32)
  const t = now()
  if (grantId) {
    await run(
      env.DB,
      'UPDATE device_grants SET access_hash = ?, access_expires_at = ?, refresh_hash = ?, refresh_expires_at = ?, last_used_at = ? WHERE id = ?',
      await tokenHash(access), t + ACCESS_TTL, await tokenHash(refresh), t + REFRESH_TTL, t, grantId,
    )
  } else {
    // 同一台设备重新登录时，旧授权作废
    await run(env.DB, 'UPDATE device_grants SET revoked_at = ? WHERE user_id = ? AND device_id = ? AND revoked_at IS NULL', t, userId, deviceId)
    const active = await first<{ n: number }>(env.DB, 'SELECT COUNT(*) AS n FROM device_grants WHERE user_id = ? AND revoked_at IS NULL', userId)
    if ((active?.n ?? 0) >= MAX_DEVICES) throw new HttpError(403, 'too_many_devices', `每个账号最多 ${MAX_DEVICES} 台设备，请先在官网移除不用的设备`)
    await run(
      env.DB,
      `INSERT INTO device_grants (id, user_id, device_id, access_hash, access_expires_at, refresh_hash, refresh_expires_at, created_at, last_used_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      `g_${randomToken(12)}`, userId, deviceId, await tokenHash(access), t + ACCESS_TTL, await tokenHash(refresh), t + REFRESH_TTL, t, t,
    )
    await audit(env.DB, userId, 'device_authorized', deviceId)
  }
  const user = await first<{ login: string; avatar_url: string | null }>(env.DB, 'SELECT login, avatar_url FROM users WHERE id = ?', userId)
  return { access_token: access, refresh_token: refresh, token_type: 'Bearer', expires_in: ACCESS_TTL / 1000, user: user ?? { login: '', avatar_url: null } }
}

const pkceMatches = async (verifier: string, challenge: string): Promise<boolean> =>
  /^[A-Za-z0-9._~-]{43,128}$/.test(verifier) && b64u.encode(await sha256(verifier)) === challenge

export const tokenEndpoint = async (env: Env, body: Record<string, string>): Promise<TokenResponse> => {
  if (body.grant_type === 'authorization_code') {
    const hash = await tokenHash(body.code ?? '')
    const row = await first<{ user_id: string; device_id: string; code_challenge: string; redirect_uri: string; expires_at: number }>(
      env.DB, 'SELECT user_id, device_id, code_challenge, redirect_uri, expires_at FROM oauth_codes WHERE code_hash = ?', hash,
    )
    await run(env.DB, 'DELETE FROM oauth_codes WHERE code_hash = ?', hash)
    if (!row || row.expires_at < now()) throw new HttpError(400, 'invalid_grant', '授权码无效或已过期，请重新登录')
    if (row.redirect_uri !== body.redirect_uri || row.device_id !== body.device_id) throw new HttpError(400, 'invalid_grant', '授权码与请求不匹配')
    if (!(await pkceMatches(body.code_verifier ?? '', row.code_challenge))) throw new HttpError(400, 'invalid_grant', 'PKCE 校验失败')
    return issue(env, row.user_id, row.device_id, null)
  }
  if (body.grant_type === 'refresh_token') {
    const token = body.refresh_token ?? ''
    if (!token.startsWith(REFRESH_PREFIX)) throw new HttpError(400, 'invalid_grant', '刷新令牌无效')
    const grant = await first<{ id: string; user_id: string; device_id: string; refresh_expires_at: number; revoked_at: number | null }>(
      env.DB, 'SELECT id, user_id, device_id, refresh_expires_at, revoked_at FROM device_grants WHERE refresh_hash = ?', await tokenHash(token),
    )
    if (!grant || grant.revoked_at !== null || grant.refresh_expires_at < now()) throw new HttpError(400, 'invalid_grant', '登录已失效，请重新登录')
    return issue(env, grant.user_id, grant.device_id, grant.id)
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
}

/** 校验 Authorization: Bearer 访问令牌。 */
export const bearerAuth = async (env: Env, request: Request): Promise<DeviceAuth> => {
  const header = request.headers.get('authorization') ?? ''
  const token = header.startsWith('Bearer ') ? header.slice(7) : ''
  if (!token.startsWith(ACCESS_PREFIX)) throw new HttpError(401, 'unauthorized', '需要登录')
  const t = now()
  const grant = await first<{ id: string; user_id: string; device_id: string; access_expires_at: number; revoked_at: number | null; last_used_at: number }>(
    env.DB, 'SELECT id, user_id, device_id, access_expires_at, revoked_at, last_used_at FROM device_grants WHERE access_hash = ?', await tokenHash(token),
  )
  if (!grant || grant.revoked_at !== null) throw new HttpError(401, 'unauthorized', '登录已失效，请重新登录')
  if (grant.access_expires_at < t) throw new HttpError(401, 'token_expired', '访问令牌已过期')
  if (t - grant.last_used_at > TOUCH_INTERVAL) await run(env.DB, 'UPDATE device_grants SET last_used_at = ? WHERE id = ?', t, grant.id)
  return { grantId: grant.id, userId: grant.user_id, deviceId: grant.device_id }
}

// MARK: 账号页面

export const listGrants = (env: Env, userId: string): Promise<GrantRow[]> =>
  all<GrantRow>(env.DB, 'SELECT id, user_id, device_id, created_at, last_used_at, revoked_at FROM device_grants WHERE user_id = ? AND revoked_at IS NULL ORDER BY last_used_at DESC', userId)

export const revokeGrant = async (env: Env, userId: string, grantId: string): Promise<void> => {
  await run(env.DB, 'UPDATE device_grants SET revoked_at = ? WHERE id = ? AND user_id = ? AND revoked_at IS NULL', now(), grantId, userId)
  await audit(env.DB, userId, 'device_revoked', grantId)
}

/** 删除账号：会话、授权、保险库全部级联删除。 */
export const deleteAccount = async (env: Env, userId: string): Promise<void> => {
  await audit(env.DB, null, 'account_deleted')
  await run(env.DB, 'DELETE FROM users WHERE id = ?', userId)
}
