export type Row = Record<string, unknown>

export const first = async <T extends Row>(db: D1Database, sql: string, ...params: unknown[]): Promise<T | null> =>
  (await db.prepare(sql).bind(...params).first<T>()) ?? null

export const all = async <T extends Row>(db: D1Database, sql: string, ...params: unknown[]): Promise<T[]> =>
  (await db.prepare(sql).bind(...params).all<T>()).results

export const run = (db: D1Database, sql: string, ...params: unknown[]): Promise<D1Result> =>
  db.prepare(sql).bind(...params).run()

export const statement = (db: D1Database, sql: string, ...params: unknown[]): D1PreparedStatement =>
  db.prepare(sql).bind(...params)

export const audit = (db: D1Database, userId: string | null, action: string, detail?: string): Promise<D1Result> =>
  run(db, 'INSERT INTO audit_log (user_id, action, detail, created_at) VALUES (?, ?, ?, ?)', userId, action, detail ?? null, Date.now())
