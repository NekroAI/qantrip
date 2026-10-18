// 同步服务端（docs/sync-design.md 第 5–7 节）。服务端只做格式、配额与并发控制，不承担任何安全判定：
// 记录、花名册、信封与加入消息对服务端都是不透明的字节。
import { all, audit, first, run, statement } from './db.js'
import { HttpError, b64u, now, randomToken, sha256 } from './http.js'
import { HARD, formatBytes } from './limits.js'

// 硬性限制（docs/sync-design.md 第 11 节）。保险库容量按账号计算（limits.ts），这里只有与账号无关的上限。
export const LIMITS = {
  recordBytes: 256 * 1024,
  batchWrites: 50,
  pageSize: 200,
  rosterEntries: 1000,
  rosterEntryBytes: 8 * 1024,
  envelopeBytes: 16 * 1024,
  joinMessageBytes: 256 * 1024,
  vaultsPerIpPerDay: 5,
} as const

const JOIN_TTL = 10 * 60 * 1000
const RECORD_ID = /^[A-Za-z0-9_-]{16,43}$/
const RECIPIENT = /^([A-Z2-7]{26}|recovery)$/

export interface VaultRow extends Record<string, unknown> {
  id: string
  user_id: string
  seq: number
  roster_len: number
  min_epoch: number
  record_count: number
  byte_count: number
  purged_seq: number
}

export const vaultOf = (db: D1Database, userId: string): Promise<VaultRow | null> =>
  first<VaultRow>(db, 'SELECT * FROM vaults WHERE user_id = ?', userId)

/** 取当前用户的保险库，并确认路径中的 vault_id 属于该用户（跨账号隔离）。 */
export const ownedVault = async (db: D1Database, userId: string, vaultId: string): Promise<VaultRow> => {
  const vault = await first<VaultRow>(db, 'SELECT * FROM vaults WHERE id = ? AND user_id = ?', vaultId, userId)
  if (!vault) throw new HttpError(404, 'vault_not_found', '保险库不存在')
  return vault
}

const day = () => Math.floor(now() / 86_400_000)

export const createVault = async (env: Env, userId: string, ip: string): Promise<VaultRow> => {
  const existing = await vaultOf(env.DB, userId)
  if (existing) return existing
  const ipHash = b64u.encode(await sha256(`qantrip-ip:${ip}`)).slice(0, 22)
  const counter = await first<{ count: number }>(env.DB, 'SELECT count FROM vault_creations WHERE ip_hash = ? AND day = ?', ipHash, day())
  if ((counter?.count ?? 0) >= LIMITS.vaultsPerIpPerDay) throw new HttpError(429, 'too_many_vaults', '今天新建的保险库过多，请明天再试')
  const t = now()
  const id = `v_${randomToken(16)}`
  await env.DB.batch([
    statement(env.DB, 'INSERT INTO vaults (id, user_id, created_at, updated_at) VALUES (?, ?, ?, ?) ON CONFLICT(user_id) DO NOTHING', id, userId, t, t),
    statement(env.DB, 'INSERT INTO vault_creations (ip_hash, day, count) VALUES (?, ?, 1) ON CONFLICT(ip_hash, day) DO UPDATE SET count = count + 1', ipHash, day()),
  ])
  await audit(env.DB, userId, 'vault_created')
  return (await vaultOf(env.DB, userId))!
}

export const deleteVault = async (env: Env, userId: string): Promise<void> => {
  await run(env.DB, 'DELETE FROM vaults WHERE user_id = ?', userId)
  await audit(env.DB, userId, 'vault_deleted')
}

// MARK: 记录

export interface WireRecord {
  record_id: string
  seq: number
  epoch: number
  ciphertext: string
  deleted: boolean
}

const toWire = (r: { record_id: string; seq: number; epoch: number; ciphertext: ArrayBuffer | Uint8Array; deleted: number }): WireRecord => ({
  record_id: r.record_id,
  seq: r.seq,
  epoch: r.epoch,
  ciphertext: b64u.encode(r.ciphertext instanceof Uint8Array ? r.ciphertext : new Uint8Array(r.ciphertext)),
  deleted: r.deleted === 1,
})

export const changes = async (env: Env, vault: VaultRow, since: number, limit: number) => {
  const size = Math.min(Math.max(limit || LIMITS.pageSize, 1), LIMITS.pageSize)
  const rows = await all<{ record_id: string; seq: number; epoch: number; ciphertext: ArrayBuffer; deleted: number }>(
    env.DB,
    'SELECT record_id, seq, epoch, ciphertext, deleted FROM records WHERE vault_id = ? AND seq > ? ORDER BY seq LIMIT ?',
    vault.id, since, size + 1,
  )
  const page = rows.slice(0, size).map(toWire)
  const last = page.at(-1)
  return { records: page, next: rows.length > size && last ? last.seq : null, head: vault.seq, purged_seq: vault.purged_seq }
}

