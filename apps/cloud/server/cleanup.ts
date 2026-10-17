// 每日清理（docs/sync-design.md 第 11 节）：每类最多处理 1,000 行，成本有上限。
import { all, run, statement } from './db.js'
import { now } from './http.js'

const DAY = 86_400_000
const BATCH = 1000
export const TOMBSTONE_TTL = 180 * DAY

const purgeExpired = (db: D1Database, table: string, column: string, before: number) =>
  run(db, `DELETE FROM ${table} WHERE rowid IN (SELECT rowid FROM ${table} WHERE ${column} < ? LIMIT ${BATCH})`, before)

export const dailyCleanup = async (env: Env): Promise<void> => {
  const t = now()
  await purgeExpired(env.DB, 'web_sessions', 'expires_at', t)
  await purgeExpired(env.DB, 'login_states', 'expires_at', t)
  await purgeExpired(env.DB, 'oauth_codes', 'expires_at', t)
  await purgeExpired(env.DB, 'join_requests', 'expires_at', t)
  await purgeExpired(env.DB, 'idempotency_keys', 'created_at', t - DAY)
  await purgeExpired(env.DB, 'vault_creations', 'day', Math.floor(t / DAY) - 2)

  // 墓碑：清理后记下 purged_seq，之后基于更早版本的写入返回 gone，客户端转为全量对账
  const tombstones = await all<{ vault_id: string; record_id: string; seq: number; size: number }>(
    env.DB, `SELECT vault_id, record_id, seq, size FROM records WHERE deleted = 1 AND updated_at < ? LIMIT ${BATCH}`, t - TOMBSTONE_TTL,
  )
  if (tombstones.length === 0) return
  await env.DB.batch(
    tombstones.flatMap((r) => [
      statement(env.DB, 'DELETE FROM records WHERE vault_id = ? AND record_id = ? AND seq = ?', r.vault_id, r.record_id, r.seq),
      statement(
        env.DB,
        'UPDATE vaults SET purged_seq = MAX(purged_seq, ?), record_count = record_count - 1, byte_count = byte_count - ? WHERE id = ? AND changes() = 1',
        r.seq, r.size, r.vault_id,
      ),
    ]),
  )
}
