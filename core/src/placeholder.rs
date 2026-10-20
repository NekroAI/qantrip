//! `/new` 正文中的占位符（docs/input-grammar.md §7）：解析、生成模板正文，以及输入到一半时的提示。
//!
//! - `{名}`：填空，使用时 `--名=…` 的文字填到这里
//! - `{名:a|b|c}`：单选，取值原样填入；第一个为默认值
//! - `{名:取值=文字|取值=文字|=文字}`：单选，按取值输出不同的文字；`=文字` 是不写这个参数时的文字
//! - `{名?文字|文字}`：开关，加上 `--名` 时输出 `?` 后的文字，否则输出 `|` 后的文字（可省略）

use crate::i18n::Locale;
use crate::template;
use crate::tr;

#[derive(Debug, Clone, PartialEq)]
pub enum Placeholder {
    Fill { name: String },
    Choice {
        name: String,
        /// (取值, 输出的文字)
        options: Vec<(String, String)>,
        /// 不写这个参数时输出的文字（`=文字`）
        unset: Option<String>,
        /// 是否写了 `取值=文字`（否则取值原样输出）
        mapped: bool,
    },
    Flag { name: String, on: String, off: String },
}

impl Placeholder {
    pub fn name(&self) -> &str {
        match self {
            Placeholder::Fill { name } | Placeholder::Choice { name, .. } | Placeholder::Flag { name, .. } => name,
        }
    }
}

