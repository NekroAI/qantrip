import { env } from 'cloudflare:test'
import { describe, expect, it } from 'vitest'
import { api } from '../server/api.js'
import { approveDesktop, parseDesktopAuthorize, type UserRow } from '../server/auth.js'
import { dailyCleanup, TOMBSTONE_TTL } from '../server/cleanup.js'
import { b64u, sha256 } from '../server/http.js'

const ctx = { waitUntil: () => undefined, passThroughOnException: () => undefined, props: {} } as unknown as ExecutionContext
const call = (path: string, init?: RequestInit) => api.request(path, init, env, ctx)
const json = (body: unknown, token?: string, method = 'POST'): RequestInit => ({
  method,
  headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
  body: JSON.stringify(body),
})
const get = (token: string): RequestInit => ({ headers: { authorization: `Bearer ${token}` } })

const VERIFIER = 'test-verifier-0123456789-abcdefghijklmnopqrstuvwxyz-ABCDEF'
let counter = 0
const deviceId = () => `DEVICE${String(++counter).padStart(4, 'A').replace(/\d/g, (d) => 'ABCDEFGHIJ'[Number(d)]!)}${'A'.repeat(16)}`.slice(0, 26)

const makeUser = async (): Promise<UserRow> => {
  const id = `u_test_${++counter}`
  await env.DB.prepare('INSERT INTO users (id, github_id, login, avatar_url, created_at, last_login_at) VALUES (?, ?, ?, NULL, 0, 0)')
    .bind(id, 1000 + counter, `user${counter}`)
    .run()
  return { id, github_id: 1000 + counter, login: `user${counter}`, avatar_url: null }
}

const authorizeParams = async (device: string, overrides: Record<string, string> = {}) =>
  new URLSearchParams({
    client_id: 'qantrip-desktop',
    response_type: 'code',
    code_challenge_method: 'S256',
    code_challenge: b64u.encode(await sha256(VERIFIER)),
    redirect_uri: 'http://127.0.0.1:53682/callback',
    state: 'state-1234',
    device_id: device,
    ...overrides,
  })

/** 走一遍桌面端 PKCE 登录，返回访问令牌与刷新令牌 */
const signIn = async (user?: UserRow) => {
  const u = user ?? (await makeUser())
  const device = deviceId()
  const req = parseDesktopAuthorize(await authorizeParams(device))
  const code = new URL(await approveDesktop(env, u, req)).searchParams.get('code')!
  const res = await call('/api/v1/oauth/token', json({ grant_type: 'authorization_code', code, code_verifier: VERIFIER, redirect_uri: req.redirectUri, device_id: device }))
  expect(res.status).toBe(200)
  const tokens = (await res.json()) as { access_token: string; refresh_token: string }
  return { user: u, device, ...tokens }
}

// 每个测试使用不同的来源 IP，避免触发"每个 IP 每天 5 个保险库"的限制（该限制另有专门测试）
const newVault = async (token: string) => {
  const init = json({}, token)
  init.headers = { ...(init.headers as Record<string, string>), 'cf-connecting-ip': `10.0.0.${++counter}` }
  return ((await (await call('/api/v1/vault', init)).json()) as { vault_id: string }).vault_id
}
const rid = () => b64u.encode(crypto.getRandomValues(new Uint8Array(16)))
const blob = (n = 64) => b64u.encode(n <= 65536 ? crypto.getRandomValues(new Uint8Array(n)) : new Uint8Array(n).fill(7))
const write = (record_id: string, base_seq: number | null, extra: Record<string, unknown> = {}) => ({
  record_id, base_seq, epoch: 1, ciphertext: blob(), deleted: false, idempotency_key: `k-${rid()}`, ...extra,
})
type Results = { results: { record_id: string; status: string; seq?: number; current?: { seq: number } }[] }
const push = async (token: string, vault: string, writes: unknown[]) =>
  (await (await call(`/api/v1/vaults/${vault}/records`, json({ writes }, token))).json()) as Results

