//! 模板的结构化表示，供图形化编辑器使用（docs/decisions.md D19）。
//!
//! 源码 → `TemplateDoc`（`parse_doc`）→ 编辑 → 源码（`serialize_doc`）。
//! frontmatter 由表单重新生成，手写的 YAML 注释不会保留。

use crate::i18n::Locale;
use crate::template::{self, Choice, ParamKind, Template, Value};

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ParamDoc {
    pub name: String,
    pub kind: ParamKind,
    pub description: String,
    pub choices: Vec<Choice>,
    /// 默认值的文本形式：开关为 true / false，多选用逗号分隔，未设置为 None
    pub default_value: Option<String>,
    pub positional: bool,
    pub short: Option<String>,
    pub required: bool,
    pub requires: Vec<String>,
    pub conflicts: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TemplateDoc {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub category: String,
    pub params: Vec<ParamDoc>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Error)]
pub enum DocError {
    Invalid { message: String },
}

impl std::fmt::Display for DocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocError::Invalid { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for DocError {}

fn value_text(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.clone(),
        Value::List(l) => l.join(","),
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        Value::Num(n) => n.to_string(),
    }
}

pub fn from_template(t: &Template) -> TemplateDoc {
    TemplateDoc {
        name: t.name.clone(),
        aliases: t.aliases.clone(),
        description: t.description.clone(),
        category: t.category.clone(),
        params: t
            .params
            .iter()
            .map(|p| ParamDoc {
                name: p.name.clone(),
                kind: p.kind,
                description: p.description.clone(),
                choices: p.choices.clone(),
                default_value: p.default.as_ref().map(value_text),
                positional: p.positional,
                short: p.short.map(|c| c.to_string()),
                required: p.required,
                requires: p.requires.clone(),
                conflicts: p.conflicts.clone(),
                min: p.min,
                max: p.max,
            })
            .collect(),
        body: t.body.clone(),
    }
}

pub fn parse_doc(source: &str) -> Result<TemplateDoc, DocError> {
    template::parse(source, false).map(|t| from_template(&t)).map_err(|message| DocError::Invalid { message })
}

// MARK: - 序列化

/// YAML 标量：需要时用 JSON 字符串（合法的 YAML 双引号字符串）。
fn yq(s: &str) -> String {
    let plain = !s.is_empty()
        && !s.starts_with([' ', '-', '?', '!', '&', '*', '#', '|', '>', '%', '@', '`', '"', '\'', '[', '{'])
        && !s.ends_with(' ')
        && !s.contains([':', '#', ',', '[', ']', '{', '}', '\n', '\t'])
        && !matches!(s.to_lowercase().as_str(), "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~")
        && s.parse::<f64>().is_err();
    if plain { s.to_string() } else { serde_json::to_string(s).unwrap() }
}

fn ylist(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|s| yq(s)).collect::<Vec<_>>().join(", "))
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { n.to_string() }
}

fn kind_name(k: ParamKind) -> &'static str {
    match k {
        ParamKind::Bool => "bool",
        ParamKind::Enum => "enum",
        ParamKind::Multi => "multi",
        ParamKind::String => "string",
        ParamKind::Number => "number",
        ParamKind::Text => "text",
    }
}