/// 选项的取值：英文字母或数字开头，可含 - _ .（会出现在命令里，如 `--env=test`）
fn valid_key(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphanumeric()) && cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// 名称部分：英文字母开头，后面是字母、数字、- 和 _
fn split_name(inner: &str) -> (&str, &str) {
    let end = inner.char_indices().find(|(i, c)| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_') || (*i == 0 && !c.is_ascii_alphabetic())).map_or(inner.len(), |(i, _)| i);
    (&inner[..end], &inner[end..])
}

/// 解析 `{` 与 `}` 之间的内容。`Ok(None)`：不是占位符（原样保留，例如 `{ "a": 1 }`）；`Err`：写法有误。
pub fn parse(inner: &str, loc: Locale) -> Result<Option<Placeholder>, String> {
    let trimmed = inner.trim();
    let (name, rest) = split_name(trimmed);
    if name.is_empty() {
        return Ok(None);
    }
    let rest_trim = rest.trim_start();
    let sep = rest_trim.chars().next();
    if !matches!(sep, None | Some(':') | Some('?')) {
        // 名称后面不是 : 或 ?（如 `{a b}`）：不当作占位符
        return Ok(None);
    }
    if name.starts_with("no-") || !template::valid_name(name) {
        return Err(tr!(loc, "参数名 `{name}` 只能用英文字母、数字、- 和 _，并以字母开头", "Parameter name `{name}` may only use letters, digits, - and _, and must start with a letter"));
    }
    let name = name.to_string();
    match sep {
        None => Ok(Some(Placeholder::Fill { name })),
        Some('?') => {
            let body = &rest_trim[1..];
            let (on, off) = body.split_once('|').unwrap_or((body, ""));
            if on.trim().is_empty() {
                return Err(tr!(loc, "开关 `{name}` 要在 ? 后面写开启时输出的文字", "Switch `{name}`: write the text to output after ?"));
            }
            Ok(Some(Placeholder::Flag { name, on: on.to_string(), off: off.to_string() }))
        }
        _ => {
            let mut options: Vec<(String, String)> = Vec::new();
            let mut unset = None;
            let mut mapped = false;
            for entry in rest_trim[1..].split('|').map(str::trim).filter(|e| !e.is_empty()) {
                match entry.split_once('=') {
                    Some((key, text)) => {
                        mapped = true;
                        let (key, text) = (key.trim(), text.trim());
                        if key.is_empty() {
                            if unset.is_some() {
                                return Err(tr!(loc, "`{name}` 只能有一个 =文字（不写参数时）", "`{name}` can have only one =text (used when the option is left out)"));
                            }
                            unset = Some(text.to_string());
                            continue;
                        }
                        if !valid_key(key) {
                            return Err(tr!(loc, "选项 `{key}` 只能用英文字母、数字、- 和 _（它会出现在命令里，如 --{name}={key}）", "Option `{key}` may only use letters, digits, - and _ (it appears in the command, like --{name}={key})"));
                        }
                        options.push((key.to_string(), text.to_string()));
                    }
                    None => options.push((entry.to_string(), entry.to_string())),
                }
            }
            if options.is_empty() {
                return Err(tr!(loc, "`{name}` 至少要有一个选项，如 {{{name}:a|b}}", "`{name}` needs at least one option, like {{{name}:a|b}}"));
            }
            let mut keys: Vec<&str> = options.iter().map(|(k, _)| k.as_str()).collect();
            keys.sort_unstable();
            if keys.windows(2).any(|w| w[0] == w[1]) {
                return Err(tr!(loc, "`{name}` 的选项有重复", "`{name}` has a duplicate option"));
            }
            Ok(Some(Placeholder::Choice { name, options, unset, mapped }))
        }
    }
}

/// 字面文字：含模板语法时转义
pub fn literal(text: &str) -> String {
    if text.contains("{{") || text.contains("{%") || text.contains("{#") {
        format!("{{% raw %}}{text}{{% endraw %}}")
    } else {
        text.to_string()
    }
}

/// 占位符生成的模板正文片段。条件块后紧跟换行时多补一个换行（模板开启了 trim_blocks，会吃掉块标签后的第一个换行）。
pub fn render(p: &Placeholder, next_is_newline: bool) -> String {
    let var = p.name().replace('-', "_");
    let block = |s: String| if next_is_newline { format!("{s}\n") } else { s };
    match p {
        Placeholder::Fill { .. } => format!("{{{{ {var} }}}}"),
        Placeholder::Choice { options, unset, mapped, .. } => {
            if !mapped {
                return match unset {
                    None => format!("{{{{ {var} }}}}"),
                    Some(u) => block(format!("{{% if {var} %}}{{{{ {var} }}}}{{% else %}}{}{{% endif %}}", literal(u))),
                };
            }
            let mut s = String::new();
            for (i, (key, text)) in options.iter().enumerate() {
                let tag = if i == 0 { "if" } else { "elif" };
                s.push_str(&format!("{{% {tag} {var} == \"{key}\" %}}{}", literal(text)));
            }
            if let Some(u) = unset {
                s.push_str(&format!("{{% else %}}{}", literal(u)));
            }
            s.push_str("{% endif %}");
            block(s)
        }
        Placeholder::Flag { on, off, .. } => {
            let mut s = format!("{{% if {var} %}}{}", literal(on));
            if !off.is_empty() {
                s.push_str(&format!("{{% else %}}{}", literal(off)));
            }
            s.push_str("{% endif %}");
            block(s)
        }
    }
}

/// 两处同名占位符是否可以共存：种类相同；单选的取值相同（各处输出的文字可以不同）
pub fn compatible(a: &Placeholder, b: &Placeholder) -> bool {
    match (a, b) {
        (Placeholder::Fill { .. }, Placeholder::Fill { .. }) | (Placeholder::Flag { .. }, Placeholder::Flag { .. }) => true,
        (Placeholder::Choice { options: x, unset: ux, .. }, Placeholder::Choice { options: y, unset: uy, .. }) => {
            x.iter().map(|(k, _)| k).eq(y.iter().map(|(k, _)| k)) && ux.is_some() == uy.is_some()
        }
        _ => false,
    }
}

/// 正文里所有完整的占位符：(起点, 终点（含 `}`）, 解析结果)。不是占位符的花括号跳过。
pub fn scan(body: &str, loc: Locale) -> Vec<(usize, usize, Result<Placeholder, String>)> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(open) = body[i..].find('{') {
        let s = i + open;
        // `{{` 与 `{%` 等模板语法原样保留
        if bytes.get(s + 1).is_some_and(|b| matches!(b, b'{' | b'%' | b'#')) || (s > 0 && bytes[s - 1] == b'{') {
            i = s + 1;
            continue;
        }
        let Some(close) = body[s + 1..].find('}') else { break };
        let e = s + 1 + close;
        match parse(&body[s + 1..e], loc) {
            Ok(Some(p)) => out.push((s, e + 1, Ok(p))),
            Ok(None) => {}
            Err(m) => out.push((s, e + 1, Err(m))),
        }
        i = e + 1;
    }
    out
}