describe('桌面端授权', () => {
  it('只接受本机回调地址与有效的 PKCE 参数', async () => {
    const d = deviceId()
    expect(() => parseDesktopAuthorize(new URLSearchParams())).toThrow()
    await expect(async () => parseDesktopAuthorize(await authorizeParams(d, { redirect_uri: 'https://evil.example/cb' }))).rejects.toThrow('本机')
    await expect(async () => parseDesktopAuthorize(await authorizeParams(d, { code_challenge_method: 'plain' }))).rejects.toThrow('PKCE')
    await expect(async () => parseDesktopAuthorize(await authorizeParams('bad'))).rejects.toThrow('设备')
  })

  it('授权码只能用一次，且必须匹配 verifier 与设备', async () => {
    const user = await makeUser()
    const device = deviceId()
    const req = parseDesktopAuthorize(await authorizeParams(device))
    const code = new URL(await approveDesktop(env, user, req)).searchParams.get('code')!
    const bad = await call('/api/v1/oauth/token', json({ grant_type: 'authorization_code', code, code_verifier: 'x'.repeat(50), redirect_uri: req.redirectUri, device_id: device }))
    expect(bad.status).toBe(400)
    // 失败后授权码已作废
    const again = await call('/api/v1/oauth/token', json({ grant_type: 'authorization_code', code, code_verifier: VERIFIER, redirect_uri: req.redirectUri, device_id: device }))
    expect(again.status).toBe(400)
  })

  it('刷新令牌轮换，旧令牌失效；撤销后访问被拒绝', async () => {
    const s = await signIn()
    const r1 = await call('/api/v1/oauth/token', json({ grant_type: 'refresh_token', refresh_token: s.refresh_token }))
    expect(r1.status).toBe(200)
    const t1 = (await r1.json()) as { access_token: string; refresh_token: string }
    expect((await call('/api/v1/oauth/token', json({ grant_type: 'refresh_token', refresh_token: s.refresh_token }))).status).toBe(400)
    expect((await call('/api/v1/vault', get(t1.access_token))).status).toBe(200)
    await call('/api/v1/oauth/revoke', json({ token: t1.access_token }))
    expect((await call('/api/v1/vault', get(t1.access_token))).status).toBe(401)
  })
})

