import { Hono, type Context } from 'hono'
import {
  accountDevices, bearerAuth, enforceDeviceLimit, finishGitHubLogin, logout, releaseDevice, revokeToken, sessionCookie, startGitHubLogin, tokenEndpoint,
  type DeviceAuth,
} from './auth.js'
import { HttpError, safeReturnPath } from './http.js'
import { HARD, policyOf } from './limits.js'
import { downloadFile, latestRelease, publicLatestRelease } from './releases.js'
import {
  activeJoins, appendRoster, cancelJoin, changes, createJoin, createVault, envelopesFor, getJoin, isJoinStep,
  ownedVault, pendingJoins, postJoinStep, pushRecords, putEnvelope, raiseMinEpoch, rosterSince, vaultOf, type VaultRow,
} from './sync.js'

type AppEnv = { Bindings: Env; Variables: { auth: DeviceAuth } }

export const handlesPath = (pathname: string): boolean =>
  pathname.startsWith('/api/') || pathname.startsWith('/auth/') || pathname.startsWith('/download/')

export const api = new Hono<AppEnv>()

api.onError((err, c) => {
  if (err instanceof HttpError) return c.json({ ...err.extra, error: err.code, message: err.message }, err.status as 400)
  console.error('api error', err instanceof Error ? err.message : 'unknown')
  return c.json({ error: 'internal', message: '服务暂时不可用，请稍后重试' }, 500)
})

const limit = async (c: Context<AppEnv>, kind: 'read' | 'write') => {
  const key = c.get('auth')?.grantId ?? c.req.header('cf-connecting-ip') ?? 'anon'
  const limiter = kind === 'write' ? c.env.WRITE_LIMITER : c.env.READ_LIMITER
  const { success } = await limiter.limit({ key })
  if (!success) throw new HttpError(429, 'rate_limited', '请求过于频繁，请稍后再试')
}

// MARK: 公开接口

api.get('/api/v1/health', (c) => c.json({ ok: true, env: c.env.QANTRIP_ENV }))
api.get('/api/v1/releases/latest', async (c) => c.json(await latestRelease(c.env)))
// 客户端检查更新：不需要登录，返回 GitHub 最新正式版本（服务端缓存）
api.get('/api/v1/update', async (c) => {
  const body = await publicLatestRelease(c.executionCtx)
  return c.json(body, 200, { 'cache-control': 'public, max-age=600' })
})
api.get('/download/:file', (c) => downloadFile(c.env, c.req.param('file')))

// MARK: 官网登录

api.get('/auth/github', async (c) => {
  try {
    return c.redirect(await startGitHubLogin(c.env, c.req.query('return') ?? null))
  } catch (e) {
    if (e instanceof HttpError && e.code === 'login_unavailable') return c.redirect('/account?login=unavailable')
    throw e
  }
})

api.get('/auth/github/callback', async (c) => {
  const code = c.req.query('code')
  const state = c.req.query('state')
  if (!code || !state) return c.redirect('/account?login=failed')
  try {
    const { session, returnTo } = await finishGitHubLogin(c.env, code, state)
    c.header('set-cookie', sessionCookie(session))
    return c.redirect(safeReturnPath(returnTo))
  } catch (e) {
    if (e instanceof HttpError) return c.redirect('/account?login=failed')
    throw e
  }
})

api.post('/auth/logout', async (c) => {
  await logout(c.env, c.req.raw)
  c.header('set-cookie', sessionCookie('', 0))
  return c.redirect('/')
})

// MARK: 桌面端令牌

const formOrJson = async (c: Context<AppEnv>): Promise<Record<string, string>> => {
  const type = c.req.header('content-type') ?? ''
  if (type.includes('application/json')) return (await c.req.json()) as Record<string, string>
  const form = await c.req.parseBody()
  return Object.fromEntries(Object.entries(form).map(([k, v]) => [k, String(v)]))
}

api.post('/api/v1/oauth/token', async (c) => {
  await limit(c, 'write')
  return c.json(await tokenEndpoint(c.env, await formOrJson(c)))
})

api.post('/api/v1/oauth/revoke', async (c) => {
  const body = await formOrJson(c)
  if (body.token) await revokeToken(c.env, body.token)
  return c.body(null, 200)
})

// MARK: 同步（需要设备访问令牌）

const authed = new Hono<AppEnv>()
authed.use('*', async (c, next) => {
  const auth = await bearerAuth(c.env, c.req.raw)
  c.set('auth', auth)
  // 超出设备数量限制的设备只能使用账号接口（查看与移除设备），不能访问保险库
  if (!c.req.path.startsWith('/api/v1/account')) await enforceDeviceLimit(c.env, auth)
  await next()
})

const usageOf = (v: VaultRow, auth: DeviceAuth) => ({
  bytes: v.byte_count,
  quota_bytes: Math.min(auth.limits.quota_bytes, HARD.vaultBytes),
  records: v.record_count,
})

// MARK: 账号

