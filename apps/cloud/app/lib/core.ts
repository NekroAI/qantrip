// 浏览器中的 Qantrip 核心（WebAssembly，scripts/build-wasm.sh 生成）。整页共享一个实例。
export interface Core {
  set_locale(code: string): void
  analyze(input: string, selected?: string): string
  render(input: string, selected?: string): string | undefined
  edit_param(input: string, selected: string | undefined, name: string, edit: string): string | undefined
  check_source(source: string, tryInput: string): string
}

let loading: Promise<Core> | null = null

export function loadCore(): Promise<Core> {
  loading ??= (async () => {
    // 绝对地址：不让打包工具把它当作源码模块处理（文件在 public/，构建时原样复制）
    const url = new URL('/wasm/qantrip_wasm.js', window.location.href).href
    const mod = (await import(/* @vite-ignore */ url)) as Core & { default: () => Promise<unknown> }
    await mod.default()
    return mod
  })()
  return loading
}
