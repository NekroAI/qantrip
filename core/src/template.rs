//! 模板文件格式（docs/decisions.md D11）：YAML frontmatter + MiniJinja 正文。
//!
//! ```markdown
//! ---
//! name: review
//! aliases: [r]
//! description: 代码审查
//! params:
//!   strict: { type: bool, short: s, description: 严格模式 }
//! ---
//! 正文……{% if strict %}……{% endif %}
//! ```
//!
//! 参数名中的 `-` 在正文里写作 `_`（`keep-terms` → `keep_terms`）。
//! 以 `_` 开头的模板是公共片段，只能被 `{% include %}` 引用，不出现在搜索结果中。

use crate::i18n::Locale;
use crate::tr;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Bool,
    Enum,
    Multi,
    String,
    Number,
    Text,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize, serde::Deserialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Str(String),
    List(Vec<String>),
    Num(f64),
}

#[derive(Debug, Clone)]
pub struct ParamSpec {
    pub name: String,
    pub kind: ParamKind,
    pub description: String,
    pub choices: Vec<Choice>,
    pub default: Option<Value>,
    pub positional: bool,
    pub short: Option<char>,
    pub required: bool,
    pub requires: Vec<String>,
    pub conflicts: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl ParamSpec {
    /// 未设置时的取值：声明的默认值，否则按类型取"空"值。
    pub fn effective_default(&self) -> Value {
        self.default.clone().unwrap_or(match self.kind {
            ParamKind::Bool => Value::Bool(false),
            ParamKind::Multi => Value::List(vec![]),
            _ => Value::Str(String::new()),
        })
    }

    /// 按值或别名精确匹配候选项（不区分大小写）。
    pub fn find_choice(&self, s: &str) -> Option<&Choice> {
        let s = s.to_lowercase();
        self.choices
            .iter()
            .find(|c| c.value.to_lowercase() == s || c.aliases.iter().any(|a| a.to_lowercase() == s))
    }

    pub fn var_name(&self) -> String {
        self.name.replace('-', "_")
    }
}

#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub category: String,
    pub params: Vec<ParamSpec>,
    pub body: String,
    pub builtin: bool,
    /// 原始文件内容（编辑器使用）
    pub source: String,
    /// 用户模板的文件路径；内置模板为 None
    pub path: Option<std::path::PathBuf>,
}

impl Template {
    pub fn is_partial(&self) -> bool {
        self.name.starts_with('_')
    }

