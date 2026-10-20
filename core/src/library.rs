//! 模板库：内置模板 + 用户目录中的模板，以及 MiniJinja 渲染环境。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, UndefinedBehavior};

use crate::i18n::Locale;
use crate::template::{self, ParamKind, Template, Value};
use crate::tr;

/// 渲染沙箱限制（PRD 4.3）。MiniJinja 本身不提供文件、网络或进程访问。
const FUEL: u64 = 200_000;
const RECURSION_LIMIT: usize = 32;
const MAX_OUTPUT_CHARS: usize = 100_000;

macro_rules! builtin {
    ($dir:literal: $($name:literal),* $(,)?) => {
        &[$(include_str!(concat!($dir, $name, ".md"))),*]
    };
}

/// 内置模板的展示顺序：同一分类的模板相邻，分类按工作流排列（理解 → 修改 → 检查 → 交付 → 翻译）。
/// 两种语言的模板名、参数与候选值一致，只有文字不同。
const BUILTIN_ZH: &[&str] = builtin!["../templates/":
    "_mode", "_scope", "_extra",
    "explain", "investigate", "plan", "arch",
    "fix", "debug", "refactor", "simplify", "perf",
    "review", "security", "test",
    "commit", "pr", "docs",
    "zh", "en",
];
const BUILTIN_EN: &[&str] = builtin!["../templates/en/":
    "_mode", "_scope", "_extra",
    "explain", "investigate", "plan", "arch",
    "fix", "debug", "refactor", "simplify", "perf",
    "review", "security", "test",
    "commit", "pr", "docs",
    "zh", "en",
];

fn builtins(loc: Locale) -> &'static [&'static str] {
    match loc {
        Locale::Zh => BUILTIN_ZH,
        Locale::En => BUILTIN_EN,
    }
}

/// 被用户删除的内置模板（docs/decisions.md D34）
#[derive(Debug, Clone, PartialEq)]
pub struct DeletedBuiltin {
    pub name: String,
    /// 删除标记文件；删除它即恢复
    pub marker: PathBuf,
}

pub struct Library {
    pub templates: Vec<Template>,
    pub problems: Vec<String>,
    /// 已删除的内置模板（可恢复）
    pub deleted: Vec<DeletedBuiltin>,
    /// 界面文字（诊断、提示、分区标题）的语言
    pub locale: Locale,
    /// 内置模板的语言
    pub template_locale: Locale,
    env: Environment<'static>,
}

impl Library {
    /// 中文界面、中文内置模板。
    pub fn load(user_dir: Option<&Path>) -> Library {
        Library::load_with(user_dir, Locale::Zh, Locale::Zh)
    }

    /// `ui`：界面文字语言；`templates`：内置模板语言（可以与界面不同）。
    pub fn load_with(user_dir: Option<&Path>, ui: Locale, templates: Locale) -> Library {
        let (list, problems, deleted) = collect(user_dir, ui, templates);
        build(list, problems, deleted, ui, templates)
    }

    /// 只含一个模板（以及内置的局部模板）的模板库：`/new` 的效果预览用，不读磁盘。
    pub fn preview(t: Template, ui: Locale, tpl: Locale) -> Library {
        let mut list: Vec<Template> = builtins(tpl).iter().filter_map(|s| template::parse_in(s, true, ui).ok()).filter(|b| b.is_partial()).collect();
        list.push(t);
        build(list, vec![], vec![], ui, tpl)
    }

    /// 编辑器预览：在当前模板库上用 `source` 覆盖（或新增）一个模板。
    /// 返回模板库和 `source` 自身的问题（格式、语法错误）。
    pub fn with_override(user_dir: Option<&Path>, source: &str) -> (Library, Option<String>, Vec<String>) {
        Library::with_override_in(user_dir, source, Locale::Zh, Locale::Zh)
    }

    pub fn with_override_in(user_dir: Option<&Path>, source: &str, ui: Locale, tpl: Locale) -> (Library, Option<String>, Vec<String>) {
        let (mut templates, problems, deleted) = collect(user_dir, ui, tpl);
        let mut own = Vec::new();
        let name = match template::parse_in(source, false, ui) {
            Ok(t) => {
                let name = t.name.clone();
                templates.retain(|o| o.name != name);
                templates.push(t);
                Some(name)
            }
            Err(e) => {
                own.push(e);
                None
            }
        };
        let lib = build(templates, problems, deleted, ui, tpl);
        if let Some(n) = &name {
            own.extend(lib.problems.iter().filter(|p| p.contains(&format!("`{n}`"))).cloned());
        }
        (lib, name, own)
    }

    /// 可搜索的模板（不含公共片段）。
    pub fn visible(&self) -> impl Iterator<Item = &Template> {
        self.templates.iter().filter(|t| !t.is_partial())
    }

