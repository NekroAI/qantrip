//! 模板库：内置模板 + 用户目录中的模板，以及 MiniJinja 渲染环境。

use std::collections::BTreeMap;
use std::path::Path;

use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, UndefinedBehavior};

use crate::template::{self, ParamKind, Template, Value};

/// 渲染沙箱限制（PRD 4.3）。MiniJinja 本身不提供文件、网络或进程访问。
const FUEL: u64 = 200_000;
const RECURSION_LIMIT: usize = 32;
const MAX_OUTPUT_CHARS: usize = 100_000;

macro_rules! builtin {
    ($($name:literal),* $(,)?) => {
        &[$(include_str!(concat!("../templates/", $name, ".md"))),*]
    };
}

const BUILTIN: &[&str] = builtin![
    "_mode", "_scope", "_extra", "zh", "en", "review", "fix", "debug", "test", "refactor", "perf", "security",
    "explain", "plan", "arch", "docs", "commit", "pr", "simplify", "investigate",
];

pub struct Library {
    pub templates: Vec<Template>,
    pub problems: Vec<String>,
    env: Environment<'static>,
}

impl Library {
    pub fn load(user_dir: Option<&Path>) -> Library {
        let mut problems = Vec::new();
        let mut templates: Vec<Template> = Vec::new();

        for src in BUILTIN {
            match template::parse(src, true) {
                Ok(t) => templates.push(t),
                Err(e) => problems.push(format!("内置模板：{e}")),
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
                match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| template::parse(&s, false)) {
                    Ok(t) => {
                        // 用户模板与内置模板同名时覆盖内置版本
                        templates.retain(|o| !(o.builtin && o.name == t.name));
                        if templates.iter().any(|o| o.name == t.name) {
                            problems.push(format!("{file}：模板名 `{}` 重复，已忽略", t.name));
                        } else {
                            templates.push(t);
                        }
                    }
                    Err(e) => problems.push(format!("{file}：{e}")),
                }
            }
        }

        resolve_alias_conflicts(&mut templates, &mut problems);

        let mut env = Environment::new();
        env.set_syntax(SyntaxConfig::builder().trim_blocks(true).lstrip_blocks(true).build().unwrap());
        env.set_undefined_behavior(UndefinedBehavior::SemiStrict);
        env.set_fuel(Some(FUEL));
        env.set_recursion_limit(RECURSION_LIMIT);
        let mut ok = Vec::new();
        for t in templates {
            match env.add_template_owned(t.name.clone(), t.body.clone()) {
                Ok(()) => ok.push(t),
                Err(e) => problems.push(format!("模板 `{}` 语法错误：{e}", t.name)),
            }
        }

        Library { templates: ok, problems, env }
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
        let tmpl = self.env.get_template(&t.name).map_err(|e| e.to_string())?;
        let out = tmpl.render(minijinja::Value::from(ctx)).map_err(|e| format!("渲染失败：{e}"))?;
        let out = tidy(&out);
        if out.chars().count() > MAX_OUTPUT_CHARS {
            return Err(format!("渲染结果超过 {MAX_OUTPUT_CHARS} 字符上限"));
        }
        Ok(out)
    }
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
fn resolve_alias_conflicts(templates: &mut [Template], problems: &mut Vec<String>) {
    let mut taken: Vec<String> = templates.iter().map(|t| t.name.to_lowercase()).collect();
    for t in templates.iter_mut() {
        let mut kept = Vec::new();
        for a in std::mem::take(&mut t.aliases) {
            let key = a.to_lowercase();
            if taken.contains(&key) {
                problems.push(format!("模板 `{}` 的别名 `{a}` 与其他模板冲突，已停用", t.name));
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
    fn renders_all_choice_combinations() {
        // 每个 enum / multi 的每个候选值都能渲染，防止模板里的映射表漏项
        let lib = Library::load(None);
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