export interface WriteInput {
  record_id: string
  base_seq: number | null
  epoch: number
  ciphertext: string
  deleted: boolean
  idempotency_key: string
}

export type WriteResult =
  | { record_id: string; status: 'ok'; seq: number }
  | { record_id: string; status: 'conflict'; current: WireRecord }
  | { record_id: string; status: 'gone' | 'stale_epoch' | 'quota_exceeded' | 'invalid'; message: string }

const parseWrite = (w: unknown): WriteInput => {
  const o = w as Partial<WriteInput>
  if (typeof o?.record_id !== 'string' || !RECORD_ID.test(o.record_id)) throw new HttpError(400, 'bad_record_id', '记录 ID 格式不正确')
  if (o.base_seq !== null && !(Number.isInteger(o.base_seq) && (o.base_seq as number) >= 0)) throw new HttpError(400, 'bad_base_seq', 'base_seq 不正确')
  if (!(Number.isInteger(o.epoch) && (o.epoch as number) >= 1)) throw new HttpError(400, 'bad_epoch', 'epoch 不正确')
  if (typeof o.ciphertext !== 'string') throw new HttpError(400, 'bad_ciphertext', '缺少密文')
  if (typeof o.idempotency_key !== 'string' || o.idempotency_key.length < 8 || o.idempotency_key.length > 64) throw new HttpError(400, 'bad_idempotency_key', '缺少幂等键')
  return { record_id: o.record_id, base_seq: o.base_seq ?? null, epoch: o.epoch as number, ciphertext: o.ciphertext, deleted: Boolean(o.deleted), idempotency_key: o.idempotency_key }
}

/** 本账号保险库的有效容量：账号限制与绝对上限中较小者 */
const vaultQuota = (quotaBytes: number) => Math.min(quotaBytes, HARD.vaultBytes)

export const pushRecords = async (env: Env, vaultId: string, deviceId: string, quotaBytes: number, body: unknown): Promise<WriteResult[]> => {
  const writes = (body as { writes?: unknown[] })?.writes
  if (!Array.isArray(writes) || writes.length === 0) throw new HttpError(400, 'bad_request', '缺少写入内容')
  if (writes.length > LIMITS.batchWrites) throw new HttpError(400, 'batch_too_large', `每批最多 ${LIMITS.batchWrites} 条`)
  const results: WriteResult[] = []
  const quota = vaultQuota(quotaBytes)
  for (const raw of writes) results.push(await writeOne(env, vaultId, deviceId, quota, parseWrite(raw)))
  return results
}

