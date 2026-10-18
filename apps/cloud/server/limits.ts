// 按账号的限制（docs/sync-design.md 第 11 节）：默认值来自 wrangler vars，运营者可在 users.limits 中为单个账号覆盖。
// 覆盖值逐项校验类型与范围，不合法的项忽略；无论如何覆盖，都不超过下面的绝对上限。

/** 绝对上限：不受默认值与覆盖值影响 */
export const HARD = {
  vaultBytes: 10 * 1024 * 1024,
  vaultRecords: 5000,
  /** 每个账号同时有效的设备授权总数（含超出设备数量限制的授权） */
  grants: 20,
} as const

export interface Limits {
  quota_bytes: number
  max_devices: number
  check_interval_secs: number
  panel_check_secs: number
  min_push_interval_secs: number
}

export type Policy = Pick<Limits, 'check_interval_secs' | 'panel_check_secs' | 'min_push_interval_secs'>

const RANGES: Record<keyof Limits, [number, number]> = {
  quota_bytes: [1024, HARD.vaultBytes],
  max_devices: [1, HARD.grants],
  check_interval_secs: [60, 7 * 86_400],
  panel_check_secs: [60, 7 * 86_400],
  min_push_interval_secs: [0, 86_400],
}

/** 环境变量缺失或不合法时的兜底值 */
const FALLBACK: Limits = {
  quota_bytes: 204_800,
  max_devices: 2,
  check_interval_secs: 3600,
  panel_check_secs: 900,
  min_push_interval_secs: 900,
}

const valid = (key: keyof Limits, v: unknown): v is number => {
  const [lo, hi] = RANGES[key]
  return typeof v === 'number' && Number.isInteger(v) && v >= lo && v <= hi
}

type LimitVars = Partial<Record<'DEFAULT_QUOTA_BYTES' | 'DEFAULT_MAX_DEVICES' | 'SYNC_CHECK_INTERVAL_SECS' | 'SYNC_PANEL_CHECK_SECS' | 'SYNC_MIN_PUSH_INTERVAL_SECS', unknown>>

export const defaultLimits = (env: Env): Limits => {
  const vars = env as unknown as LimitVars
  const pick = (key: keyof Limits, raw: unknown): number => {
    const n = typeof raw === 'string' && /^\d+$/.test(raw.trim()) ? Number(raw.trim()) : raw
    return valid(key, n) ? n : FALLBACK[key]
  }
  return {
    quota_bytes: pick('quota_bytes', vars.DEFAULT_QUOTA_BYTES),
    max_devices: pick('max_devices', vars.DEFAULT_MAX_DEVICES),
    check_interval_secs: pick('check_interval_secs', vars.SYNC_CHECK_INTERVAL_SECS),
    panel_check_secs: pick('panel_check_secs', vars.SYNC_PANEL_CHECK_SECS),
    min_push_interval_secs: pick('min_push_interval_secs', vars.SYNC_MIN_PUSH_INTERVAL_SECS),
  }
}

/** 默认值叠加账号覆盖值（users.limits 的 JSON 文本）。 */
export const effectiveLimits = (env: Env, overrides: string | null | undefined): Limits => {
  const out = defaultLimits(env)
  if (!overrides) return out
  let parsed: unknown
  try {
    parsed = JSON.parse(overrides)
  } catch {
    return out
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return out
  for (const key of Object.keys(RANGES) as (keyof Limits)[]) {
    const v = (parsed as Record<string, unknown>)[key]
    if (valid(key, v)) out[key] = v
  }
  return out
}

export const policyOf = (l: Limits): Policy => ({
  check_interval_secs: l.check_interval_secs,
  panel_check_secs: l.panel_check_secs,
  min_push_interval_secs: l.min_push_interval_secs,
})

/** 以字节数为单位的容量的展示文本（错误说明用） */
export const formatBytes = (n: number): string =>
  n >= 1024 * 1024 ? `${+(n / 1024 / 1024).toFixed(2)} MiB` : n >= 1024 ? `${+(n / 1024).toFixed(1)} KiB` : `${n} B`
