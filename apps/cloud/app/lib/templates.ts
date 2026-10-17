// 内置模板一览：由 scripts/build-wasm.sh 通过 WebAssembly 核心生成（app/lib/templates.json），
// 与客户端内置模板完全一致，按模板库顺序排列（同分类相邻）。
import type { Locale } from './i18n'
import data from './templates.json'

export interface TemplateParam {
  name: string
  kind: 'bool' | 'enum' | 'multi' | 'string' | 'number' | 'text'
  short: string | null
  positional: boolean
  description: string
  choices: [string, string][]
}

export interface BuiltinTemplate {
  name: string
  aliases: string[]
  description: string
  category: string
  params: TemplateParam[]
  source: string
}

export const templatesOf = (locale: Locale) => (data as unknown as Record<Locale, BuiltinTemplate[]>)[locale]

/** 按分类分组，保持顺序 */
export function groupTemplates(list: BuiltinTemplate[]): { category: string; items: BuiltinTemplate[] }[] {
  const groups: { category: string; items: BuiltinTemplate[] }[] = []
  for (const t of list) {
    const g = groups.find((x) => x.category === t.category)
    if (g) g.items.push(t)
    else groups.push({ category: t.category, items: [t] })
  }
  return groups
}