/// 光标所在的、还没有写完的占位符（`{` 之后没有 `}`）：返回 `{` 的位置。
pub fn open_at_end(body: &str) -> Option<usize> {
    let s = body.rfind('{')?;
    let after = &body[s + 1..];
    if after.contains('}') || after.starts_with(['{', '%', '#']) || (s > 0 && body.as_bytes()[s - 1] == b'{') {
        return None;
    }
    Some(s)
}

/// 正在写的占位符（`{` 之后的内容）对应的提示：下一步可以写什么。返回 (提示, 错误)。
pub fn hint_inside(frag: &str, loc: Locale) -> (String, Option<String>) {
    let (name, rest) = split_name(frag.trim_start());
    if name.is_empty() {
        return if frag.trim().is_empty() {
            (
                tr!(
                    loc,
                    "写参数名（英文字母开头，如 env），接着：写 }} 是填空，写 : 是选项，写 ? 是开关",
                    "Type a parameter name (starting with a letter, e.g. env), then: }} for a blank, : for options, ? for a switch"
                ),
                None,
            )
        } else {
            (String::new(), Some(tr!(loc, "参数名要以英文字母开头", "Parameter names start with a letter")))
        };
    }
    let n = name;
    let rest = rest.trim_start();
    match rest.chars().next() {
        None => (
            tr!(
                loc,
                "参数 {n}：写 }} 结束 → 填空（使用时 --{n}=文字）；写 : → 选项（如 {n}:test=…|prod=…）；写 ? → 开关（如 {n}?加上 --{n} 时的文字）",
                "Parameter {n}: }} ends it → a blank (use it as --{n}=text); : → options (e.g. {n}:test=…|prod=…); ? → a switch (e.g. {n}?text when --{n} is given)"
            ),
            None,
        ),
        Some('?') => {
            let body = &rest[1..];
            let (on, off) = body.split_once('|').map_or((body, None), |(a, b)| (a, Some(b)));
            let on = on.trim();
            let hint = match (on.is_empty(), off) {
                (true, _) => tr!(loc, "开关 --{n}：写加上 --{n} 时输出的文字", "Switch --{n}: type the text to output when --{n} is given"),
                (false, None) => tr!(
                    loc,
                    "开关 --{n}：加上时输出「{on}」。可以写 | 再写不加时的文字；写 }} 结束",
                    "Switch --{n}: outputs \"{on}\" when given. Add | and the text for when it isn't; }} ends it"
                ),
                (false, Some(off)) if off.trim().is_empty() => tr!(
                    loc,
                    "开关 --{n}：加上时输出「{on}」，不加时不输出任何文字。接着写不加时的文字，或写 }} 结束",
                    "Switch --{n}: \"{on}\" when given, nothing otherwise. Type the text for when it isn't, or }} to end"
                ),
                (false, Some(off)) => {
                    let off = off.trim();
                    tr!(loc, "开关 --{n}：加上时输出「{on}」，不加时输出「{off}」。写 }} 结束", "Switch --{n}: \"{on}\" when given, \"{off}\" otherwise. }} ends it")
                }
            };
            (hint, None)
        }
        Some(':') => {
            let entries: Vec<&str> = rest[1..].split('|').collect();
            let current = entries.last().copied().unwrap_or("").trim();
            let done: Vec<String> = entries[..entries.len() - 1]
                .iter()
                .map(|e| e.trim())
                .filter(|e| !e.is_empty())
                .map(|e| match e.split_once('=') {
                    Some((k, t)) if k.trim().is_empty() => {
                        let t = t.trim();
                        tr!(loc, "不写时「{t}」", "left out: \"{t}\"")
                    }
                    Some((k, t)) => format!("{}={}", k.trim(), t.trim()),
                    None => e.to_string(),
                })
                .collect();
            let list = done.join(if loc == Locale::Zh { "、" } else { ", " });
            let so_far = if done.is_empty() { String::new() } else { tr!(loc, "已有：{list}。", "So far: {list}. ") };
            if let Some((key, _)) = current.split_once('=') {
                let key = key.trim();
                if !key.is_empty() && !valid_key(key) {
                    return (String::new(), Some(tr!(loc, "选项 `{key}` 只能用英文字母、数字、- 和 _（它会出现在命令里，如 --{n}={key}）", "Option `{key}` may only use letters, digits, - and _ (it appears in the command, like --{n}={key})")));
                }
                let target = if key.is_empty() { tr!(loc, "不加 --{n}", "--{n} is left out") } else { tr!(loc, "是 --{n}={key}", "says --{n}={key}") };
                return (
                    tr!(loc, "{so_far}接着写命令里{target} 时输出的文字；写 | 开始下一个选项，写 }} 结束", "{so_far}Now type the text to output when the command {target}; | starts the next option, }} ends it"),
                    None,
                );
            }
            (
                tr!(
                    loc,
                    "{so_far}选项 --{n}：写「取值=文字」，如 test=上线到测试环境（命令里写 --{n}=test 时输出这段文字）；只写取值则原样填入；写「=文字」是不写 --{n} 时的文字；用 | 分隔，写 }} 结束",
                    "{so_far}Options for --{n}: write value=text, e.g. test=deploy to staging (output when the command says --{n}=test); a bare value is inserted as is; =text is used when --{n} is left out; separate with |, end with }}"
                ),
                None,
            )
        }
        _ => (tr!(loc, "接着写 }}（填空）、:（选项）或 ?（开关）", "Continue with }} (blank), : (options) or ? (switch)"), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Placeholder {
        parse(s, Locale::Zh).unwrap().unwrap()
    }

    #[test]
    fn parses_each_form() {
        assert_eq!(p("topic"), Placeholder::Fill { name: "topic".into() });
        assert_eq!(p("lang:英文|日文"), Placeholder::Choice { name: "lang".into(), options: vec![("英文".into(), "英文".into()), ("日文".into(), "日文".into())], unset: None, mapped: false });
        assert_eq!(
            p("env:test=上线到测试环境|prod=上线到正式环境|=推送到仓库"),
            Placeholder::Choice {
                name: "env".into(),
                options: vec![("test".into(), "上线到测试环境".into()), ("prod".into(), "上线到正式环境".into())],
                unset: Some("推送到仓库".into()),
                mapped: true
            }
        );
        assert_eq!(p("strict?严格|宽松"), Placeholder::Flag { name: "strict".into(), on: "严格".into(), off: "宽松".into() });
        // 不是占位符
        assert_eq!(parse(" \"a\": 1 ", Locale::Zh), Ok(None));
        assert_eq!(parse("a b", Locale::Zh), Ok(None));
        assert_eq!(parse("1x", Locale::Zh), Ok(None));
        // 写法有误
        assert!(parse("env:测试=x", Locale::Zh).is_err());
        assert!(parse("env:a=x|a=y", Locale::Zh).is_err());
        assert!(parse("env:=x|=y", Locale::Zh).is_err());
        assert!(parse("s?", Locale::Zh).is_err());
        assert!(parse("env:", Locale::Zh).is_err());
    }

    #[test]
    fn hints_follow_each_character() {
        let h = |s: &str| hint_inside(s, Locale::Zh);
        assert!(h("").0.contains("参数名"));
        assert!(h("env").0.contains("参数 env"));
        assert!(h("env:").0.contains("取值=文字"));
        assert!(h("env:test=").0.contains("--env=test"));
        assert!(h("env:test=测试|=").0.contains("不加 --env"));
        assert!(h("env:test=测试|").0.contains("已有：test=测试"));
        assert!(h("s?").0.contains("开关 --s"));
        assert!(h("s?严格").0.contains("严格"));
        assert!(h("s?严格|宽松").0.contains("宽松"));
        assert!(h("env:测试=").1.is_some());
        assert!(h("1").1.is_some());
    }
}