pub fn serialize_doc(doc: &TemplateDoc) -> String {
    let mut y = String::from("---\n");
    y += &format!("name: {}\n", yq(&doc.name));
    if !doc.aliases.is_empty() {
        y += &format!("aliases: {}\n", ylist(&doc.aliases));
    }
    if !doc.description.is_empty() {
        y += &format!("description: {}\n", yq(&doc.description));
    }
    if !doc.category.is_empty() {
        y += &format!("category: {}\n", yq(&doc.category));
    }
    if !doc.params.is_empty() {
        y += "params:\n";
        for p in &doc.params {
            y += &format!("  {}:\n", yq(&p.name));
            y += &format!("    type: {}\n", kind_name(p.kind));
            if p.positional {
                y += "    positional: true\n";
            }
            if let Some(s) = p.short.as_deref().filter(|s| !s.is_empty()) {
                y += &format!("    short: {}\n", yq(s));
            }
            if p.required {
                y += "    required: true\n";
            }
            if !p.description.is_empty() {
                y += &format!("    description: {}\n", yq(&p.description));
            }
            if let Some(d) = p.default_value.as_deref().filter(|d| !d.is_empty()) {
                let v = match p.kind {
                    ParamKind::Bool | ParamKind::Number => d.to_string(),
                    ParamKind::Multi => ylist(&d.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>()),
                    _ => yq(d),
                };
                y += &format!("    default: {v}\n");
            }
            if let Some(m) = p.min {
                y += &format!("    min: {}\n", fmt_num(m));
            }
            if let Some(m) = p.max {
                y += &format!("    max: {}\n", fmt_num(m));
            }
            if !p.requires.is_empty() {
                y += &format!("    requires: {}\n", ylist(&p.requires));
            }
            if !p.conflicts.is_empty() {
                y += &format!("    conflicts: {}\n", ylist(&p.conflicts));
            }
            if !p.choices.is_empty() {
                y += "    choices:\n";
                for c in &p.choices {
                    let mut fields = vec![format!("value: {}", yq(&c.value))];
                    if !c.label.is_empty() && c.label != c.value {
                        fields.push(format!("label: {}", yq(&c.label)));
                    }
                    if !c.aliases.is_empty() {
                        fields.push(format!("aliases: {}", ylist(&c.aliases)));
                    }
                    y += &format!("      - {{ {} }}\n", fields.join(", "));
                }
            }
        }
    }
    y += "---\n";
    y += &doc.body;
    if !doc.body.ends_with('\n') {
        y.push('\n');
    }
    y
}

/// 新模板的初始文档。
pub fn new_doc(name: &str, loc: Locale) -> TemplateDoc {
    TemplateDoc {
        name: name.to_string(),
        aliases: vec![],
        description: String::new(),
        category: String::new(),
        params: vec![],
        body: match loc {
            Locale::Zh => "在这里编写 Prompt。可以使用参数，例如 {% if strict %}严格模式{% endif %}。\n{% include \"_extra\" %}\n",
            Locale::En => "Write your prompt here. You can use parameters, e.g. {% if strict %}strict mode{% endif %}.\n{% include \"_extra\" %}\n",
        }
        .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Library;
    use std::collections::BTreeMap;

    #[test]
    fn builtins_round_trip_through_doc() {
        let lib = Library::load(None);
        for t in &lib.templates {
            let doc = from_template(t);
            let src = serialize_doc(&doc);
            let back = parse_doc(&src).unwrap_or_else(|e| panic!("{}: {e}\n{src}", t.name));
            assert_eq!(doc, back, "{}", t.name);
        }
        // 序列化后的模板渲染结果不变
        let (lib2, _, problems) = Library::with_override(None, &serialize_doc(&from_template(lib.get("review").unwrap())));
        assert!(problems.is_empty(), "{problems:?}");
        let a = lib.render(lib.get("review").unwrap(), &BTreeMap::new(), Some("x")).unwrap();
        let b = lib2.render(lib2.get("review").unwrap(), &BTreeMap::new(), Some("x")).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn quotes_tricky_scalars() {
        let mut doc = new_doc("t", Locale::Zh);
        doc.description = "含: 冒号, 逗号 # 井号".into();
        doc.aliases = vec!["yes".into(), "1".into()];
        let back = parse_doc(&serialize_doc(&doc)).unwrap();
        assert_eq!(back.description, doc.description);
        assert_eq!(back.aliases, doc.aliases);
    }

    #[test]
    fn override_reports_own_problems() {
        let (_, name, problems) = Library::with_override(None, "---\nname: bad\n---\n{% if %}");
        assert_eq!(name.as_deref(), Some("bad"));
        assert!(problems.iter().any(|p| p.contains("语法错误")), "{problems:?}");
        let (_, name, problems) = Library::with_override(None, "no frontmatter");
        assert!(name.is_none() && !problems.is_empty());
    }
}