const writeOne = async (env: Env, vaultId: string, deviceId: string, quota: number, w: WriteInput): Promise<WriteResult> => {
  const db = env.DB
  const cached = await first<{ result: string }>(db, 'SELECT result FROM idempotency_keys WHERE vault_id = ? AND key = ?', vaultId, w.idempotency_key)
  if (cached) return JSON.parse(cached.result) as WriteResult

  const bytes = b64u.decode(w.ciphertext)
  if (bytes.length === 0 || bytes.length > LIMITS.recordBytes) return remember(env, vaultId, w, { record_id: w.record_id, status: 'invalid', message: '记录为空或超过 256 KiB' })

  const vault = (await first<VaultRow>(db, 'SELECT * FROM vaults WHERE id = ?', vaultId))!
  if (w.epoch < vault.min_epoch) return remember(env, vaultId, w, { record_id: w.record_id, status: 'stale_epoch', message: '密钥版本已过期，请先完成同步' })

  const current = await first<{ record_id: string; seq: number; epoch: number; ciphertext: ArrayBuffer; deleted: number; size: number }>(
    db, 'SELECT record_id, seq, epoch, ciphertext, deleted, size FROM records WHERE vault_id = ? AND record_id = ?', vaultId, w.record_id,
  )
  const t = now()
  const del = w.deleted ? 1 : 0

  if (w.base_seq === null) {
    if (current) return { record_id: w.record_id, status: 'conflict', current: toWire(current) }
    // 新记录：配额检查与 seq 分配在同一个事务中完成
    const [ins] = await db.batch([
      statement(
        db,
        `INSERT INTO records (vault_id, record_id, seq, epoch, ciphertext, size, deleted, writer_device_id, updated_at)
         SELECT id, ?, seq + 1, ?, ?, ?, ?, ?, ? FROM vaults
         WHERE id = ? AND record_count < ? AND byte_count + ? <= ?
         ON CONFLICT(vault_id, record_id) DO NOTHING`,
        w.record_id, w.epoch, bytes, bytes.length, del, deviceId, t, vaultId, HARD.vaultRecords, bytes.length, quota,
      ),
      statement(db, 'UPDATE vaults SET seq = seq + 1, record_count = record_count + 1, byte_count = byte_count + ?, updated_at = ? WHERE id = ? AND changes() = 1', bytes.length, t, vaultId),
    ])
    if (ins!.meta.changes === 1) {
      const seq = await first<{ seq: number }>(db, 'SELECT seq FROM records WHERE vault_id = ? AND record_id = ?', vaultId, w.record_id)
      return remember(env, vaultId, w, { record_id: w.record_id, status: 'ok', seq: seq!.seq })
    }
    const again = await first<{ record_id: string; seq: number; epoch: number; ciphertext: ArrayBuffer; deleted: number }>(
      db, 'SELECT record_id, seq, epoch, ciphertext, deleted FROM records WHERE vault_id = ? AND record_id = ?', vaultId, w.record_id,
    )
    if (again) return { record_id: w.record_id, status: 'conflict', current: toWire(again) }
    return remember(env, vaultId, w, { record_id: w.record_id, status: 'quota_exceeded', message: `保险库已满（最多 ${HARD.vaultRecords.toLocaleString('en-US')} 条、${formatBytes(quota)}）` })
  }

  if (!current) {
    if (w.base_seq <= vault.purged_seq) return { record_id: w.record_id, status: 'gone', message: '该记录已被清理，请先全量对账' }
    return { record_id: w.record_id, status: 'gone', message: '记录不存在，请先全量对账' }
  }
  if (current.seq !== w.base_seq) return { record_id: w.record_id, status: 'conflict', current: toWire(current) }
  const delta = bytes.length - current.size
  // 只有增长的写入受容量限制：缩小或删除总是允许，容量下调后仍能腾出空间
  if (delta > 0 && vault.byte_count + delta > quota) {
    return remember(env, vaultId, w, { record_id: w.record_id, status: 'quota_exceeded', message: `保险库已满（最多 ${formatBytes(quota)}）` })
  }

  // CAS：只有 seq 仍等于 base_seq 时才更新；更新成功才推进保险库 seq
  const [upd] = await db.batch([
    statement(
      db,
      `UPDATE records SET seq = (SELECT seq + 1 FROM vaults WHERE id = ?), epoch = ?, ciphertext = ?, size = ?, deleted = ?, writer_device_id = ?, updated_at = ?
       WHERE vault_id = ? AND record_id = ? AND seq = ?`,
      vaultId, w.epoch, bytes, bytes.length, del, deviceId, t, vaultId, w.record_id, w.base_seq,
    ),
    statement(db, 'UPDATE vaults SET seq = seq + 1, byte_count = byte_count + ?, updated_at = ? WHERE id = ? AND changes() = 1', delta, t, vaultId),
  ])
  if (upd!.meta.changes === 1) {
    const seq = await first<{ seq: number }>(db, 'SELECT seq FROM records WHERE vault_id = ? AND record_id = ?', vaultId, w.record_id)
    return remember(env, vaultId, w, { record_id: w.record_id, status: 'ok', seq: seq!.seq })
  }
  const latest = await first<{ record_id: string; seq: number; epoch: number; ciphertext: ArrayBuffer; deleted: number }>(
    db, 'SELECT record_id, seq, epoch, ciphertext, deleted FROM records WHERE vault_id = ? AND record_id = ?', vaultId, w.record_id,
  )
  return latest ? { record_id: w.record_id, status: 'conflict', current: toWire(latest) } : { record_id: w.record_id, status: 'gone', message: '记录不存在，请先全量对账' }
}

/** 记住确定性的结果（成功、配额、过期），重试时直接返回；冲突类结果不缓存，以便客户端看到最新版本。 */
const remember = async (env: Env, vaultId: string, w: WriteInput, result: WriteResult): Promise<WriteResult> => {
  await run(env.DB, 'INSERT INTO idempotency_keys (vault_id, key, result, created_at) VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING', vaultId, w.idempotency_key, JSON.stringify(result), now())
  return result
}