    pub fn get(&self, name: &str) -> Option<&Template> {
        self.templates.iter().find(|t| t.name == name)
    }

    /// 用参数值渲染模板。`values` 中未出现的参数取默认值。
    pub fn render(&self, t: &Template, values: &BTreeMap<String, Value>, extra: Option<&str>) -> Result<String, String> {
        let mut ctx: BTreeMap<String, minijinja::Value> = BTreeMap::new();
        for p in &t.params {
            let v = values.get(&p.name).cloned().unwrap_or_else(|| p.effective_default());
            ctx.insert(p.var_name(), to_jinja(&v, p.kind));
        }
        ctx.insert("extra".into(), minijinja::Value::from(extra.unwrap_or("")));
        let loc = self.locale;
        let tmpl = self.env.get_template(&t.name).map_err(|e| e.to_string())?;
        let out = tmpl.render(minijinja::Value::from(ctx)).map_err(|e| tr!(loc, "渲染失败：{e}", "Render failed: {e}"))?;
        let out = tidy(&out);
        if out.chars().count() > MAX_OUTPUT_CHARS {
            return Err(tr!(loc, "渲染结果超过 {MAX_OUTPUT_CHARS} 字符上限", "The result exceeds the {MAX_OUTPUT_CHARS}-character limit"));
        }
        Ok(out)
    }
}

fn collect(user_dir: Option<&Path>, ui: Locale, tpl: Locale) -> (Vec<Template>, Vec<String>, Vec<DeletedBuiltin>) {
    let mut deleted: Vec<DeletedBuiltin> = Vec::new();
    let mut problems = Vec::new();
    let mut templates: Vec<Template> = Vec::new();

    for src in builtins(tpl) {
        match template::parse_in(src, true, ui) {
            Ok(t) => templates.push(t),
            Err(e) => problems.push(tr!(ui, "内置模板：{e}", "Built-in template: {e}")),
        }
    }

    if let Some(dir) = user_dir
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .collect();
        paths.sort();
        for path in paths {
            let file = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| template::parse_in(&s, false, ui)) {
                Ok(mut t) => {
                    t.path = Some(path.clone());
                    // 删除标记只隐藏同名内置模板，不影响同名的用户模板
                    if t.deleted {
                        if builtins(tpl).iter().any(|s| template::parse(s, true).is_ok_and(|b| b.name == t.name && !b.is_partial())) {
                            deleted.push(DeletedBuiltin { name: t.name.clone(), marker: path.clone() });
                        }
                        continue;
                    }
                    // 用户模板与内置模板同名时覆盖内置版本
                    templates.retain(|o| !(o.builtin && o.name == t.name));
                    if templates.iter().any(|o| o.name == t.name) {
                        let n = &t.name;
                        problems.push(tr!(ui, "{file}：模板名 `{n}` 重复，已忽略", "{file}: template name `{n}` is already taken; skipped"));
                    } else {
                        templates.push(t);
                    }
                }
                Err(e) => problems.push(tr!(ui, "{file}：{e}", "{file}: {e}")),
            }
        }
    }
    templates.retain(|t| !(t.builtin && deleted.iter().any(|d| d.name == t.name)));
    (templates, problems, deleted)
}

fn build(mut templates: Vec<Template>, mut problems: Vec<String>, deleted: Vec<DeletedBuiltin>, loc: Locale, tpl: Locale) -> Library {
    resolve_alias_conflicts(&mut templates, &mut problems, loc);

    let mut env = Environment::new();
    env.set_syntax(SyntaxConfig::builder().trim_blocks(true).lstrip_blocks(true).build().unwrap());
    env.set_undefined_behavior(UndefinedBehavior::SemiStrict);
    env.set_fuel(Some(FUEL));
    env.set_recursion_limit(RECURSION_LIMIT);
    let mut ok = Vec::new();
    for t in templates {
        match env.add_template_owned(t.name.clone(), t.body.clone()) {
            Ok(()) => ok.push(t),
            Err(e) => {
                let n = &t.name;
                problems.push(tr!(loc, "模板 `{n}` 语法错误：{e}", "Template `{n}` has a syntax error: {e}"))
            }
        }
    }
    Library { templates: ok, problems, deleted, locale: loc, template_locale: tpl, env }
}

/// 内置模板源码（不含被用户覆盖的情况），用于"恢复默认"。
pub fn builtin_source(name: &str) -> Option<&'static str> {
    builtin_source_in(name, Locale::Zh)
}

pub fn builtin_source_in(name: &str, loc: Locale) -> Option<&'static str> {
    builtins(loc).iter().copied().find(|s| template::parse(s, true).is_ok_and(|t| t.name == name))
}