authed.get('/account', async (c) => {
  await limit(c, 'read')
  const auth = c.get('auth')
  const [user, vault] = await Promise.all([
    c.env.DB.prepare('SELECT login FROM users WHERE id = ?').bind(auth.userId).first<{ login: string }>(),
    vaultOf(c.env.DB, auth.userId),
  ])
  return c.json({ login: user?.login ?? '', limits: auth.limits, usage: vault ? usageOf(vault, auth) : null })
})

authed.get('/account/devices', async (c) => {
  await limit(c, 'read')
  return c.json(await accountDevices(c.env, c.get('auth')))
})

authed.delete('/account/devices/:grant', async (c) => {
  await limit(c, 'write')
  return c.json(await releaseDevice(c.env, c.get('auth'), c.req.param('grant')))
})

authed.get('/vault', async (c) => {
  await limit(c, 'read')
  const vault = await vaultOf(c.env.DB, c.get('auth').userId)
  return vault ? c.json({ vault_id: vault.id, seq: vault.seq, roster_len: vault.roster_len, min_epoch: vault.min_epoch }) : c.json({ vault_id: null })
})

authed.post('/vault', async (c) => {
  await limit(c, 'write')
  const vault = await createVault(c.env, c.get('auth').userId, c.req.header('cf-connecting-ip') ?? 'unknown')
  return c.json({ vault_id: vault.id, seq: vault.seq, roster_len: vault.roster_len, min_epoch: vault.min_epoch }, 201)
})

const vault = (c: Context<AppEnv>) => ownedVault(c.env.DB, c.get('auth').userId, c.req.param('vault')!)

authed.get('/vaults/:vault/head', async (c) => {
  await limit(c, 'read')
  const v = await vault(c)
  const auth = c.get('auth')
  return c.json({
    seq: v.seq,
    roster_index: v.roster_len,
    min_epoch: v.min_epoch,
    pending_joins: await pendingJoins(c.env, v.id, auth.deviceId),
    usage: usageOf(v, auth),
    policy: policyOf(auth.limits),
  })
})

authed.get('/vaults/:vault/changes', async (c) => {
  await limit(c, 'read')
  return c.json(await changes(c.env, await vault(c), Number(c.req.query('since') ?? 0), Number(c.req.query('limit') ?? 200)))
})

authed.post('/vaults/:vault/records', async (c) => {
  await limit(c, 'write')
  const v = await vault(c)
  const auth = c.get('auth')
  return c.json({ results: await pushRecords(c.env, v.id, auth.deviceId, auth.limits.quota_bytes, await c.req.json()) })
})

authed.post('/vaults/:vault/min-epoch', async (c) => {
  await limit(c, 'write')
  const v = await vault(c)
  const { epoch } = (await c.req.json()) as { epoch: number }
  return c.json({ min_epoch: await raiseMinEpoch(c.env, v.id, epoch) })
})

authed.get('/vaults/:vault/roster', async (c) => {
  await limit(c, 'read')
  const v = await vault(c)
  return c.json({ entries: await rosterSince(c.env, v.id, Number(c.req.query('since') ?? 0)), length: v.roster_len })
})

authed.post('/vaults/:vault/roster', async (c) => {
  await limit(c, 'write')
  const v = await vault(c)
  return c.json({ index: await appendRoster(c.env, v.id, await c.req.json()) }, 201)
})

authed.put('/vaults/:vault/envelopes/:recipient/:epoch', async (c) => {
  await limit(c, 'write')
  const v = await vault(c)
  await putEnvelope(c.env, v.id, c.req.param('recipient'), Number(c.req.param('epoch')), await c.req.json())
  return c.body(null, 204)
})

authed.get('/vaults/:vault/envelopes/:recipient', async (c) => {
  await limit(c, 'read')
  const v = await vault(c)
  return c.json({ envelopes: await envelopesFor(c.env, v.id, c.req.param('recipient')) })
})

authed.post('/vaults/:vault/joins', async (c) => {
  await limit(c, 'write')
  const v = await vault(c)
  return c.json(await createJoin(c.env, v.id, c.get('auth').deviceId, await c.req.json()), 201)
})

authed.get('/vaults/:vault/joins', async (c) => {
  await limit(c, 'read')
  const v = await vault(c)
  return c.json({ joins: await activeJoins(c.env, v.id) })
})

authed.get('/vaults/:vault/joins/:join', async (c) => {
  await limit(c, 'read')
  const v = await vault(c)
  return c.json(await getJoin(c.env, v.id, c.req.param('join')))
})

authed.put('/vaults/:vault/joins/:join/:step', async (c) => {
  await limit(c, 'write')
  const v = await vault(c)
  const step = c.req.param('step')
  if (!isJoinStep(step)) throw new HttpError(404, 'not_found', '未知的步骤')
  await postJoinStep(c.env, v.id, c.req.param('join'), step, await c.req.json())
  return c.body(null, 204)
})

authed.delete('/vaults/:vault/joins/:join', async (c) => {
  await limit(c, 'write')
  const v = await vault(c)
  await cancelJoin(c.env, v.id, c.req.param('join'))
  return c.body(null, 204)
})

api.route('/api/v1', authed)

api.all('/api/*', (c) => c.json({ error: 'not_found', message: '接口不存在' }, 404))