describe('保险库与记录', () => {
  it('不同账号之间完全隔离', async () => {
    const a = await signIn()
    const b = await signIn()
    const vault = await newVault(a.access_token)
    expect((await call(`/api/v1/vaults/${vault}/head`, get(b.access_token))).status).toBe(404)
    expect((await call(`/api/v1/vaults/${vault}/records`, json({ writes: [write(rid(), null)] }, b.access_token))).status).toBe(404)
  })

  it('CAS：基于过期版本的写入返回冲突与当前版本', async () => {
    const s = await signIn()
    const vault = await newVault(s.access_token)
    const id = rid()
    const created = await push(s.access_token, vault, [write(id, null)])
    expect(created.results[0]).toMatchObject({ status: 'ok', seq: 1 })
    const updated = await push(s.access_token, vault, [write(id, 1)])
    expect(updated.results[0]).toMatchObject({ status: 'ok', seq: 2 })
    const stale = await push(s.access_token, vault, [write(id, 1)])
    expect(stale.results[0]!.status).toBe('conflict')
    expect(stale.results[0]!.current!.seq).toBe(2)
    // 新建已存在的记录同样是冲突
    expect((await push(s.access_token, vault, [write(id, null)])).results[0]!.status).toBe('conflict')
  })

  it('幂等：同一个幂等键重试不会重复写入', async () => {
    const s = await signIn()
    const vault = await newVault(s.access_token)
    const w = write(rid(), null)
    const first = await push(s.access_token, vault, [w])
    const retry = await push(s.access_token, vault, [w])
    expect(retry).toEqual(first)
    const head = (await (await call(`/api/v1/vaults/${vault}/head`, get(s.access_token))).json()) as { seq: number }
    expect(head.seq).toBe(1)
  })

  it('限制：单条记录大小、每批条数、epoch 下限', async () => {
    const s = await signIn()
    const vault = await newVault(s.access_token)
    const big = await push(s.access_token, vault, [write(rid(), null, { ciphertext: blob(256 * 1024 + 1) })])
    expect(big.results[0]!.status).toBe('invalid')
    const batch = await call(`/api/v1/vaults/${vault}/records`, json({ writes: Array.from({ length: 51 }, () => write(rid(), null)) }, s.access_token))
    expect(batch.status).toBe(400)
    await call(`/api/v1/vaults/${vault}/min-epoch`, json({ epoch: 3 }, s.access_token))
    expect((await push(s.access_token, vault, [write(rid(), null, { epoch: 2 })])).results[0]!.status).toBe('stale_epoch')
    // 下限只增不减
    const lowered = (await (await call(`/api/v1/vaults/${vault}/min-epoch`, json({ epoch: 1 }, s.access_token))).json()) as { min_epoch: number }
    expect(lowered.min_epoch).toBe(3)
  })

  it('增量拉取按 seq 分页', async () => {
    const s = await signIn()
    const vault = await newVault(s.access_token)
    await push(s.access_token, vault, Array.from({ length: 5 }, () => write(rid(), null)))
    const p1 = (await (await call(`/api/v1/vaults/${vault}/changes?since=0&limit=2`, get(s.access_token))).json()) as { records: { seq: number }[]; next: number | null; head: number }
    expect(p1.records.map((r) => r.seq)).toEqual([1, 2])
    expect(p1.next).toBe(2)
    expect(p1.head).toBe(5)
    const p3 = (await (await call(`/api/v1/vaults/${vault}/changes?since=4&limit=2`, get(s.access_token))).json()) as { records: { seq: number }[]; next: number | null }
    expect(p3.records.map((r) => r.seq)).toEqual([5])
    expect(p3.next).toBeNull()
  })

  it('墓碑清理后，基于旧版本的写入返回 gone', async () => {
    const s = await signIn()
    const vault = await newVault(s.access_token)
    const id = rid()
    await push(s.access_token, vault, [write(id, null)])
    await push(s.access_token, vault, [write(id, 1, { deleted: true })])
    await env.DB.prepare('UPDATE records SET updated_at = ? WHERE vault_id = ?').bind(Date.now() - TOMBSTONE_TTL - 1000, vault).run()
    await dailyCleanup(env)
    const res = await push(s.access_token, vault, [write(id, 2)])
    expect(res.results[0]!.status).toBe('gone')
    const head = (await (await call(`/api/v1/vaults/${vault}/changes?since=0`, get(s.access_token))).json()) as { purged_seq: number; records: unknown[] }
    expect(head.purged_seq).toBe(2)
    expect(head.records).toHaveLength(0)
  })
})

describe('保险库创建限制', () => {
  it('同一 IP 每天最多新建 5 个保险库', async () => {
    const ip = '192.0.2.77'
    const create = async () => {
      const s = await signIn()
      const init = json({}, s.access_token)
      init.headers = { ...(init.headers as Record<string, string>), 'cf-connecting-ip': ip }
      return (await call('/api/v1/vault', init)).status
    }
    for (let i = 0; i < 5; i++) expect(await create()).toBe(201)
    expect(await create()).toBe(429)
  })
})

