import { env } from 'cloudflare:workers'
import { Form, redirect } from 'react-router'
import { approveDesktop, denyDesktop, loginAvailable, parseDesktopAuthorize, sessionUser } from '../../server/auth.js'
import { HttpError } from '../../server/http.js'
import { type Locale, useLocale } from '../lib/i18n'
import type { Route } from './+types/oauth-authorize'

const T = {
  en: {
    cannot: "Can't authorize",
    invalid: 'Invalid authorization request',
    soon: 'Sign-in is coming soon',
    h1: 'Sign in to Qantrip on this computer',
    as: 'Sign in this device as',
    note: 'This only lets the device upload and download encrypted data. To decrypt, you still need to enter a code on another signed-in device, or use your recovery phrase.',
    allow: 'Allow',
    deny: 'Deny',
  },
  zh: {
    cannot: '无法授权',
    invalid: '授权请求无效',
    soon: '登录即将开放',
    h1: '登录 Qantrip 桌面端',
    as: '以下列身份登录这台设备：',
    note: '登录只让这台设备可以上传和下载加密数据。解密需要在另一台已登录的设备上输入校验码，或使用恢复短语。',
    allow: '允许',
    deny: '拒绝',
  },
} satisfies Record<Locale, unknown>

export const meta: Route.MetaFunction = () => [{ title: 'Authorize · Nekro Qantrip' }, { name: 'robots', content: 'noindex' }]

const parse = (request: Request) => {
  try {
    return { req: parseDesktopAuthorize(new URL(request.url).searchParams), error: null }
  } catch (e) {
    return { req: null, error: e instanceof HttpError ? e.message : 'invalid' }
  }
}

export const loader = async ({ request }: Route.LoaderArgs) => {
  const { req, error } = parse(request)
  if (!req) return { error, device: null, login: null }
  const user = await sessionUser(env, request)
  if (!user) {
    if (!loginAvailable(env)) return { error: 'soon', device: null, login: null }
    const url = new URL(request.url)
    throw redirect(`/auth/github?return=${encodeURIComponent(url.pathname + url.search)}`)
  }
  return { error: null, device: req.deviceId, login: user.login }
}

export const action = async ({ request }: Route.ActionArgs) => {
  const { req } = parse(request)
  const user = await sessionUser(env, request)
  if (!req || !user) throw redirect('/account')
  const form = await request.formData()
  throw redirect(form.get('intent') === 'approve' ? await approveDesktop(env, user, req) : denyDesktop(req))
}

export default function Authorize({ loaderData }: Route.ComponentProps) {
  const { error, device, login } = loaderData
  const t = T[useLocale()]
  return (
    <section className="wrap page" style={{ maxWidth: 560 }}>
      {error ? (
        <>
          <h1>{t.cannot}</h1>
          <p className="muted">{error === 'soon' ? t.soon : error === 'invalid' ? t.invalid : error}</p>
        </>
      ) : (
        <>
          <h1>{t.h1}</h1>
          <p className="muted">
            {t.as} <strong>{login}</strong> (<span className="mono">{device?.slice(0, 4)}…{device?.slice(-4)}</span>)
          </p>
          <div className="callout">
            <span>{t.note}</span>
          </div>
          <Form method="post" className="row" style={{ justifyContent: 'flex-start' }}>
            <button className="btn primary" type="submit" name="intent" value="approve">
              {t.allow}
            </button>
            <button className="btn" type="submit" name="intent" value="deny">
              {t.deny}
            </button>
          </Form>
        </>
      )}
    </section>
  )
}
