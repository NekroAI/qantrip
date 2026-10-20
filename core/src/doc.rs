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
            Locale::Zh => "在这里写提示词。需要参数时，先在上面的「参数」里添加，再点正文下方的写法插入到这里。\n{% include \"_extra\" %}\n",
            Locale::En => "Write your prompt here. For parameters, add them under Parameters above, then click a form below the text to insert it here.\n{% include \"_extra\" %}\n",
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

// MARK: - 编辑器引导

/// 编辑器正文下方"插入"的一种写法
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TemplateSnippet {
    /// 这种写法做什么，如"加上 --strict 时才出现的文字"
    pub label: String,
    /// 插入正文的模板代码
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TemplateGuide {
    /// 试用框的示例输入（用到模板实际有的参数）
    pub try_example: String,
    pub snippets: Vec<TemplateSnippet>,
}

/// 按模板当前的参数生成编辑器引导：可插入的写法与试用示例。
pub fn guide(d: &TemplateDoc, loc: Locale) -> TemplateGuide {
    let mut snippets = Vec::new();
    let mut push = |label: String, code: String| snippets.push(TemplateSnippet { label, code });
    for p in &d.params {
        let n = &p.name;
        let var = n.replace('-', "_");
        let first = p.choices.first().map(|c| c.value.clone()).unwrap_or_else(|| "a".into());
        match p.kind {
            ParamKind::Bool => {
                push(crate::tr!(loc, "加上 --{n} 时才出现的文字", "Text that appears only with --{n}"), format!("{{% if {var} %}}…{{% endif %}}"));
                push(crate::tr!(loc, "加上 --{n} 与不加时各用一段文字", "One text with --{n}, another without"), format!("{{% if {var} %}}…{{% else %}}…{{% endif %}}"));
            }
            ParamKind::Enum => {
                let mut code = String::new();
                for (i, c) in p.choices.iter().take(6).enumerate() {
                    let tag = if i == 0 { "if" } else { "elif" };
                    code.push_str(&format!("{{% {tag} {var} == \"{}\" %}}…", c.value));
                }
                if code.is_empty() {
                    code = format!("{{% if {var} == \"a\" %}}…");
                }
                code.push_str("{% endif %}");
                push(crate::tr!(loc, "按 --{n} 的取值输出不同的文字", "Different text for each value of --{n}"), code);
                push(crate::tr!(loc, "填入 --{n} 的取值", "Insert the value of --{n}"), format!("{{{{ {var} }}}}"));
            }
            ParamKind::Multi => {
                push(crate::tr!(loc, "选了 {first} 时才出现的文字", "Text that appears when {first} is chosen"), format!("{{% if \"{first}\" in {var} %}}…{{% endif %}}"));
                push(crate::tr!(loc, "列出 --{n} 选中的取值", "List the chosen values of --{n}"), format!("{{{{ {var} | join(\"、\") }}}}"));
            }
            ParamKind::String | ParamKind::Number | ParamKind::Text => {
                push(crate::tr!(loc, "填入 --{n} 的内容", "Insert what --{n} says"), format!("{{{{ {var} }}}}"));
                push(crate::tr!(loc, "写了 --{n} 时才出现的一段", "A passage that appears only when --{n} is given"), format!("{{% if {var} %}}…{{{{ {var} }}}}…{{% endif %}}"));
            }
        }
    }
    push(
        crate::tr!(loc, "命令后面多写的文字（附加要求）接在这里", "Extra text typed after the command goes here"),
        "{% include \"_extra\" %}".into(),
    );
    let name = if d.name.is_empty() { "my-template".to_string() } else { d.name.clone() };
    let try_example = match d.params.first() {
        None => name,
        Some(p) => {
            let n = &p.name;
            let first = p.choices.first().map(|c| c.value.clone());
            match (p.kind, first) {
                (ParamKind::Bool, _) => format!("{name} --{n}"),
                (ParamKind::Enum | ParamKind::Multi, Some(v)) if p.positional => format!("{name} {v}"),
                (ParamKind::Enum | ParamKind::Multi, Some(v)) => format!("{name} --{n}={v}"),
                (ParamKind::Number, _) => format!("{name} --{n}=3"),
                _ => {
                    let sample = crate::tr!(loc, "示例", "example");
                    format!("{name} --{n}={sample}")
                }
            }
        }
    };
    TemplateGuide { try_example, snippets }
}