describe('花名册、信封与加入', () => {
  it('花名册追加使用 CAS', async () => {
    const s = await signIn()
    const vault = await newVault(s.access_token)
    expect((await call(`/api/v1/vaults/${vault}/roster`, json({ expected_index: 0, entry: blob(200) }, s.access_token))).status).toBe(201)
    expect((await call(`/api/v1/vaults/${vault}/roster`, json({ expected_index: 0, entry: blob(200) }, s.access_token))).status).toBe(409)
    expect((await call(`/api/v1/vaults/${vault}/roster`, json({ expected_index: 1, entry: blob(200) }, s.access_token))).status).toBe(201)
    const roster = (await (await call(`/api/v1/vaults/${vault}/roster?since=1`, get(s.access_token))).json()) as { entries: { index: number }[]; length: number }
    expect(roster.entries.map((e) => e.index)).toEqual([1])
    expect(roster.length).toBe(2)
  })

  it('信封按接收方与 epoch 保存', async () => {
    const s = await signIn()
    const vault = await newVault(s.access_token)
    expect((await call(`/api/v1/vaults/${vault}/envelopes/recovery/1`, json({ roster_index: 1, data: blob(120) }, s.access_token, 'PUT'))).status).toBe(204)
    expect((await call(`/api/v1/vaults/${vault}/envelopes/${s.device}/2`, json({ roster_index: 3, data: blob(120) }, s.access_token, 'PUT'))).status).toBe(204)
    const list = (await (await call(`/api/v1/vaults/${vault}/envelopes/${s.device}`, get(s.access_token))).json()) as { envelopes: { epoch: number }[] }
    expect(list.envelopes.map((e) => e.epoch)).toEqual([2])
    expect((await call(`/api/v1/vaults/${vault}/envelopes/not-a-device/1`, json({ roster_index: 1, data: blob() }, s.access_token, 'PUT'))).status).toBe(400)
  })

  it('加入请求：同时只允许一个，消息必须按顺序且只能写一次', async () => {
    const s = await signIn()
    const vault = await newVault(s.access_token)
    const created = (await (await call(`/api/v1/vaults/${vault}/joins`, json({ data: blob(64) }, s.access_token))).json()) as { join_id: string }
    expect((await call(`/api/v1/vaults/${vault}/joins`, json({ data: blob(64) }, s.access_token))).status).toBe(409)
    const step = (name: string) => call(`/api/v1/vaults/${vault}/joins/${created.join_id}/${name}`, json({ data: blob(64) }, s.access_token, 'PUT'))
    expect((await step('reveal')).status).toBe(409)
    expect((await step('reply')).status).toBe(204)
    expect((await step('reply')).status).toBe(409)
    expect((await step('reveal')).status).toBe(204)
    expect((await step('result')).status).toBe(204)
    const view = (await (await call(`/api/v1/vaults/${vault}/joins/${created.join_id}`, get(s.access_token))).json()) as Record<string, unknown>
    expect(view.result).toBeTypeOf('string')
  })
})

describe('公开接口', () => {
  it('下载开关关闭时，版本与下载接口不可用', async () => {
    expect((await call('/api/v1/releases/latest')).status).toBe(404)
    expect((await call('/download/Qantrip-0.1.0-arm64.dmg')).status).toBe(404)
    expect((await call('/api/v1/health')).status).toBe(200)
  })

  it('未登录访问同步接口返回 401', async () => {
    expect((await call('/api/v1/vault')).status).toBe(401)
    expect((await call('/api/v1/vault', get('qta_invalid'))).status).toBe(401)
  })
})

describe('seo', () => {
  const prod = { ...env, QANTRIP_ENV: 'production', SITE_ORIGIN: 'https://qantrip.nekro.ai' } as Env

  it('serves robots, sitemap and llms.txt; staging is not indexable', async () => {
    const { seoResponse } = await import('../server/seo.js')
    expect(await seoResponse('/robots.txt', prod)!.text()).toContain('Sitemap: https://qantrip.nekro.ai/sitemap.xml')
    expect(await seoResponse('/robots.txt', { ...prod, QANTRIP_ENV: 'staging' } as Env)!.text()).toContain('Disallow: /\n')
    const sitemap = await seoResponse('/sitemap.xml', prod)!.text()
    expect(sitemap).toContain('<loc>https://qantrip.nekro.ai/zh/docs</loc>')
    expect(sitemap).toContain('hreflang="zh-Hans"')
    const full = await seoResponse('/llms-full.txt', prod)!.text()
    expect(full).toContain('`review` (alias r)')
    expect(seoResponse('/docs', prod)).toBeNull()
  })

  it('redirects Chinese browsers to /zh unless English was chosen', async () => {
    const { localeRedirect } = await import('../server/seo.js')
    const req = (path: string, headers: Record<string, string>) => new Request(`https://qantrip.nekro.ai${path}`, { headers })
    expect(localeRedirect(req('/docs', { 'accept-language': 'zh-CN,zh;q=0.9' }))?.headers.get('location')).toBe('/zh/docs')
    expect(localeRedirect(req('/', { 'accept-language': 'zh-CN' }))?.headers.get('location')).toBe('/zh')
    expect(localeRedirect(req('/', { 'accept-language': 'zh-CN', cookie: 'lang=en' }))).toBeNull()
    expect(localeRedirect(req('/', { 'accept-language': 'en-US', cookie: 'lang=zh' }))?.headers.get('location')).toBe('/zh')
    expect(localeRedirect(req('/', { 'accept-language': 'en-US' }))).toBeNull()
    expect(localeRedirect(req('/account', { 'accept-language': 'zh-CN' }))).toBeNull()
  })
})