/** 客户端在 rotate_done 之后上报 epoch 下限；只增不减。 */
export const raiseMinEpoch = async (env: Env, vaultId: string, epoch: number): Promise<number> => {
  if (!(Number.isInteger(epoch) && epoch >= 1)) throw new HttpError(400, 'bad_epoch', 'epoch 不正确')
  await run(env.DB, 'UPDATE vaults SET min_epoch = MAX(min_epoch, ?), updated_at = ? WHERE id = ?', epoch, now(), vaultId)
  return (await first<{ min_epoch: number }>(env.DB, 'SELECT min_epoch FROM vaults WHERE id = ?', vaultId))!.min_epoch
}

// MARK: 花名册

export const rosterSince = async (env: Env, vaultId: string, since: number) => {
  const rows = await all<{ idx: number; entry: ArrayBuffer }>(
    env.DB, 'SELECT idx, entry FROM roster_entries WHERE vault_id = ? AND idx >= ? ORDER BY idx LIMIT 1000', vaultId, Math.max(0, since),
  )
  return rows.map((r) => ({ index: r.idx, entry: b64u.encode(new Uint8Array(r.entry)) }))
}

export const appendRoster = async (env: Env, vaultId: string, body: unknown): Promise<number> => {
  const o = body as { expected_index?: unknown; entry?: unknown }
  if (!Number.isInteger(o?.expected_index) || typeof o.entry !== 'string') throw new HttpError(400, 'bad_request', '缺少 expected_index 或 entry')
  const expected = o.expected_index as number
  const bytes = b64u.decode(o.entry)
  if (bytes.length === 0 || bytes.length > LIMITS.rosterEntryBytes) throw new HttpError(400, 'entry_too_large', '花名册条目为空或过大')
  if (expected >= LIMITS.rosterEntries) throw new HttpError(403, 'roster_full', '花名册条目数已达上限')
  const t = now()
  // CAS：只有当前长度等于 expected_index 时才追加
  const [ins] = await env.DB.batch([
    statement(env.DB, 'INSERT INTO roster_entries (vault_id, idx, entry, created_at) SELECT id, roster_len, ?, ? FROM vaults WHERE id = ? AND roster_len = ?', bytes, t, vaultId, expected),
    statement(env.DB, 'UPDATE vaults SET roster_len = roster_len + 1, updated_at = ? WHERE id = ? AND changes() = 1', t, vaultId),
  ])
  if (ins!.meta.changes !== 1) throw new HttpError(409, 'roster_conflict', '花名册已被其他设备更新，请重新拉取')
  return expected
}

// MARK: 信封

export const putEnvelope = async (env: Env, vaultId: string, recipient: string, epoch: number, body: unknown): Promise<void> => {
  if (!RECIPIENT.test(recipient)) throw new HttpError(400, 'bad_recipient', '接收方格式不正确')
  if (!(Number.isInteger(epoch) && epoch >= 1)) throw new HttpError(400, 'bad_epoch', 'epoch 不正确')
  const o = body as { roster_index?: unknown; data?: unknown }
  if (!Number.isInteger(o?.roster_index) || typeof o.data !== 'string') throw new HttpError(400, 'bad_request', '缺少 roster_index 或 data')
  const bytes = b64u.decode(o.data)
  if (bytes.length === 0 || bytes.length > LIMITS.envelopeBytes) throw new HttpError(400, 'envelope_too_large', '信封为空或过大')
  const count = await first<{ n: number }>(env.DB, 'SELECT COUNT(*) AS n FROM envelopes WHERE vault_id = ? AND recipient = ?', vaultId, recipient)
  if ((count?.n ?? 0) >= 64) throw new HttpError(403, 'too_many_envelopes', '信封数量已达上限')
  await run(
    env.DB,
    `INSERT INTO envelopes (vault_id, recipient, epoch, roster_index, data, created_at) VALUES (?, ?, ?, ?, ?, ?)
     ON CONFLICT(vault_id, recipient, epoch) DO UPDATE SET roster_index = excluded.roster_index, data = excluded.data, created_at = excluded.created_at`,
    vaultId, recipient, epoch, o.roster_index, bytes, now(),
  )
}

export const envelopesFor = async (env: Env, vaultId: string, recipient: string) => {
  if (!RECIPIENT.test(recipient)) throw new HttpError(400, 'bad_recipient', '接收方格式不正确')
  const rows = await all<{ epoch: number; roster_index: number; data: ArrayBuffer }>(
    env.DB, 'SELECT epoch, roster_index, data FROM envelopes WHERE vault_id = ? AND recipient = ? ORDER BY epoch DESC LIMIT 64', vaultId, recipient,
  )
  return rows.map((r) => ({ epoch: r.epoch, roster_index: r.roster_index, data: b64u.encode(new Uint8Array(r.data)) }))
}

