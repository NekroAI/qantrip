/** 接口错误：status + 机器可读的 code + 给用户看的中文说明。 */
export class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message)
  }
}

export const now = (): number => Date.now()

const B64U = /^[A-Za-z0-9_-]*$/

export const b64u = {
  encode(bytes: Uint8Array): string {
    let s = ''
    for (const b of bytes) s += String.fromCharCode(b)
    return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
  },
  decode(text: string): Uint8Array {
    if (!B64U.test(text)) throw new HttpError(400, 'bad_encoding', '数据不是合法的 base64url 编码')
    const s = atob(text.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((text.length + 3) % 4))
    const out = new Uint8Array(s.length)
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i)
    return out
  },
}

export const randomToken = (bytes = 32): string => b64u.encode(crypto.getRandomValues(new Uint8Array(bytes)))

export const sha256 = async (data: string | Uint8Array): Promise<Uint8Array> =>
  new Uint8Array(await crypto.subtle.digest('SHA-256', typeof data === 'string' ? new TextEncoder().encode(data) : new Uint8Array(data)))

/** 令牌哈希：服务端只保存哈希，不保存令牌本身。 */
export const tokenHash = async (token: string): Promise<string> => b64u.encode(await sha256(token))

/** 只允许站内相对路径，防止登录后被重定向到外站。 */
export const safeReturnPath = (value: string | null | undefined, fallback = '/account'): string =>
  value && value.startsWith('/') && !value.startsWith('//') && !value.startsWith('/\\') ? value : fallback