    pub fn param(&self, name: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    pub fn positional(&self) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.positional)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFront {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    params: serde_norway::Mapping,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawParam {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    choices: Vec<RawChoice>,
    default: Option<serde_norway::Value>,
    #[serde(default)]
    positional: bool,
    short: Option<String>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    conflicts: Vec<String>,
    min: Option<f64>,
    max: Option<f64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawChoice {
    Plain(String),
    Full {
        value: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        aliases: Vec<String>,
    },
}

fn valid_name(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic())
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 解析一个模板文件（中文错误信息）。`builtin` 只用于标记来源。
pub fn parse(source: &str, builtin: bool) -> Result<Template, String> {
    parse_in(source, builtin, Locale::Zh)
}

/// 解析一个模板文件，错误信息使用 `loc`。
pub fn parse_in(source: &str, builtin: bool, loc: Locale) -> Result<Template, String> {
    let rest = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))
        .ok_or_else(|| tr!(loc, "缺少开头的 --- frontmatter", "Missing the opening --- of the front matter"))?;
    let end = rest.find("\n---").ok_or_else(|| tr!(loc, "frontmatter 没有以 --- 结束", "The front matter doesn't end with ---"))?;
    let front = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();

    let raw: RawFront = serde_norway::from_str(front).map_err(|e| tr!(loc, "frontmatter 格式错误：{e}", "Invalid front matter: {e}"))?;
    let partial = raw.name.starts_with('_');
    if !partial && !valid_name(&raw.name) {
        let n = &raw.name;
        return Err(tr!(loc, "模板名 `{n}` 只能包含字母、数字、- 和 _，并以字母开头", "Template name `{n}` may only contain letters, digits, - and _, and must start with a letter"));
    }

    let mut params = Vec::new();
    for (k, v) in raw.params {
        let name = k.as_str().ok_or_else(|| tr!(loc, "参数名必须是字符串", "Parameter names must be strings"))?.to_string();
        if !valid_name(&name) || name.starts_with("no-") {
            return Err(tr!(loc, "参数名 `{name}` 不合法", "Invalid parameter name `{name}`"));
        }
        let rp: RawParam = serde_norway::from_value(v).map_err(|e| tr!(loc, "参数 `{name}`：{e}", "Parameter `{name}`: {e}"))?;
        params.push(build_param(name, rp, loc)?);
    }
    validate_params(&params, loc)?;

    Ok(Template {
        name: raw.name,
        aliases: raw.aliases,
        description: raw.description,
        category: raw.category,
        params,
        body,
        builtin,
        source: source.to_string(),
        path: None,
    })
}

fn build_param(name: String, rp: RawParam, loc: Locale) -> Result<ParamSpec, String> {
    let kind = match rp.kind.as_str() {
        "bool" => ParamKind::Bool,
        "enum" => ParamKind::Enum,
        "multi" => ParamKind::Multi,
        "string" => ParamKind::String,
        "number" => ParamKind::Number,
        "text" => ParamKind::Text,
        other => return Err(tr!(loc, "参数 `{name}`：未知类型 `{other}`", "Parameter `{name}`: unknown type `{other}`")),
    };
    let choices: Vec<Choice> = rp
        .choices
        .into_iter()
        .map(|c| match c {
            RawChoice::Plain(v) => Choice { label: v.clone(), value: v, aliases: vec![] },
            RawChoice::Full { value, label, aliases } => {
                Choice { label: if label.is_empty() { value.clone() } else { label }, value, aliases }
            }
        })
        .collect();
    let has_choices = matches!(kind, ParamKind::Enum | ParamKind::Multi);
    if has_choices && choices.is_empty() {
        return Err(tr!(loc, "参数 `{name}`：enum / multi 必须声明 choices", "Parameter `{name}`: enum / multi needs choices"));
    }
    if !has_choices && !choices.is_empty() {
        return Err(tr!(loc, "参数 `{name}`：只有 enum / multi 可以声明 choices", "Parameter `{name}`: only enum / multi can have choices"));
    }
    if rp.positional && !has_choices {
        return Err(tr!(loc, "参数 `{name}`：只有 enum / multi 可以声明为位置参数", "Parameter `{name}`: only enum / multi can be positional"));
    }
    let short = match rp.short {
        None => None,
        Some(s) => {
            let mut cs = s.chars();
            match (cs.next(), cs.next()) {
                (Some(c), None) if c.is_ascii_alphabetic() && kind == ParamKind::Bool => Some(c),
                _ => return Err(tr!(loc, "参数 `{name}`：short 必须是单个字母，且只能用于 bool 参数", "Parameter `{name}`: short must be a single letter and only works on bool parameters")),
            }
        }
    };

    let spec = ParamSpec {
        name: name.clone(),
        kind,
        description: rp.description,
        choices,
        default: None,
        positional: rp.positional,
        short,
        required: rp.required,
        requires: rp.requires,
        conflicts: rp.conflicts,
        min: rp.min,
        max: rp.max,
    };
    let default = match rp.default {
        None => None,
        Some(v) => Some(convert_default(&spec, v, loc).map_err(|e| tr!(loc, "参数 `{name}` 的默认值：{e}", "Default of parameter `{name}`: {e}"))?),
    };
    Ok(ParamSpec { default, ..spec })
}

fn convert_default(spec: &ParamSpec, v: serde_norway::Value, loc: Locale) -> Result<Value, String> {
    use serde_norway::Value as Y;
    let check_choice = |s: &str| -> Result<String, String> {
        spec.find_choice(s).map(|c| c.value.clone()).ok_or_else(|| tr!(loc, "`{s}` 不在 choices 中", "`{s}` is not one of the choices"))
    };
    Ok(match (spec.kind, v) {
        (ParamKind::Bool, Y::Bool(b)) => Value::Bool(b),
        (ParamKind::Enum, Y::String(s)) => Value::Str(check_choice(&s)?),
        (ParamKind::Multi, Y::Sequence(items)) => Value::List(
            items
                .into_iter()
                .map(|i| i.as_str().ok_or_else(|| tr!(loc, "必须是字符串列表", "Must be a list of strings")).and_then(check_choice))
                .collect::<Result<_, _>>()?,
        ),
        (ParamKind::String | ParamKind::Text, Y::String(s)) => Value::Str(s),
        (ParamKind::Number, Y::Number(n)) => Value::Num(n.as_f64().ok_or_else(|| tr!(loc, "不是有效数字", "Not a valid number"))?),
        _ => return Err(tr!(loc, "类型与参数不符", "Doesn't match the parameter type")),
    })
}

fn validate_params(params: &[ParamSpec], loc: Locale) -> Result<(), String> {
    if params.iter().filter(|p| p.positional).count() > 1 {
        return Err(tr!(loc, "至多一个参数可以声明为位置参数", "At most one parameter can be positional"));
    }
    for (i, p) in params.iter().enumerate() {
        if let Some(c) = p.short
            && params[..i].iter().any(|q| q.short == Some(c))
        {
            return Err(tr!(loc, "短名 -{c} 重复", "Short name -{c} is used twice"));
        }
        for r in p.requires.iter().chain(&p.conflicts) {
            if !params.iter().any(|q| &q.name == r) {
                let n = &p.name;
                return Err(tr!(loc, "参数 `{n}` 引用了不存在的参数 `{r}`", "Parameter `{n}` refers to unknown parameter `{r}`"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let t = parse(
            "---\nname: t\naliases: [x]\nparams:\n  b: { type: bool, short: s }\n  e:\n    type: enum\n    default: two\n    choices: [one, { value: two, label: 二, aliases: [\"2\"] }]\n---\nbody {{ b }}\n",
            true,
        )
        .unwrap();
        assert_eq!(t.name, "t");
        assert_eq!(t.params[0].short, Some('s'));
        assert_eq!(t.params[1].default, Some(Value::Str("two".into())));
        assert_eq!(t.params[1].find_choice("2").unwrap().value, "two");
        assert_eq!(t.body, "body {{ b }}\n");
    }

    #[test]
    fn rejects_bad_schema() {
        let bad = [
            "---\nname: t\nparams:\n  e: { type: enum }\n---\n",
            "---\nname: t\nparams:\n  s: { type: string, positional: true }\n---\n",
            "---\nname: t\nparams:\n  s: { type: string, short: x }\n---\n",
            "---\nname: t\nparams:\n  e: { type: enum, choices: [a], default: b }\n---\n",
            "---\nname: t\nparams:\n  a: { type: bool, requires: [zz] }\n---\n",
            "---\nname: t\nunknown: 1\n---\n",
            "no frontmatter",
        ];
        for b in bad {
            assert!(parse(b, true).is_err(), "should reject: {b}");
        }
    }
}