fn to_jinja(v: &Value, kind: ParamKind) -> minijinja::Value {
    match v {
        Value::Bool(b) => (*b).into(),
        Value::Str(s) => s.as_str().into(),
        Value::List(l) => minijinja::Value::from(l.clone()),
        Value::Num(n) if kind == ParamKind::Number && n.fract() == 0.0 && n.abs() < 1e15 => (*n as i64).into(),
        Value::Num(n) => (*n).into(),
    }
}

/// 输出整理：去掉行尾空白，把连续空行压缩为一行，去掉首尾空白。保证确定性。
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank = 0;
    for line in s.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(line);
    }
    out
}

/// 别名冲突：同一别名（或与其他模板名相同）时，保留先出现者，后者的别名标记为不可用。
fn resolve_alias_conflicts(templates: &mut [Template], problems: &mut Vec<String>, loc: Locale) {
    let mut taken: Vec<String> = templates.iter().map(|t| t.name.to_lowercase()).collect();
    for t in templates.iter_mut() {
        let mut kept = Vec::new();
        for a in std::mem::take(&mut t.aliases) {
            let key = a.to_lowercase();
            if taken.contains(&key) {
                let n = &t.name;
                problems.push(tr!(loc, "模板 `{n}` 的别名 `{a}` 与其他模板冲突，已停用", "Alias `{a}` of template `{n}` clashes with another template; disabled"));
            } else {
                taken.push(key);
                kept.push(a);
            }
        }
        t.aliases = kept;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_load_and_render_with_defaults() {
        let lib = Library::load(None);
        assert!(lib.problems.is_empty(), "{:?}", lib.problems);
        assert_eq!(lib.visible().count(), 17);
        for t in lib.visible() {
            let out = lib.render(t, &BTreeMap::new(), None).unwrap_or_else(|e| panic!("{}: {e}", t.name));
            assert!(!out.is_empty() && !out.contains("{{") && !out.contains("{%"), "{}: {out}", t.name);
        }
    }

    #[test]
    fn english_builtins_mirror_chinese() {
        let zh = Library::load(None);
        let en = Library::load_with(None, Locale::En, Locale::En);
        assert!(en.problems.is_empty(), "{:?}", en.problems);
        let names = |l: &Library| l.templates.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&zh), names(&en));
        for (a, b) in zh.templates.iter().zip(&en.templates) {
            assert_eq!(a.aliases, b.aliases, "{}", a.name);
            let ps = |t: &Template| t.params.iter().map(|p| (p.name.clone(), p.kind, p.short, p.positional, p.choices.iter().map(|c| c.value.clone()).collect::<Vec<_>>())).collect::<Vec<_>>();
            assert_eq!(ps(a), ps(b), "{}", a.name);
        }
    }

    #[test]
    fn renders_all_choice_combinations() {
        // 每个 enum / multi 的每个候选值都能渲染，防止模板里的映射表漏项
        for lib in [Library::load(None), Library::load_with(None, Locale::En, Locale::En)] {
        for t in lib.visible() {
            for p in &t.params {
                for c in &p.choices {
                    let v = if p.kind == ParamKind::Multi { Value::List(vec![c.value.clone()]) } else { Value::Str(c.value.clone()) };
                    let vals = BTreeMap::from([(p.name.clone(), v)]);
                    lib.render(t, &vals, Some("附加")).unwrap_or_else(|e| panic!("{} {}={}: {e}", t.name, p.name, c.value));
                }
                if p.kind == ParamKind::Bool {
                    let vals = BTreeMap::from([(p.name.clone(), Value::Bool(true))]);
                    lib.render(t, &vals, None).unwrap();
                }
            }
        }
        }
    }

    #[test]
    fn sandbox_limits() {
        let dir = std::env::temp_dir().join(format!("qantrip-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("loop.md"), "---\nname: loop\n---\n{% for i in range(100000) %}{% for j in range(100000) %}x{% endfor %}{% endfor %}").unwrap();
        std::fs::write(dir.join("rec.md"), "---\nname: rec\n---\n{% include \"rec\" %}").unwrap();
        std::fs::write(dir.join("zh.md"), "---\nname: zh\n---\n用户覆盖").unwrap();
        let lib = Library::load(Some(&dir));
        assert!(lib.render(lib.get("loop").unwrap(), &BTreeMap::new(), None).is_err());
        assert!(lib.render(lib.get("rec").unwrap(), &BTreeMap::new(), None).is_err());
        assert_eq!(lib.render(lib.get("zh").unwrap(), &BTreeMap::new(), None).unwrap(), "用户覆盖");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tidy_collapses_blank_lines() {
        assert_eq!(tidy("\n a  \n\n\n\nb\n\n"), " a\n\nb");
    }
}