// MARK: 新设备加入（服务端只转发四段消息，docs/sync-design.md 6.2）

const JOIN_STEPS = ['reply', 'reveal', 'result'] as const
export type JoinStep = (typeof JOIN_STEPS)[number]
export const isJoinStep = (s: string): s is JoinStep => (JOIN_STEPS as readonly string[]).includes(s)

const message = (body: unknown): Uint8Array => {
  const data = (body as { data?: unknown })?.data
  if (typeof data !== 'string') throw new HttpError(400, 'bad_request', '缺少 data')
  const bytes = b64u.decode(data)
  if (bytes.length === 0 || bytes.length > LIMITS.joinMessageBytes) throw new HttpError(400, 'message_too_large', '消息为空或过大')
  return bytes
}

export const createJoin = async (env: Env, vaultId: string, deviceId: string, body: unknown): Promise<{ join_id: string; expires_at: number }> => {
  const bytes = message(body)
  const t = now()
  await run(env.DB, 'DELETE FROM join_requests WHERE vault_id = ? AND expires_at < ?', vaultId, t)
  const active = await first<{ id: string }>(env.DB, 'SELECT id FROM join_requests WHERE vault_id = ? LIMIT 1', vaultId)
  if (active) throw new HttpError(409, 'join_in_progress', '已有一个进行中的加入请求，请完成或等待其过期（10 分钟）')
  const id = `j_${randomToken(12)}`
  await run(env.DB, 'INSERT INTO join_requests (id, vault_id, commit_msg, created_at, expires_at, device_id) VALUES (?, ?, ?, ?, ?, ?)', id, vaultId, bytes, t, t + JOIN_TTL, deviceId)
  return { join_id: id, expires_at: t + JOIN_TTL }
}

type JoinRow = { id: string; commit_msg: ArrayBuffer; reply_msg: ArrayBuffer | null; reveal_msg: ArrayBuffer | null; result_msg: ArrayBuffer | null; expires_at: number }
const enc = (b: ArrayBuffer | null) => (b ? b64u.encode(new Uint8Array(b)) : null)
const joinView = (r: JoinRow) => ({ join_id: r.id, commit: enc(r.commit_msg), reply: enc(r.reply_msg), reveal: enc(r.reveal_msg), result: enc(r.result_msg), expires_at: r.expires_at })

/** 其他设备发起、尚未过期的加入请求数（每个保险库同时最多一个，按 vault_id 索引计数） */
export const pendingJoins = async (env: Env, vaultId: string, deviceId: string): Promise<number> =>
  (await first<{ n: number }>(
    env.DB, 'SELECT COUNT(*) AS n FROM join_requests WHERE vault_id = ? AND expires_at > ? AND (device_id IS NULL OR device_id != ?)', vaultId, now(), deviceId,
  ))?.n ?? 0

export const activeJoins = async (env: Env, vaultId: string) =>
  (await all<JoinRow>(env.DB, 'SELECT * FROM join_requests WHERE vault_id = ? AND expires_at > ? LIMIT 1', vaultId, now())).map(joinView)

export const getJoin = async (env: Env, vaultId: string, joinId: string) => {
  const row = await first<JoinRow>(env.DB, 'SELECT * FROM join_requests WHERE id = ? AND vault_id = ? AND expires_at > ?', joinId, vaultId, now())
  if (!row) throw new HttpError(404, 'join_not_found', '加入请求不存在或已过期')
  return joinView(row)
}

/** 每一段消息只能写一次，且必须按顺序：reply → reveal → result。 */
export const postJoinStep = async (env: Env, vaultId: string, joinId: string, step: JoinStep, body: unknown): Promise<void> => {
  const bytes = message(body)
  const prev = { reply: 'commit_msg', reveal: 'reply_msg', result: 'reveal_msg' }[step]
  const res = await run(
    env.DB,
    `UPDATE join_requests SET ${step}_msg = ? WHERE id = ? AND vault_id = ? AND expires_at > ? AND ${step}_msg IS NULL AND ${prev} IS NOT NULL`,
    bytes, joinId, vaultId, now(),
  )
  if (res.meta.changes !== 1) throw new HttpError(409, 'join_step_conflict', '加入流程的状态不符，请重新开始')
}

export const cancelJoin = (env: Env, vaultId: string, joinId: string): Promise<D1Result> =>
  run(env.DB, 'DELETE FROM join_requests WHERE id = ? AND vault_id = ?', joinId, vaultId)
