import { env } from 'cloudflare:workers'
import { Form, redirect } from 'react-router'
import { approveDesktop, denyDesktop, loginAvailable, parseDesktopAuthorize, sessionUser } from '../../server/auth.js'
import { HttpError } from '../../server/http.js'
import type { Route } from './+types/oauth-authorize'

export const meta: Route.MetaFunction = () => [{ title: '授权桌面端 · Nekro Qantrip' }, { name: 'robots', content: 'noindex' }]

const parse = (request: Request) => {
  try {
    return { req: parseDesktopAuthorize(new URL(request.url).searchParams), error: null }
  } catch (e) {
    return { req: null, error: e instanceof HttpError ? e.message : '授权请求无效' }
  }
}

export const loader = async ({ request }: Route.LoaderArgs) => {
  const { req, error } = parse(request)
  if (!req) return { error, device: null, login: null }
  const user = await sessionUser(env, request)
  if (!user) {
    if (!loginAvailable(env)) return { error: '登录即将开放', device: null, login: null }
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
  return (
    <section className="wrap page" style={{ maxWidth: 560 }}>
      {error ? (
        <>
          <h1>无法授权</h1>
          <p className="muted">{error}</p>
        </>
      ) : (
        <>
          <h1>登录 Qantrip 桌面端</h1>
          <p className="muted">
            以 <strong>{login}</strong> 的身份登录这台设备（<span className="mono">{device?.slice(0, 4)}…{device?.slice(-4)}</span>）。
          </p>
          <div className="callout">
            <span>登录只让这台设备可以上传和下载加密数据。解密需要在另一台已登录的设备上输入校验码，或使用恢复短语。</span>
          </div>
          <Form method="post" className="row" style={{ justifyContent: 'flex-start' }}>
            <button className="btn primary" type="submit" name="intent" value="approve">
              允许
            </button>
            <button className="btn" type="submit" name="intent" value="deny">
              拒绝
            </button>
          </Form>
        </>
      )}
    </section>
  )
}
