//! 输入语法解析（docs/input-grammar.md）。
//!
//! `analyze` 把面板输入解析为界面需要的全部信息。所有位置都是输入（或预览）
//! 字符串中的 UTF-16 偏移，便于 Swift 直接构造 `NSRange`。

use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use crate::i18n::Locale;
use crate::library::Library;
use crate::tr;
use crate::template::{ParamKind, ParamSpec, Template, Value};
use crate::usage::Usage;

// MARK: - 对外类型

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, uniffi::Enum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchTier {
    Exact,
    Prefix,
    Fuzzy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    Command,
    CommandUnknown,
    OptionName,
    OptionValue,
    ShortFlag,
    Positional,
    ExtraMarker,
    Extra,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct Token {
    pub kind: TokenKind,
    pub start: u32,
    pub end: u32,
}

/// 对输入的一次替换：把 `[start, end)` 换成 `text`。
#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct Edit {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub start: u32,
    pub end: u32,
    /// 位于正在输入的最后一个词上（界面可以弱化显示）。
    pub at_cursor: bool,
    /// Tab 可接受的修正建议。
    pub fix: Option<Edit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    Template,
    Param,
    Choice,
    /// 首屏的"最近施放 / 试试看"：完整调用，`key` 为输入文本
    Invocation,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct Candidate {
    pub kind: CandidateKind,
    /// 模板名 / 参数名 / 候选值
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub detail: String,
    /// Tab 选中该项时对输入的修改
    pub edit: Edit,
    pub tier: Option<MatchTier>,
    /// 候选值已被选中（多选参数）
    pub active: bool,
    /// 首屏分段标题；输入后为空
    pub section: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct ChoiceState {
    pub value: String,
    pub label: String,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct ParamState {
    pub name: String,
    pub kind: ParamKind,
    pub short: Option<String>,
    pub description: String,
    pub display: String,
    pub is_set: bool,
    pub required: bool,
    pub positional: bool,
    pub choices: Vec<ChoiceState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightKind {
    Param,
    Extra,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct Highlight {
    pub kind: HighlightKind,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct Preview {
    pub text: String,
    pub highlights: Vec<Highlight>,
    pub chars: u32,
    pub lines: u32,
    /// 按 Claude Code 的规则会被折叠为 `[Pasted text]`（docs/decisions.md D13）
    pub collapses_in_claude_code: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct TemplateInfo {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub category: String,
    pub builtin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Command,
    Params,
    Extra,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnterAction {
    Insert,
    /// 先修改输入（模糊匹配补全命令词 / 补上缺失的必填参数），不插入
    Complete { edit: Edit },
    Blocked,
}

/// 鼠标对参数的操作（docs/decisions.md D16）
#[derive(Debug, Clone, PartialEq, uniffi::Enum, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParamEdit {
    /// 开关参数取反
    Toggle,
    /// 设置单选 / 文本 / 数字的值
    Set { value: String },
    /// 多选参数中加入或移除一个值
    ToggleChoice { value: String },
    /// 恢复默认
    Clear,
    /// 在输入中放置 `--name=` 并把光标移到值的位置（文本 / 数字参数）
    Begin,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct EditResult {
    pub text: String,
    /// 新光标位置（UTF-16）
    pub caret: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct Analysis {
    pub tokens: Vec<Token>,
    pub phase: Phase,
    pub template: Option<TemplateInfo>,
    pub locked: bool,
    pub candidates: Vec<Candidate>,
    /// 光标在末尾时显示的灰字补全
    pub ghost: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub params: Vec<ParamState>,
    pub preview: Option<Preview>,
    pub canonical: Option<String>,
    pub enter: EnterAction,
    pub hint: String,
}

// MARK: - 词法

struct Word<'a> {
    raw: &'a str,
    start: usize,
    end: usize,
    /// 参数区规范化后的文本（docs/input-grammar.md §1）
    norm: String,
}

fn is_ws(c: char) -> bool {
    c.is_whitespace()
}

fn closing_quote(c: char) -> Option<char> {
    match c {
        '"' => Some('"'),
        '\'' => Some('\''),
        '“' => Some('”'),
        '‘' => Some('’'),
        _ => None,
    }
}

/// 按空白切词；引号内的空白不切分。未闭合的引号延续到输入末尾。
fn scan(input: &str) -> Vec<Word<'_>> {
    let mut words = Vec::new();
    let mut it = input.char_indices().peekable();
    while let Some(&(i, c)) = it.peek() {
        if is_ws(c) {
            it.next();
            continue;
        }
        let start = i;
        let mut end = input.len();
        let mut quote: Option<char> = None;
        while let Some(&(j, ch)) = it.peek() {
            if let Some(q) = quote {
                it.next();
                if ch == '\\' && q == '"' {
                    it.next();
                } else if ch == q {
                    quote = None;
                }
                continue;
            }
            if is_ws(ch) {
                end = j;
                break;
            }
            quote = closing_quote(ch);
            it.next();
        }
        let raw = &input[start..end];
        words.push(Word { raw, start, end, norm: normalize(raw) });
    }
    words
}

fn normalize(word: &str) -> String {
    let mut s = String::with_capacity(word.len());
    let body = if let Some(r) = word.strip_prefix("——") {
        s.push_str("--");
        r
    } else if let Some(r) = word.strip_prefix('—').filter(|r| r.starts_with(|c: char| c.is_alphabetic())) {
        s.push_str("--");
        r
    } else {
        word
    };
    for c in body.chars() {
        s.push(match c {
            '－' => '-',
            '＝' => '=',
            '，' | '、' => ',',
            '“' | '”' => '"',
            '‘' | '’' => '\'',
            _ => c,
        });
    }
    s
}

/// 拆分多选值：逗号分隔，引号内的逗号不拆分。
fn split_items(raw: &str, loc: Locale) -> Result<Vec<String>, String> {
    let unclosed = || tr!(loc, "引号未闭合", "Unclosed quote");
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut chars = raw.chars();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = true;
                loop {
                    match chars.next() {
                        None => return Err(unclosed()),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => cur.push('\n'),
                            Some('t') => cur.push('\t'),
                            Some(e) => cur.push(e),
                            None => return Err(unclosed()),
                        },
                        Some(x) => cur.push(x),
                    }
                }
            }
            '\'' => {
                quoted = true;
                loop {
                    match chars.next() {
                        None => return Err(unclosed()),
                        Some('\'') => break,
                        Some(x) => cur.push(x),
                    }
                }
            }
            ',' => {
                if !cur.is_empty() || quoted {
                    items.push(std::mem::take(&mut cur));
                }
                quoted = false;
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() || quoted {
        items.push(cur);
    }
    Ok(items)
}

/// 单个值（字符串 / 数字参数）：去掉引号，不按逗号拆分。
fn unquote(raw: &str, loc: Locale) -> Result<String, String> {
    if raw.starts_with('"') || raw.starts_with('\'') {
        let items = split_items(raw, loc)?;
        if items.len() == 1 && (raw.ends_with('"') || raw.ends_with('\'')) {
            return Ok(items.into_iter().next().unwrap());
        }
    }
    Ok(raw.to_string())
}

fn quote_if_needed(s: &str) -> String {
    let needs = s.is_empty() || s.chars().any(|c| is_ws(c) || matches!(c, ',' | '"' | '\'' | '\\'));
    if !needs {
        return s.to_string();
    }
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

// MARK: - 匹配

fn edit_distance(a: &str, b: &str) -> usize {
    // Damerau–Levenshtein（相邻交换算一次编辑）
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

/// 返回 (层级, 层内次序)。次序越小越靠前。
fn match_name(q: &str, name: &str) -> Option<(MatchTier, usize)> {
    let n = name.to_lowercase();
    if n == q {
        return Some((MatchTier::Exact, 0));
    }
    if n.starts_with(q) {
        return Some((MatchTier::Prefix, n.chars().count()));
    }
    let qn = q.chars().count();
    let limit = if qn <= 3 { 1 } else { 2 };
    let dist = edit_distance(q, &n).min(edit_distance(q, &n.chars().take(qn).collect::<String>()) + 1);
    if qn >= 2 && dist <= limit {
        return Some((MatchTier::Fuzzy, dist));
    }
    if qn >= 2 && is_subsequence(q, &n) {
        return Some((MatchTier::Fuzzy, 3 + n.chars().count() - qn));
    }
    None
}

fn match_templates<'a>(lib: &'a Library, usage: &Usage, cmd: &str) -> Vec<(&'a Template, MatchTier)> {
    let q = cmd.to_lowercase();
    let mut out: Vec<(&Template, MatchTier, usize)> = lib
        .visible()
        .filter_map(|t| {
            if q.is_empty() {
                return Some((t, MatchTier::Prefix, 0));
            }
            std::iter::once(&t.name)
                .chain(&t.aliases)
                .filter_map(|n| match_name(&q, n))
                .min()
                .map(|(tier, rank)| (t, tier, rank))
        })
        .collect();
    // 层级优先；层内按使用频率（面板打开时的快照）、匹配次序、名称（§3）
    out.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then(usage.score(&b.0.name).total_cmp(&usage.score(&a.0.name)))
            .then((a.2, &a.0.name).cmp(&(b.2, &b.0.name)))
    });
    out.into_iter().map(|(t, tier, _)| (t, tier)).collect()
}

/// 唯一前缀解析：精确优先；否则唯一的前缀匹配；否则返回全部前缀匹配项。
fn resolve_prefix<'a, T>(q: &str, items: &'a [T], keys: impl Fn(&T) -> Vec<String>) -> Result<&'a T, Vec<&'a T>> {
    let q = q.to_lowercase();
    if let Some(x) = items.iter().find(|x| keys(x).iter().any(|k| k.to_lowercase() == q)) {
        return Ok(x);
    }
    let hits: Vec<&T> = items.iter().filter(|x| keys(x).iter().any(|k| k.to_lowercase().starts_with(&q))).collect();
    if hits.len() == 1 { Ok(hits[0]) } else { Err(hits) }
}

// MARK: - 解析

struct Ctx<'a> {
    loc: Locale,
    input: &'a str,
    tokens: Vec<Token>,
    diags: Vec<Diagnostic>,
}

impl Ctx<'_> {
    fn u16(&self, byte: usize) -> u32 {
        self.input[..byte].encode_utf16().count() as u32
    }
    fn token(&mut self, kind: TokenKind, start: usize, end: usize) {
        let (s, e) = (self.u16(start), self.u16(end));
        if e > s {
            self.tokens.push(Token { kind, start: s, end: e });
        }
    }
    fn diag(&mut self, sev: Severity, msg: impl Into<String>, start: usize, end: usize, at_cursor: bool, fix: Option<Edit>) {
        let (s, e) = (self.u16(start), self.u16(end));
        self.diags.push(Diagnostic { severity: sev, message: msg.into(), start: s, end: e, at_cursor, fix });
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Long,
    Short,
    Positional,
}

struct Parsed {
    values: BTreeMap<String, Value>,
    sources: BTreeMap<String, Source>,
    extra: Option<String>,
    extra_start: Option<usize>,
}

fn choice_list(p: &ParamSpec) -> String {
    p.choices.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(" / ")
}

fn resolve_choice(p: &ParamSpec, item: &str, loc: Locale) -> Result<String, String> {
    match resolve_prefix(item, &p.choices, |c| std::iter::once(c.value.clone()).chain(c.aliases.clone()).collect()) {
        Ok(c) => Ok(c.value.clone()),
        Err(hits) if hits.len() > 1 => {
            let opts = hits.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(" / ");
            Err(tr!(loc, "`{item}` 有歧义：{opts}", "`{item}` is ambiguous: {opts}"))
        }
        Err(_) => {
            let (name, list) = (&p.name, choice_list(p));
            Err(tr!(loc, "--{name} 没有候选值 `{item}`，可选：{list}", "--{name} has no value `{item}`; options: {list}"))
        }
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn set_value(parsed: &mut Parsed, ctx: &mut Ctx, p: &ParamSpec, v: Value, src: Source, span: (usize, usize), at_cursor: bool) {
    let prev = parsed.sources.get(&p.name).copied();
    match (&v, parsed.values.get_mut(&p.name)) {
        // 多选：合并去重并保持顺序（§4.1）
        (Value::List(new), Some(Value::List(old))) if prev.is_some_and(|s| (s == Source::Positional) == (src == Source::Positional)) => {
            for x in new {
                if !old.contains(x) {
                    old.push(x.clone());
                }
            }
            return;
        }
        _ => {}
    }
    match (prev, src) {
        // 位置参数与长选项同时出现：以长选项为准并警告（§4.3）
        (Some(Source::Long), Source::Positional) => {
            let name = &p.name;
            ctx.diag(Severity::Warning, tr!(ctx.loc, "已被 --{name} 覆盖", "Overridden by --{name}"), span.0, span.1, at_cursor, None);
            return;
        }
        (Some(Source::Positional), Source::Long) => {
            let name = &p.name;
            ctx.diag(Severity::Warning, tr!(ctx.loc, "--{name} 覆盖了前面的位置参数", "--{name} overrides the positional value"), span.0, span.1, at_cursor, None);
        }
        (Some(_), _) if p.kind != ParamKind::Multi => {
            let name = &p.name;
            ctx.diag(Severity::Warning, tr!(ctx.loc, "--{name} 重复设置，以最后一次为准", "--{name} is set twice; the last one wins"), span.0, span.1, at_cursor, None);
        }
        _ => {}
    }
    parsed.values.insert(p.name.clone(), v);
    parsed.sources.insert(p.name.clone(), src);
}

fn closest_param<'a>(t: &'a Template, name: &str) -> Option<&'a ParamSpec> {
    t.params
        .iter()
        .map(|p| (edit_distance(name, &p.name), p))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, p)| p)
}

fn parse_long(t: &Template, w: &Word, at_cursor: bool, parsed: &mut Parsed, ctx: &mut Ctx) {
    let body = &w.norm[2..];
    let (name_part, value_part) = match body.find('=') {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    // 原文中 `=`（或 `＝`）的位置，用于切分高亮
    let eq = w.raw.find(['=', '＝']).map(|i| w.start + i);
    let name_end = eq.unwrap_or(w.end);
    let value_start = eq.map(|i| i + w.raw[i - w.start..].chars().next().unwrap().len_utf8());

    let (negated, lookup) = match name_part.strip_prefix("no-") {
        Some(rest) if t.param(name_part).is_none() => (true, rest),
        _ => (false, name_part),
    };
    let p = match resolve_prefix(lookup, &t.params, |p| vec![p.name.clone()]) {
        Ok(p) => p,
        Err(hits) => {
            ctx.token(TokenKind::Invalid, w.start, w.end);
            if lookup.is_empty() {
                if !at_cursor {
                    ctx.diag(Severity::Error, tr!(ctx.loc, "`--` 后需要参数名", "Expected an option name after `--`"), w.start, w.end, at_cursor, None);
                }
            } else if hits.len() > 1 {
                let names = hits.iter().map(|p| format!("--{}", p.name)).collect::<Vec<_>>().join(" / ");
                ctx.diag(Severity::Error, tr!(ctx.loc, "--{lookup} 有歧义：{names}", "--{lookup} is ambiguous: {names}"), w.start, name_end, at_cursor, None);
            } else {
                let fix = closest_param(t, lookup).map(|c| Edit {
                    start: ctx.u16(w.start),
                    end: ctx.u16(name_end),
                    text: format!("--{}{}", if negated { "no-" } else { "" }, c.name),
                });
                let msg = match &fix {
                    Some(f) => {
                        let s = &f.text;
                        tr!(ctx.loc, "未知参数 --{lookup}，是否是 {s}？", "Unknown option --{lookup}. Did you mean {s}?")
                    }
                    None => tr!(ctx.loc, "未知参数 --{lookup}", "Unknown option --{lookup}"),
                };
                ctx.diag(Severity::Error, msg, w.start, name_end, at_cursor, fix);
            }
            return;
        }
    };
    ctx.token(TokenKind::OptionName, w.start, name_end);
    if let (Some(vs), Some(_)) = (value_start, value_part) {
        ctx.token(TokenKind::OptionValue, vs, w.end);
    }
    let vspan = (value_start.unwrap_or(w.start), w.end);

    if negated {
        if p.kind != ParamKind::Bool {
            let name = &p.name;
            ctx.diag(Severity::Error, tr!(ctx.loc, "--no- 只能用于开关参数，--{name} 不是开关", "--no- only works with switches; --{name} is not a switch"), w.start, name_end, at_cursor, None);
        } else if value_part.is_some() {
            let name = &p.name;
            ctx.diag(Severity::Error, tr!(ctx.loc, "--no-{name} 不接受值", "--no-{name} doesn't take a value"), w.start, w.end, at_cursor, None);
        } else {
            set_value(parsed, ctx, p, Value::Bool(false), Source::Long, (w.start, w.end), at_cursor);
        }
        return;
    }

    let Some(raw) = value_part else {
        if p.kind == ParamKind::Bool {
            set_value(parsed, ctx, p, Value::Bool(true), Source::Long, (w.start, w.end), at_cursor);
        } else {
            let fix = Edit { start: ctx.u16(w.start), end: ctx.u16(w.end), text: format!("--{}=", p.name) };
            let name = &p.name;
            ctx.diag(Severity::Error, tr!(ctx.loc, "--{name} 需要一个值：--{name}=…", "--{name} needs a value: --{name}=…"), w.start, w.end, at_cursor, Some(fix));
        }
        return;
    };

    if raw.is_empty() && p.kind != ParamKind::String && p.kind != ParamKind::Text {
        let (name, list) = (&p.name, choice_list(p));
        let msg = if p.choices.is_empty() {
            tr!(ctx.loc, "--{name} 需要一个值", "--{name} needs a value")
        } else {
            tr!(ctx.loc, "--{name} 需要一个值，可选：{list}", "--{name} needs a value; options: {list}")
        };
        ctx.diag(Severity::Error, msg, w.start, w.end, at_cursor, None);
        return;
    }
    let loc = ctx.loc;
    let name = &p.name;
    let result: Result<Value, String> = match p.kind {
        ParamKind::Bool => parse_bool(raw).map(Value::Bool).ok_or(tr!(loc, "--{name} 是开关，值只能是 true / false", "--{name} is a switch; use true or false")),
        ParamKind::Enum => unquote(raw, loc).and_then(|v| resolve_choice(p, &v, loc)).map(Value::Str),
        ParamKind::Multi => split_items(raw, loc).and_then(|items| {
            let mut out: Vec<String> = Vec::new();
            for i in items {
                let v = resolve_choice(p, &i, loc)?;
                if !out.contains(&v) {
                    out.push(v);
                }
            }
            let list = choice_list(p);
            if out.is_empty() { Err(tr!(loc, "--{name} 需要至少一个值，可选：{list}", "--{name} needs at least one value; options: {list}")) } else { Ok(Value::List(out)) }
        }),
        ParamKind::String | ParamKind::Text => unquote(raw, loc).map(Value::Str),
        ParamKind::Number => unquote(raw, loc).and_then(|v| {
            let n: f64 = v.parse().map_err(|_| tr!(loc, "--{name} 需要数字", "--{name} needs a number"))?;
            if p.min.is_some_and(|m| n < m) || p.max.is_some_and(|m| n > m) {
                let (lo, hi) = (p.min.map(fmt_num).unwrap_or("…".into()), p.max.map(fmt_num).unwrap_or("…".into()));
                return Err(tr!(loc, "--{name} 取值范围 {lo}–{hi}", "--{name} must be between {lo} and {hi}"));
            }
            Ok(Value::Num(n))
        }),
    };
    match result {
        Ok(v) => set_value(parsed, ctx, p, v, Source::Long, (w.start, w.end), at_cursor),
        Err(e) => ctx.diag(Severity::Error, e, vspan.0, vspan.1, at_cursor, None),
    }
}

fn parse_short(t: &Template, w: &Word, at_cursor: bool, parsed: &mut Parsed, ctx: &mut Ctx) {
    let letters = &w.norm[1..];
    let mut ok = true;
    for c in letters.chars() {
        match t.params.iter().find(|p| p.short == Some(c)) {
            Some(p) => set_value(parsed, ctx, p, Value::Bool(true), Source::Short, (w.start, w.end), at_cursor),
            None => {
                ok = false;
                ctx.diag(Severity::Error, tr!(ctx.loc, "未知短参数 -{c}", "Unknown flag -{c}"), w.start, w.end, at_cursor, None);
            }
        }
    }
    ctx.token(if ok { TokenKind::ShortFlag } else { TokenKind::Invalid }, w.start, w.end);
}

fn parse_params(t: &Template, words: &[Word], ends_with_ws: bool, ctx: &mut Ctx) -> Parsed {
    let mut parsed = Parsed { values: BTreeMap::new(), sources: BTreeMap::new(), extra: None, extra_start: None };
    let last = words.len().saturating_sub(1);
    for (i, w) in words.iter().enumerate() {
        let at_cursor = i == last && !ends_with_ws;
        if w.norm == "--" && at_cursor {
            // 尚未输入空白的 `--` 视为正在输入参数名（PRD 3.4：输入 `r --` 时展示参数）
            ctx.token(TokenKind::OptionName, w.start, w.end);
            continue;
        }
        if w.norm == "--" {
            ctx.token(TokenKind::ExtraMarker, w.start, w.end);
            let rest = ctx.input[w.end..].trim();
            if !rest.is_empty() {
                let s = w.end + (ctx.input[w.end..].len() - ctx.input[w.end..].trim_start().len());
                ctx.token(TokenKind::Extra, s, s + rest.len());
                parsed.extra_start = Some(s);
            }
            parsed.extra = Some(rest.to_string()).filter(|s| !s.is_empty());
            parsed.extra_start = parsed.extra_start.or(Some(w.end));
            return parsed;
        }
        if w.norm.starts_with("--") {
            parse_long(t, w, at_cursor, &mut parsed, ctx);
        } else if w.norm.starts_with('-') && w.norm.len() > 1 {
            parse_short(t, w, at_cursor, &mut parsed, ctx);
        } else if let Some(p) = t.positional().filter(|p| p.find_choice(&w.norm).is_some()) {
            ctx.token(TokenKind::Positional, w.start, w.end);
            let v = p.find_choice(&w.norm).unwrap().value.clone();
            let v = if p.kind == ParamKind::Multi { Value::List(vec![v]) } else { Value::Str(v) };
            set_value(&mut parsed, ctx, p, v, Source::Positional, (w.start, w.end), at_cursor);
        } else {
            // 隐式附加文本：从这个词起全部原文保留（§4.4）
            let text = ctx.input[w.start..].trim_end();
            ctx.token(TokenKind::Extra, w.start, w.start + text.len());
            parsed.extra = Some(text.to_string());
            parsed.extra_start = Some(w.start);
            return parsed;
        }
    }
    parsed
}

fn validate(t: &Template, parsed: &Parsed, ctx: &mut Ctx) -> (Option<Edit>, usize) {
    let mut missing = 0;
    let end = ctx.input.len();
    let mut fill: Option<Edit> = None;
    for p in &t.params {
        let set = parsed.values.contains_key(&p.name);
        if p.required && !set {
            let at = ctx.u16(end);
            let sep = if ctx.input.ends_with(is_ws) { "" } else { " " };
            let edit = Edit { start: at, end: at, text: format!("{sep}--{}=", p.name) };
            fill.get_or_insert(edit.clone());
            missing += 1;
            let name = &p.name;
            ctx.diags.push(Diagnostic {
                severity: Severity::Error,
                message: tr!(ctx.loc, "缺少必填参数 --{name}", "Missing required option --{name}"),
                start: at,
                end: at,
                at_cursor: false,
                fix: Some(edit),
            });
        }
        let active = parsed.values.get(&p.name).is_some_and(|v| *v != Value::Bool(false));
        if !active {
            continue;
        }
        for r in &p.requires {
            if !parsed.values.contains_key(r) {
                let name = &p.name;
                let msg = tr!(ctx.loc, "--{name} 需要同时设置 --{r}", "--{name} requires --{r}");
                ctx.diag(Severity::Error, msg, 0, end, false, None);
            }
        }
        for c in &p.conflicts {
            if parsed.values.get(c).is_some_and(|v| *v != Value::Bool(false)) && p.name < *c {
                let name = &p.name;
                let msg = tr!(ctx.loc, "--{name} 与 --{c} 不能同时使用", "--{name} and --{c} can't be used together");
                ctx.diag(Severity::Error, msg, 0, end, false, None);
            }
        }
    }
    (fill, missing)
}

// MARK: - 展示

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

fn label_of(p: &ParamSpec, v: &str) -> String {
    p.choices.iter().find(|c| c.value == v).map(|c| c.label.clone()).unwrap_or_else(|| v.to_string())
}

fn display(p: &ParamSpec, v: &Value, loc: Locale) -> String {
    match v {
        Value::Bool(b) => match (b, loc) {
            (true, Locale::Zh) => "开".into(),
            (false, Locale::Zh) => "关".into(),
            (true, Locale::En) => "on".into(),
            (false, Locale::En) => "off".into(),
        },
        Value::Str(s) if s.is_empty() => "—".into(),
        Value::Str(s) if p.kind == ParamKind::Enum => label_of(p, s),
        Value::Str(s) => s.clone(),
        Value::List(l) if l.is_empty() => "—".into(),
        Value::List(l) => l.iter().map(|x| label_of(p, x)).collect::<Vec<_>>().join(if loc == Locale::Zh { "、" } else { ", " }),
        Value::Num(n) => fmt_num(*n),
    }
}

fn param_states(t: &Template, values: &BTreeMap<String, Value>, loc: Locale) -> Vec<ParamState> {
    t.params
        .iter()
        .map(|p| {
            let v = values.get(&p.name).cloned().unwrap_or_else(|| p.effective_default());
            let on = |c: &str| match &v {
                Value::Str(s) => s == c,
                Value::List(l) => l.iter().any(|x| x == c),
                _ => false,
            };
            ParamState {
                name: p.name.clone(),
                kind: p.kind,
                short: p.short.map(|c| c.to_string()),
                description: p.description.clone(),
                display: display(p, &v, loc),
                is_set: values.contains_key(&p.name),
                required: p.required,
                positional: p.positional,
                choices: p.choices.iter().map(|c| ChoiceState { value: c.value.clone(), label: c.label.clone(), on: on(&c.value) }).collect(),
            }
        })
        .collect()
}

fn info(t: &Template) -> TemplateInfo {
    TemplateInfo {
        name: t.name.clone(),
        aliases: t.aliases.clone(),
        description: t.description.clone(),
        category: t.category.clone(),
        builtin: t.builtin,
    }
}

/// 规范形式（§6）：参数按 schema 顺序、省略默认值，附加文本用 `--` 显式开始。
pub fn canonical(t: &Template, values: &BTreeMap<String, Value>, extra: Option<&str>) -> String {
    let mut out = t.name.clone();
    for p in &t.params {
        let Some(v) = values.get(&p.name) else { continue };
        if *v == p.effective_default() {
            continue;
        }
        out.push(' ');
        match v {
            Value::Bool(true) => out.push_str(&format!("--{}", p.name)),
            Value::Bool(false) => out.push_str(&format!("--no-{}", p.name)),
            Value::Str(s) => out.push_str(&format!("--{}={}", p.name, quote_if_needed(s))),
            Value::List(l) => out.push_str(&format!(
                "--{}={}",
                p.name,
                l.iter().map(|s| quote_if_needed(s)).collect::<Vec<_>>().join(",")
            )),
            Value::Num(n) => out.push_str(&format!("--{}={}", p.name, fmt_num(*n))),
        }
    }
    if let Some(e) = extra.filter(|e| !e.is_empty()) {
        out.push_str(" -- ");
        out.push_str(e);
    }
    out
}

fn utf16_offsets(s: &str) -> Vec<u32> {
    // 第 i 个字符起始处的 UTF-16 偏移（末尾多一个总长度）
    let mut v = Vec::with_capacity(s.len() + 1);
    let mut acc = 0u32;
    for c in s.chars() {
        v.push(acc);
        acc += c.len_utf16() as u32;
    }
    v.push(acc);
    v
}

fn preview(lib: &Library, t: &Template, values: &BTreeMap<String, Value>, extra: Option<&str>) -> Result<Preview, String> {
    let text = lib.render(t, values, extra)?;
    let base = lib.render(t, &BTreeMap::new(), None).unwrap_or_default();
    let offs = utf16_offsets(&text);

    // 与默认渲染逐字比较，标出参数带来的变化；相隔很近的片段合并，避免碎片化
    let mut spans: Vec<(usize, usize)> = Vec::new();
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut config = similar::TextDiff::configure();
    // wasm32-unknown-unknown 没有系统时钟（Instant::now 会 panic），浏览器中不设置超时
    #[cfg(not(target_arch = "wasm32"))]
    config.timeout(Duration::from_millis(20));
    let diff = config.diff_chars(&base, &text);
    for op in diff.ops() {
        let r = op.new_range();
        if op.tag() != similar::DiffTag::Equal && !r.is_empty() {
            match spans.last_mut() {
                Some(last) if r.start <= last.1 + 2 => last.1 = r.end,
                _ => spans.push((r.start, r.end)),
            }
        }
    }
    let mut highlights = Vec::new();
    let extra_span = extra.filter(|e| !e.is_empty()).and_then(|e| {
        text.rfind(e).map(|b| {
            let s = text[..b].chars().count();
            (s, s + e.chars().count())
        })
    });
    for (s, e) in spans {
        // 附加文本单独标注
        let (s, e) = match extra_span {
            Some((xs, xe)) if s >= xs && e <= xe => continue,
            Some((xs, _)) if e > xs && s < xs => (s, xs),
            _ => (s, e),
        };
        if e > s {
            highlights.push(Highlight { kind: HighlightKind::Param, start: offs[s], end: offs[e] });
        }
    }
    if let Some((s, e)) = extra_span {
        highlights.push(Highlight { kind: HighlightKind::Extra, start: offs[s], end: offs[e] });
    }
    let chars = text.chars().count() as u32;
    let newlines = text.matches('\n').count() as u32;
    Ok(Preview {
        collapses_in_claude_code: chars > 800 || newlines > 2,
        lines: newlines + 1,
        chars,
        highlights,
        text,
    })
}

// MARK: - 补全

fn template_candidate(ctx: &Ctx, t: &Template, tier: MatchTier, cmd: &Word) -> Candidate {
    Candidate {
        kind: CandidateKind::Template,
        key: t.name.clone(),
        title: t.name.clone(),
        subtitle: t.aliases.join(" · "),
        detail: t.description.clone(),
        edit: Edit { start: ctx.u16(cmd.start), end: ctx.u16(cmd.end), text: format!("{} ", t.name) },
        tier: Some(tier),
        active: false,
        section: String::new(),
    }
}

fn param_completions(ctx: &Ctx, t: &Template, partial: Option<&Word>, parsed: &Parsed) -> (Vec<Candidate>, Option<String>) {
    let end = ctx.input.len();
    let (start, typed) = match partial {
        Some(w) => (w.start, w.norm.as_str()),
        None => (end, ""),
    };
    let edit = |text: String| Edit { start: ctx.u16(start), end: ctx.u16(end), text };

    // 正在输入取值：--name=val
    if let Some(body) = typed.strip_prefix("--")
        && let Some(eq) = body.find('=')
        && let Ok(p) = resolve_prefix(&body[..eq], &t.params, |p| vec![p.name.clone()])
    {
        let raw = &body[eq + 1..];
        let (done, cur) = match raw.rfind(',') {
            Some(i) if p.kind == ParamKind::Multi => (&raw[..=i], &raw[i + 1..]),
            _ => ("", raw),
        };
        let chosen: Vec<&str> = done.split(',').filter(|s| !s.is_empty()).collect();
        let options: Vec<(String, String, String)> = match p.kind {
            ParamKind::Bool => vec![("true".into(), tr!(ctx.loc, "开", "on"), String::new()), ("false".into(), tr!(ctx.loc, "关", "off"), String::new())],
            ParamKind::Enum | ParamKind::Multi => p
                .choices
                .iter()
                .filter(|c| !chosen.contains(&c.value.as_str()))
                .map(|c| (c.value.clone(), c.label.clone(), c.aliases.join(" · ")))
                .collect(),
            _ => vec![],
        };
        let cur_l = cur.to_lowercase();
        let cands: Vec<Candidate> = options
            .into_iter()
            .filter(|(v, _, _)| v.to_lowercase().starts_with(&cur_l))
            .map(|(v, label, aliases)| Candidate {
                kind: CandidateKind::Choice,
                key: v.clone(),
                title: v.clone(),
                subtitle: label,
                detail: aliases,
                edit: edit(format!("--{}={done}{v} ", p.name)),
                tier: None,
                active: false,
                section: String::new(),
            })
            .collect();
        let ghost = cands.first().map(|c| c.key.chars().skip(cur.chars().count()).collect::<String>()).filter(|g| !g.is_empty());
        return (cands, ghost);
    }

    // 正在输入参数名，或刚输完一个词等待下一个
    let name_q = typed.strip_prefix("--").unwrap_or(if typed.is_empty() { "" } else { "\u{0}" });
    let mut cands = Vec::new();
    if name_q != "\u{0}" {
        for p in &t.params {
            let default_on = p.effective_default() == Value::Bool(true);
            let flag = if p.kind == ParamKind::Bool && default_on { format!("no-{}", p.name) } else { p.name.clone() };
            if !flag.starts_with(name_q) && !p.name.starts_with(name_q) {
                continue;
            }
            let suffix = if p.kind == ParamKind::Bool { " " } else { "=" };
            let state = parsed.values.get(&p.name).map(|v| display(p, v, ctx.loc));
            cands.push(Candidate {
                kind: CandidateKind::Param,
                key: p.name.clone(),
                title: format!("--{flag}"),
                subtitle: p.short.map(|c| format!("-{c}")).unwrap_or_default(),
                detail: match state {
                    Some(s) => {
                        let d = &p.description;
                        tr!(ctx.loc, "{d}（当前：{s}）", "{d} (now: {s})")
                    }
                    None => p.description.clone(),
                },
                edit: edit(format!("--{flag}{suffix}")),
                tier: None,
                active: parsed.values.contains_key(&p.name),
                section: String::new(),
            });
        }
    }
    // 位置参数的候选值
    if let Some(p) = t.positional()
        && !typed.starts_with('-')
    {
        let q = typed.to_lowercase();
        for c in &p.choices {
            if c.value.to_lowercase().starts_with(&q) {
                let on = matches!(parsed.values.get(&p.name), Some(Value::List(l)) if l.contains(&c.value))
                    || parsed.values.get(&p.name) == Some(&Value::Str(c.value.clone()));
                cands.push(Candidate {
                    kind: CandidateKind::Choice,
                    key: c.value.clone(),
                    title: c.value.clone(),
                    subtitle: c.label.clone(),
                    detail: p.description.clone(),
                    edit: edit(format!("{} ", c.value)),
                    tier: None,
                    active: on,
                    section: String::new(),
                });
            }
        }
    }
    let ghost = if typed.is_empty() {
        None
    } else {
        cands.first().and_then(|c| {
            let full = c.edit.text.trim_end();
            full.strip_prefix(typed).map(str::to_string).filter(|g| !g.is_empty())
        })
    };
    (cands, ghost)
}

// MARK: - 入口

pub fn analyze(lib: &Library, usage: &Usage, input: &str, selected: Option<&str>) -> Analysis {
    if input.trim().is_empty() {
        return home(lib, usage, selected);
    }
    let loc = lib.locale;
    let mut ctx = Ctx { loc, input, tokens: vec![], diags: vec![] };
    let words = scan(input);
    let ends_with_ws = input.ends_with(is_ws);
    let empty = Analysis {
        tokens: vec![],
        phase: Phase::Command,
        template: None,
        locked: false,
        candidates: vec![],
        ghost: None,
        diagnostics: vec![],
        params: vec![],
        preview: None,
        canonical: None,
        enter: EnterAction::Blocked,
        hint: String::new(),
    };

    let cmd_word = words.first();
    let cmd_text = cmd_word.map(|w| w.norm.as_str()).unwrap_or("");
    let matches = match_templates(lib, usage, cmd_text);
    let locked = cmd_word.is_some() && (words.len() > 1 || ends_with_ws);

    // 当前模板：界面高亮的那一项（若仍在匹配结果中），否则最佳匹配
    let current = selected
        .and_then(|s| matches.iter().find(|(t, _)| t.name == s))
        .or(matches.first())
        .copied();

    let Some((t, tier)) = current else {
        if let Some(w) = cmd_word {
            ctx.token(TokenKind::CommandUnknown, w.start, w.end);
            let raw = w.raw;
            ctx.diag(Severity::Error, tr!(loc, "没有匹配 `{raw}` 的模板", "No template matches `{raw}`"), w.start, w.end, !locked, None);
        }
        return Analysis { tokens: ctx.tokens, diagnostics: ctx.diags, hint: tr!(loc, "没有匹配的模板", "No matching template"), ..empty };
    };

    if !locked {
        // 命令阶段：候选为模板
        let mut a = Analysis { template: Some(info(t)), ..empty };
        let dummy = Word { raw: "", start: 0, end: 0, norm: String::new() };
        let w = cmd_word.unwrap_or(&dummy);
        if cmd_word.is_some() {
            ctx.token(TokenKind::Command, w.start, w.end);
        }
        a.candidates = matches.iter().map(|(t, tier)| template_candidate(&ctx, t, *tier, w)).collect();
        a.ghost = if cmd_text.is_empty() {
            None
        } else {
            let n = t.name.to_lowercase();
            n.starts_with(&cmd_text.to_lowercase()).then(|| t.name.chars().skip(cmd_text.chars().count()).collect::<String>()).filter(|g| !g.is_empty())
        };
        a.params = param_states(t, &BTreeMap::new(), loc);
        a.preview = preview(lib, t, &BTreeMap::new(), None).ok();
        a.canonical = Some(t.name.clone());
        if tier == MatchTier::Fuzzy && !cmd_text.eq_ignore_ascii_case(&t.name) {
            a.enter = EnterAction::Complete { edit: Edit { start: ctx.u16(w.start), end: ctx.u16(w.end), text: t.name.clone() } };
            let name = &t.name;
            a.hint = tr!(loc, "模糊匹配：↩ 补全为 {name}", "Fuzzy match: ↩ completes to {name}");
        } else {
            a.enter = EnterAction::Insert;
        }
        a.tokens = ctx.tokens;
        return a;
    }

    // 命令已锁定：解析参数
    let w = cmd_word.unwrap();
    ctx.token(TokenKind::Command, w.start, w.end);
    let parsed = parse_params(t, &words[1..], ends_with_ws, &mut ctx);
    let (fill, missing) = validate(t, &parsed, &mut ctx);

    let extra_started = parsed.extra_start.is_some();
    let partial = (!ends_with_ws).then(|| words.last()).flatten().filter(|lw| lw.start > w.start);
    // 正在输入的词恰好是附加文本的第一个词时，它也可能是位置参数候选值的前缀
    let typing_first_extra = parsed.extra_start.is_some_and(|s| !ends_with_ws && partial.is_some_and(|p| p.start == s));
    let (candidates, ghost) = if extra_started && !typing_first_extra {
        (vec![], None)
    } else {
        param_completions(&ctx, t, partial, &parsed)
    };
    let in_extra = extra_started && (!typing_first_extra || candidates.is_empty());

    let errors = ctx.diags.iter().any(|d| d.severity == Severity::Error);
    let only_missing = errors && missing > 0 && ctx.diags.iter().filter(|d| d.severity == Severity::Error).count() == missing;
    let enter = if !errors {
        EnterAction::Insert
    } else if only_missing {
        EnterAction::Complete { edit: fill.unwrap() }
    } else {
        EnterAction::Blocked
    };
    let pv = preview(lib, t, &parsed.values, parsed.extra.as_deref());
    if let Err(e) = &pv {
        ctx.diags.push(Diagnostic { severity: Severity::Error, message: e.clone(), start: 0, end: 0, at_cursor: false, fix: None });
    }
    let hint = if tier == MatchTier::Fuzzy && !cmd_text.eq_ignore_ascii_case(&t.name) && !t.aliases.iter().any(|a| a.eq_ignore_ascii_case(cmd_text)) {
        let name = &t.name;
        tr!(loc, "已匹配为 {name}", "Matched {name}")
    } else {
        String::new()
    };
    let enter = if pv.is_err() { EnterAction::Blocked } else { enter };

    Analysis {
        tokens: ctx.tokens,
        phase: if in_extra { Phase::Extra } else { Phase::Params },
        template: Some(info(t)),
        locked: true,
        candidates,
        ghost,
        params: param_states(t, &parsed.values, loc),
        preview: pv.ok(),
        canonical: (!errors).then(|| canonical(t, &parsed.values, parsed.extra.as_deref())),
        enter,
        hint,
        diagnostics: ctx.diags,
    }
}

/// 解析并返回可插入的最终文本；输入有错误时返回 None。
pub fn render_final(lib: &Library, usage: &Usage, input: &str, selected: Option<&str>) -> Option<String> {
    let a = analyze(lib, usage, input, selected);
    match a.enter {
        EnterAction::Insert => a.preview.map(|p| p.text),
        _ => None,
    }
}

// MARK: - 首屏（docs/decisions.md D14）

/// 新用户没有历史时展示的示例
const EXAMPLES_ZH: &[&str] = &["r security 重点检查认证和权限", "zh -k", "commit --lang=zh", "explain beginner -d"];
const EXAMPLES_EN: &[&str] = &["r security focus on the auth module", "en technical", "commit", "explain beginner -d"];
const RECENT_LIMIT: usize = 3;
const FREQUENT_LIMIT: usize = 5;

fn invocation_candidate(lib: &Library, usage: &Usage, invocation: &str, section: &str) -> Option<Candidate> {
    let a = analyze(lib, usage, invocation, None);
    let t = a.template.as_ref()?;
    (a.enter == EnterAction::Insert).then(|| Candidate {
        kind: CandidateKind::Invocation,
        key: invocation.to_string(),
        title: invocation.to_string(),
        subtitle: String::new(),
        detail: t.description.clone(),
        edit: Edit { start: 0, end: 0, text: format!("{invocation} ") },
        tier: None,
        active: false,
        section: section.to_string(),
    })
}

fn home(lib: &Library, usage: &Usage, selected: Option<&str>) -> Analysis {
    let loc = lib.locale;
    let (s_recent, s_try, s_frequent, s_other) = match loc {
        Locale::Zh => ("最近施放", "试试看", "常用", "其他"),
        Locale::En => ("Recent", "Try these", "Frequent", "Other"),
    };
    let examples = if loc == Locale::Zh { EXAMPLES_ZH } else { EXAMPLES_EN };
    let mut cands: Vec<Candidate> = Vec::new();
    let recent: Vec<Candidate> = usage
        .recent(RECENT_LIMIT * 2)
        .iter()
        .filter_map(|e| invocation_candidate(lib, usage, &e.invocation, s_recent))
        .take(RECENT_LIMIT)
        .collect();
    if recent.is_empty() {
        cands.extend(examples.iter().filter_map(|e| invocation_candidate(lib, usage, e, s_try)));
    } else {
        cands.extend(recent);
    }

    let empty = Word { raw: "", start: 0, end: 0, norm: String::new() };
    let ctx = Ctx { loc, input: "", tokens: vec![], diags: vec![] };
    let mut frequent: Vec<&Template> = lib.visible().filter(|t| usage.score(&t.name) > 0.0).collect();
    frequent.sort_by(|a, b| usage.score(&b.name).total_cmp(&usage.score(&a.name)).then(a.name.cmp(&b.name)));
    frequent.truncate(FREQUENT_LIMIT);
    for t in &frequent {
        cands.push(Candidate { section: s_frequent.into(), ..template_candidate(&ctx, t, MatchTier::Prefix, &empty) });
    }
    let mut rest: Vec<&Template> = lib.visible().filter(|t| !frequent.iter().any(|f| f.name == t.name)).collect();
    // 保持模板库顺序，同分类相邻（分类按首次出现排序）
    let cat_rank = |c: &str| if c.is_empty() { usize::MAX } else { lib.visible().position(|t| t.category == c).unwrap_or(usize::MAX) };
    let lib_rank = |n: &str| lib.visible().position(|t| t.name == n).unwrap_or(usize::MAX);
    rest.sort_by_key(|t| (cat_rank(&t.category), lib_rank(&t.name)));
    for t in rest {
        let section = if t.category.is_empty() { s_other.to_string() } else { t.category.clone() };
        cands.push(Candidate { section, ..template_candidate(&ctx, t, MatchTier::Prefix, &empty) });
    }

    let current = selected.and_then(|k| cands.iter().find(|c| c.key == k)).or(cands.first()).cloned();
    let base = Analysis {
        tokens: vec![],
        phase: Phase::Command,
        template: None,
        locked: false,
        candidates: vec![],
        ghost: None,
        diagnostics: vec![],
        params: vec![],
        preview: None,
        canonical: None,
        enter: EnterAction::Blocked,
        hint: String::new(),
    };
    let mut a = match current {
        Some(c) if c.kind == CandidateKind::Invocation => {
            let inner = analyze(lib, usage, &c.key, None);
            Analysis { hint: tr!(loc, "↩ 重放这次施放", "↩ to cast it again"), ..inner }
        }
        Some(c) => match lib.get(&c.key) {
            Some(t) => Analysis {
                template: Some(info(t)),
                params: param_states(t, &BTreeMap::new(), loc),
                preview: preview(lib, t, &BTreeMap::new(), None).ok(),
                canonical: Some(t.name.clone()),
                enter: EnterAction::Insert,
                ..base
            },
            None => base,
        },
        None => base,
    };
    a.tokens = vec![];
    a.locked = false;
    a.phase = Phase::Command;
    a.ghost = None;
    a.diagnostics.clear();
    a.candidates = cands;
    a
}

// MARK: - 鼠标编辑参数（docs/decisions.md D16）

/// 解析出当前模板、命令词原文、参数值和附加文本。
fn parse_input<'a>(lib: &'a Library, usage: &Usage, input: &str, selected: Option<&str>) -> Option<(&'a Template, String, Parsed)> {
    if input.trim().is_empty() {
        // 首屏：以高亮的模板（或调用）为起点
        let a = home(lib, usage, selected);
        let key = selected.and_then(|k| a.candidates.iter().find(|c| c.key == k)).or(a.candidates.first())?.key.clone();
        if lib.get(&key).is_some() {
            return parse_input(lib, usage, &format!("{key} "), None);
        }
        return parse_input(lib, usage, &key, None);
    }
    let words = scan(input);
    let cmd = words.first()?;
    let matches = match_templates(lib, usage, &cmd.norm);
    let (t, _) = selected.and_then(|s| matches.iter().find(|(t, _)| t.name == s)).or(matches.first()).copied()?;
    let mut ctx = Ctx { loc: lib.locale, input, tokens: vec![], diags: vec![] };
    let parsed = parse_params(t, &words[1..], true, &mut ctx);
    Some((t, cmd.raw.to_string(), parsed))
}

fn render_value(p: &ParamSpec, v: &Value) -> String {
    match v {
        Value::Bool(true) => format!("--{}", p.name),
        Value::Bool(false) => format!("--no-{}", p.name),
        Value::Str(x) if p.positional => x.clone(),
        Value::List(l) if p.positional => l.join(" "),
        Value::Str(x) => format!("--{}={}", p.name, quote_if_needed(x)),
        Value::List(l) => format!("--{}={}", p.name, l.iter().map(|x| quote_if_needed(x)).collect::<Vec<_>>().join(",")),
        Value::Num(n) => format!("--{}={}", p.name, fmt_num(*n)),
    }
}

pub fn edit_param(lib: &Library, usage: &Usage, input: &str, selected: Option<&str>, name: &str, edit: ParamEdit) -> Option<EditResult> {
    let (t, cmd, mut parsed) = parse_input(lib, usage, input, selected)?;
    let p = t.param(name)?;
    let current = parsed.values.get(name).cloned().unwrap_or_else(|| p.effective_default());
    let mut begin = false;
    match edit {
        ParamEdit::Toggle if p.kind == ParamKind::Bool => {
            parsed.values.insert(p.name.clone(), Value::Bool(current != Value::Bool(true)));
        }
        ParamEdit::Set { value } => {
            let v = match p.kind {
                ParamKind::Enum => Value::Str(resolve_choice(p, &value, lib.locale).ok()?),
                ParamKind::Number => Value::Num(value.parse().ok()?),
                ParamKind::Bool => Value::Bool(parse_bool(&value)?),
                ParamKind::Multi => Value::List(vec![resolve_choice(p, &value, lib.locale).ok()?]),
                ParamKind::String | ParamKind::Text => Value::Str(value),
            };
            parsed.values.insert(p.name.clone(), v);
        }
        ParamEdit::ToggleChoice { value } if p.kind == ParamKind::Multi => {
            let v = resolve_choice(p, &value, lib.locale).ok()?;
            let mut list = match current {
                Value::List(l) => l,
                _ => vec![],
            };
            if let Some(i) = list.iter().position(|x| *x == v) {
                list.remove(i);
            } else {
                list.push(v);
            }
            // 保持 schema 中的候选顺序
            list.sort_by_key(|x| p.choices.iter().position(|c| c.value == *x));
            parsed.values.insert(p.name.clone(), Value::List(list));
        }
        ParamEdit::Clear => {
            parsed.values.remove(name);
        }
        ParamEdit::Begin => begin = true,
        _ => return None,
    }

    let mut text = cmd;
    let mut caret: Option<usize> = None;
    for q in &t.params {
        if begin && q.name == name {
            let v = match parsed.values.get(name) {
                Some(Value::Str(s)) => quote_if_needed(s),
                Some(Value::Num(n)) => fmt_num(*n),
                _ => String::new(),
            };
            text.push_str(&format!(" --{}=", q.name));
            text.push_str(if v == "\"\"" { "" } else { &v });
            caret = Some(text.len());
            continue;
        }
        let Some(v) = parsed.values.get(&q.name) else { continue };
        if *v == q.effective_default() {
            continue;
        }
        text.push(' ');
        text.push_str(&render_value(q, v));
    }
    match parsed.extra.as_deref().filter(|e| !e.is_empty()) {
        Some(extra) => {
            // 附加文本若可能被误解析为参数，则用 `--` 显式开始
            let first = extra.split_whitespace().next().unwrap_or("");
            let first_norm = normalize(first);
            let ambiguous = first_norm.starts_with('-') || t.positional().is_some_and(|p| p.find_choice(&first_norm).is_some());
            text.push_str(if ambiguous { " -- " } else { " " });
            text.push_str(extra);
        }
        None => text.push(' '),
    }
    let caret = caret.map(|b| text[..b].encode_utf16().count()).unwrap_or_else(|| text.encode_utf16().count()) as u32;
    Some(EditResult { text, caret })
}

/// 插入成功后记录使用：模板与不含附加文本的规范形式。
pub fn record_use(lib: &Library, usage: &mut Usage, input: &str, selected: Option<&str>, app: Option<&str>) {
    let Some((t, _, parsed)) = parse_input(lib, usage, input, selected) else { return };
    let invocation = canonical(t, &parsed.values, None);
    usage.record(&t.name, &invocation, app, crate::usage::now());
}

#[cfg(test)]
mod tests;
