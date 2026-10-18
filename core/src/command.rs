//! 面板指令模式（docs/decisions.md D31）：以 `/` 开头的输入管理用户模板。
//!
//! `/new 名称 [正文]`、`/edit 名称`、`/dup 名称 新名称`、`/alias 名称 别名`、`/rm 名称`。
//! 解析结果沿用模板命令的 `Analysis`（高亮、候选、灰字、诊断、预览），回车动作为 `EnterAction::Run`。
//! 指令只写用户模板目录；删除由平台外壳移到废纸篓，可以恢复。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::doc::{self, ParamDoc, TemplateDoc};
use crate::grammar::{
    param_states, Analysis, Candidate, CandidateKind, Diagnostic, Edit, EnterAction, MatchTier, Phase, Preview, Severity, TemplateInfo,
    Token, TokenKind,
};
use crate::i18n::Locale;
use crate::library::Library;
use crate::template::{self, Choice, ParamKind, Template};
use crate::tr;

/// 指令执行结果，由平台外壳展示并完成后续动作。
#[derive(Debug, Clone, PartialEq, uniffi::Record, serde::Serialize)]
pub struct CommandOutcome {
    /// 完成提示（HUD）
    pub message: String,
    /// 需要在模板库中打开的模板
    pub open_editor: Option<String>,
    /// 需要移到废纸篓的文件（`/rm`）
    pub trash_path: Option<String>,
    /// 由外壳执行的应用操作：`sync`、`settings`、`library`、`lang:auto|zh|en`（内置模板语言）
    pub action: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cmd {
    New,
    Edit,
    Dup,
    Alias,
    Rm,
    Sync,
    Settings,
    Library,
    Lang,
}

const COMMANDS: &[(Cmd, &str)] = &[
    (Cmd::New, "new"),
    (Cmd::Edit, "edit"),
    (Cmd::Dup, "dup"),
    (Cmd::Alias, "alias"),
    (Cmd::Rm, "rm"),
    (Cmd::Sync, "sync"),
    (Cmd::Settings, "settings"),
    (Cmd::Library, "library"),
    (Cmd::Lang, "lang"),
];

/// `/lang` 的取值：(值, 中文说明, 英文说明)
const LANGS: &[(&str, &str, &str)] = &[("auto", "跟随界面语言", "Same as the interface"), ("zh", "中文", "Chinese"), ("en", "English", "English")];

impl Cmd {
    fn usage(self, loc: Locale) -> String {
        match self {
            Cmd::New => tr!(loc, "/new 名称 [正文]", "/new name [text]"),
            Cmd::Edit => tr!(loc, "/edit 名称", "/edit name"),
            Cmd::Dup => tr!(loc, "/dup 名称 新名称", "/dup name new-name"),
            Cmd::Alias => tr!(loc, "/alias 名称 别名", "/alias name alias"),
            Cmd::Rm => tr!(loc, "/rm 名称", "/rm name"),
            Cmd::Sync => "/sync".into(),
            Cmd::Settings => "/settings".into(),
            Cmd::Library => tr!(loc, "/library [名称]", "/library [name]"),
            Cmd::Lang => "/lang auto|zh|en".into(),
        }
    }

    fn summary(self, loc: Locale) -> String {
        match self {
            Cmd::New => tr!(loc, "新建模板，正文可用 {{参数}} 和 {{参数:a|b}}", "New template; use {{param}} and {{param:a|b}} in the text"),
            Cmd::Edit => tr!(loc, "在模板库中编辑", "Edit in the template library"),
            Cmd::Dup => tr!(loc, "复制一个模板（可改写内置模板）", "Copy a template (e.g. to change a built-in one)"),
            Cmd::Alias => tr!(loc, "给模板添加别名", "Add an alias to a template"),
            Cmd::Rm => {
                let (zh, en) = trash_word();
                tr!(loc, "删除模板（移到{zh}）", "Delete a template (moves it to the {en})")
            }
            Cmd::Sync => tr!(loc, "立即同步", "Sync now"),
            Cmd::Settings => tr!(loc, "打开设置", "Open settings"),
            Cmd::Library => tr!(loc, "打开模板库", "Open the template library"),
            Cmd::Lang => tr!(loc, "切换内置模板的语言", "Switch the built-in templates' language"),
        }
    }
}

/// 系统回收站的叫法（中文, 英文）
fn trash_word() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") { ("回收站", "Recycle Bin") } else { ("废纸篓", "Trash") }
}

/// 输入是否为指令（允许全角斜杠）。
pub fn is_command(input: &str) -> bool {
    let t = input.trim_start();
    t.starts_with('/') || t.starts_with('／')
}

struct Word<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

fn words(input: &str) -> Vec<Word<'_>> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in input.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push(Word { text: &input[s..i], start: s, end: i });
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(Word { text: &input[s..], start: s, end: input.len() });
    }
    out
}

fn u16(input: &str, byte: usize) -> u32 {
    input[..byte].encode_utf16().count() as u32
}

// MARK: - 占位符 → 模板

/// 把带占位符的正文变成模板：`{名}` → 文本参数，`{名:a|b}` → 单选参数（第一个为默认，首个单选为位置参数）。
/// 其余文字原样保留；含 MiniJinja 语法字符的片段用 raw 块包住，避免被当作模板语法。
pub fn build_template(name: &str, body: &str, loc: Locale) -> Result<TemplateDoc, String> {
    let source_body = body;
    let mut params: Vec<ParamDoc> = Vec::new();
    let mut out = String::new();
    let mut literal = String::new();
    let flush = |literal: &mut String, out: &mut String| {
        if literal.contains("{{") || literal.contains("{%") || literal.contains("{#") {
            out.push_str("{% raw %}");
            out.push_str(literal);
            out.push_str("{% endraw %}");
        } else {
            out.push_str(literal);
        }
        literal.clear();
    };
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < body.len() {
        let c = body[i..].chars().next().unwrap();
        if c == '{'
            && bytes.get(i + 1) != Some(&b'{')
            && (i == 0 || bytes[i - 1] != b'{')
            && let Some(close) = body[i + 1..].find('}')
            && let Some((pname, choices)) = parse_placeholder(&body[i + 1..i + 1 + close])
        {
            if !template::valid_name(pname) || pname.starts_with("no-") {
                return Err(tr!(loc, "参数名 `{pname}` 只能用英文字母、数字、- 和 _，并以字母开头", "Parameter name `{pname}` may only use letters, digits, - and _, and must start with a letter"));
            }
            let kind = if choices.is_empty() { ParamKind::String } else { ParamKind::Enum };
            match params.iter().find(|p| p.name == pname) {
                Some(p) if p.kind != kind || (!choices.is_empty() && p.choices.iter().map(|c| c.value.as_str()).ne(choices.iter().copied())) => {
                    return Err(tr!(loc, "参数 `{pname}` 的写法前后不一致", "Parameter `{pname}` is written differently in two places"));
                }
                Some(_) => {}
                None => {
                    let positional = kind == ParamKind::Enum && !params.iter().any(|p| p.positional);
                    params.push(ParamDoc {
                        name: pname.to_string(),
                        kind,
                        description: String::new(),
                        choices: choices.iter().map(|v| Choice { value: v.to_string(), label: v.to_string(), aliases: vec![] }).collect(),
                        default_value: choices.first().map(|v| v.to_string()),
                        positional,
                        short: None,
                        required: false,
                        requires: vec![],
                        conflicts: vec![],
                        min: None,
                        max: None,
                    });
                }
            }
            flush(&mut literal, &mut out);
            out.push_str(&format!("{{{{ {} }}}}", pname.replace('-', "_")));
            i += close + 2;
            continue;
        }
        literal.push(c);
        i += c.len_utf8();
    }
    flush(&mut literal, &mut out);
    let mut body = out.trim_end().to_string();
    body.push_str("\n{% include \"_extra\" %}\n");
    let first_line = literal_description(&placeholder_names(source_body));
    Ok(TemplateDoc { name: name.to_string(), aliases: vec![], description: first_line, category: String::new(), params, body })
}

/// `{名}` 或 `{名:a|b|c}`；不是占位符时返回 None（原样保留）。
fn parse_placeholder(inner: &str) -> Option<(&str, Vec<&str>)> {
    let (name, rest) = match inner.split_once(':') {
        Some((n, r)) => (n.trim(), Some(r)),
        None => (inner.trim(), None),
    };
    if name.is_empty() || name.contains(char::is_whitespace) || !name.chars().next()?.is_ascii_alphabetic() {
        return None;
    }
    let choices: Vec<&str> = match rest {
        None => vec![],
        Some(r) => {
            let list: Vec<&str> = r.split('|').map(str::trim).filter(|s| !s.is_empty()).collect();
            if list.is_empty() {
                return None;
            }
            let mut uniq = list.clone();
            uniq.dedup();
            if uniq.len() != list.len() {
                return None;
            }
            list
        }
    };
    Some((name, choices))
}

/// 描述用：`{名:a|b}` 显示为 `{名}`
fn placeholder_names(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').and_then(|close| parse_placeholder(&after[..close]).map(|(n, _)| (close, n))) {
            Some((close, n)) => {
                out.push_str(&format!("{{{n}}}"));
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 描述取正文第一行（去掉模板语法），最多 40 个字符。
fn literal_description(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("{%")).unwrap_or("");
    let clean: String = line.replace("{% raw %}", "").replace("{% endraw %}", "");
    let mut s: String = clean.chars().take(40).collect();
    if clean.chars().count() > 40 {
        s.push('…');
    }
    s
}

// MARK: - 解析

struct Ctx<'a> {
    input: &'a str,
    tokens: Vec<Token>,
    diags: Vec<Diagnostic>,
}

impl Ctx<'_> {
    fn token(&mut self, kind: TokenKind, start: usize, end: usize) {
        let (s, e) = (u16(self.input, start), u16(self.input, end));
        self.tokens.push(Token { kind, start: s, end: e });
    }
    fn error(&mut self, msg: String, start: usize, end: usize, at_cursor: bool) {
        let (s, e) = (u16(self.input, start), u16(self.input, end));
        self.diags.push(Diagnostic { severity: Severity::Error, message: msg, start: s, end: e, at_cursor, fix: None });
    }
    fn warn(&mut self, msg: String, start: usize, end: usize) {
        let (s, e) = (u16(self.input, start), u16(self.input, end));
        self.diags.push(Diagnostic { severity: Severity::Warning, message: msg, start: s, end: e, at_cursor: false, fix: None });
    }
}

/// 解析后的指令：要执行的动作（没有错误时）。
enum Plan {
    Write { path: PathBuf, source: String, message: String, open: Option<String> },
    Open { name: String },
    Trash { path: PathBuf, name: String },
    Action { action: String, message: String },
}

struct Outcome {
    analysis: Analysis,
    plan: Option<Plan>,
}

fn find_template<'a>(lib: &'a Library, name: &str) -> Option<&'a Template> {
    lib.visible().find(|t| t.name == name || t.aliases.iter().any(|a| a == name))
}

fn name_taken(lib: &Library, name: &str) -> bool {
    lib.templates.iter().any(|t| t.name == name || t.aliases.iter().any(|a| a == name))
}

fn template_info(t: &Template) -> TemplateInfo {
    TemplateInfo { name: t.name.clone(), aliases: t.aliases.clone(), description: t.description.clone(), category: t.category.clone(), builtin: t.builtin }
}

fn plain_preview(text: &str) -> Preview {
    Preview {
        text: text.to_string(),
        highlights: vec![],
        chars: text.chars().count() as u32,
        lines: text.lines().count().max(1) as u32,
        collapses_in_claude_code: false,
    }
}

fn empty_analysis() -> Analysis {
    Analysis {
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
    }
}

fn parse(lib: &Library, user_dir: Option<&Path>, input: &str, clipboard: Option<&str>) -> Outcome {
    let loc = lib.locale;
    let mut ctx = Ctx { input, tokens: vec![], diags: vec![] };
    let mut a = empty_analysis();
    let ws = words(input);
    let ends_ws = input.ends_with(char::is_whitespace);
    let Some(first) = ws.first() else { return Outcome { analysis: a, plan: None } };
    let slash = first.text.chars().next().map(char::len_utf8).unwrap_or(1);
    let typed = &first.text[slash..];

    // 指令名
    let cmd = COMMANDS.iter().find(|(_, n)| *n == typed).map(|(c, _)| *c);
    let completing_cmd = ws.len() == 1 && !ends_ws;
    if completing_cmd || cmd.is_none() {
        let typed_lower = typed.to_lowercase();
        for (c, n) in COMMANDS.iter().filter(|(_, n)| n.starts_with(&typed_lower)) {
            a.candidates.push(Candidate {
                kind: CandidateKind::Template,
                key: format!("/{n}"),
                title: format!("/{n}"),
                subtitle: c.summary(loc),
                detail: c.usage(loc),
                edit: Edit { start: u16(input, first.start), end: u16(input, first.end), text: format!("/{n} ") },
                tier: Some(MatchTier::Prefix),
                active: false,
                section: String::new(),
            });
        }
        if let Some(c) = a.candidates.first()
            && completing_cmd
            && c.title.len() > first.text.len()
            && c.title.starts_with(first.text)
        {
            a.ghost = Some(format!("{} ", &c.title[first.text.len()..]));
        }
    }
    // 还没有确定指令：右侧显示用法说明
    if cmd.is_none() || completing_cmd {
        let help: Vec<String> = COMMANDS.iter().map(|(c, _)| format!("{}\n    {}", c.usage(loc), c.summary(loc))).collect();
        a.preview = Some(plain_preview(&help.join("\n\n")));
    }
    let Some(cmd) = cmd else {
        ctx.token(TokenKind::CommandUnknown, first.start, first.end);
        if a.candidates.is_empty() {
            let w = first.text;
            let all = COMMANDS.iter().map(|(_, n)| format!("/{n}")).collect::<Vec<_>>().join(" ");
            ctx.error(tr!(loc, "未知指令 {w}，可用：{all}", "Unknown command {w}. Available: {all}"), first.start, first.end, !ends_ws);
        }
        a.hint = tr!(loc, "指令模式", "Command mode");
        return finish(ctx, a, None);
    };
    ctx.token(TokenKind::Command, first.start, first.end);
    a.hint = cmd.usage(loc);
    a.locked = true;

    let arg = |i: usize| ws.get(i);
    // 正在输入第 i 个参数（模板名）时给出模板候选
    let suggest_templates = |a: &mut Analysis, idx: usize, user_only: bool| {
        let current = arg(idx).filter(|_| ws.len() == idx + 1 && !ends_ws);
        let at_new = ws.len() == idx && ends_ws;
        if current.is_none() && !at_new {
            return;
        }
        let prefix = current.map(|w| w.text.to_lowercase()).unwrap_or_default();
        let (start, end) = current.map(|w| (w.start, w.end)).unwrap_or((input.len(), input.len()));
        for t in lib.visible().filter(|t| !user_only || !t.builtin).filter(|t| t.name.starts_with(&prefix)) {
            a.candidates.push(Candidate {
                kind: CandidateKind::Template,
                key: t.name.clone(),
                title: t.name.clone(),
                subtitle: t.description.clone(),
                detail: t.description.clone(),
                edit: Edit { start: u16(input, start), end: u16(input, end), text: format!("{} ", t.name) },
                tier: Some(MatchTier::Prefix),
                active: false,
                section: String::new(),
            });
        }
        if let (Some(w), Some(c)) = (current, a.candidates.first())
            && c.title.starts_with(w.text)
            && c.title.len() > w.text.len()
        {
            a.ghost = Some(c.title[w.text.len()..].to_string());
        }
    };

    let plan = match cmd {
        Cmd::New => {
            let Some(name_w) = arg(1) else {
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::Positional, name_w.start, name_w.end);
            let name = name_w.text;
            let at_cursor = ws.len() == 2 && !ends_ws;
            if !template::valid_name(name) {
                ctx.error(tr!(loc, "模板名只能用英文字母、数字、- 和 _，并以字母开头", "Template names may only use letters, digits, - and _, and must start with a letter"), name_w.start, name_w.end, at_cursor);
                return finish(ctx, a, None);
            }
            if lib.templates.iter().any(|t| !t.builtin && t.name == name) {
                ctx.error(tr!(loc, "已有模板 `{name}`，用 /edit {name} 修改", "`{name}` already exists. Use /edit {name}"), name_w.start, name_w.end, at_cursor);
                return finish(ctx, a, None);
            }
            if lib.templates.iter().any(|t| t.builtin && t.name == name) {
                ctx.warn(tr!(loc, "将覆盖内置模板 `{name}`", "This will replace the built-in `{name}`"), name_w.start, name_w.end);
            } else if lib.templates.iter().any(|t| t.aliases.iter().any(|al| al == name)) {
                ctx.error(tr!(loc, "`{name}` 已是其他模板的别名", "`{name}` is already another template's alias"), name_w.start, name_w.end, at_cursor);
                return finish(ctx, a, None);
            }
            let typed_body = input[name_w.end..].trim();
            let from_clipboard = typed_body.is_empty();
            let body = if from_clipboard { clipboard.map(str::trim).unwrap_or("") } else { typed_body };
            if !from_clipboard {
                let body_start = input.len() - input[name_w.end..].trim_start().len();
                ctx.token(TokenKind::Extra, body_start, input.trim_end().len());
                // 占位符高亮
                let mut i = body_start;
                while let Some(open) = input[i..].find('{') {
                    let s = i + open;
                    let Some(close) = input[s..].find('}') else { break };
                    if parse_placeholder(&input[s + 1..s + close]).is_some() {
                        ctx.token(TokenKind::OptionName, s, s + close + 1);
                    }
                    i = s + close + 1;
                }
            }
            if body.is_empty() {
                ctx.error(tr!(loc, "在名称后输入正文，或先复制要保存的文字", "Type the text after the name, or copy the text you want to save first"), name_w.start, name_w.end, false);
                a.hint = tr!(loc, "正文为空", "No text yet");
                return finish(ctx, a, None);
            }
            match build_template(name, body, loc).map(|d| doc::serialize_doc(&d)) {
                Err(e) => {
                    ctx.error(e, name_w.start, input.len(), false);
                    None
                }
                Ok(source) => match template::parse_in(&source, false, loc) {
                    Err(e) => {
                        ctx.error(e, name_w.start, input.len(), false);
                        None
                    }
                    Ok(t) => {
                        a.template = Some(template_info(&t));
                        a.params = param_states(&t, &BTreeMap::new(), loc);
                        a.preview = Some(plain_preview(&source));
                        a.hint = if from_clipboard {
                            tr!(loc, "↩ 用剪贴板内容创建 {name}", "↩ create {name} from the clipboard")
                        } else {
                            tr!(loc, "↩ 创建 {name}", "↩ create {name}")
                        };
                        // 没有用户目录（官网演示）时只预览
                        user_dir.map(|dir| Plan::Write {
                            path: dir.join(format!("{name}.md")),
                            source,
                            message: tr!(loc, "已创建 {name}", "Created {name}"),
                            open: None,
                        })
                    }
                },
            }
        }
        Cmd::Edit => {
            suggest_templates(&mut a, 1, false);
            let Some(w) = arg(1) else { return finish(ctx, a, None) };
            ctx.token(TokenKind::Positional, w.start, w.end);
            match find_template(lib, w.text) {
                None => {
                    let n = w.text;
                    ctx.error(tr!(loc, "没有模板 `{n}`", "No template `{n}`"), w.start, w.end, ws.len() == 2 && !ends_ws);
                    None
                }
                Some(t) => {
                    let n = t.name.clone();
                    a.template = Some(template_info(t));
                    a.preview = Some(plain_preview(&t.source));
                    a.hint = tr!(loc, "↩ 在模板库中打开 {n}", "↩ open {n} in the library");
                    Some(Plan::Open { name: n })
                }
            }
        }
        Cmd::Dup => {
            suggest_templates(&mut a, 1, false);
            let Some(w) = arg(1) else { return finish(ctx, a, None) };
            ctx.token(TokenKind::Positional, w.start, w.end);
            let Some(src) = find_template(lib, w.text) else {
                let n = w.text;
                ctx.error(tr!(loc, "没有模板 `{n}`", "No template `{n}`"), w.start, w.end, ws.len() == 2 && !ends_ws);
                return finish(ctx, a, None);
            };
            a.template = Some(template_info(src));
            let Some(nw) = arg(2) else {
                a.hint = tr!(loc, "输入新名称", "Type a new name");
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::OptionValue, nw.start, nw.end);
            let new = nw.text;
            let at_cursor = ws.len() == 3 && !ends_ws;
            if !template::valid_name(new) {
                ctx.error(tr!(loc, "模板名只能用英文字母、数字、- 和 _，并以字母开头", "Template names may only use letters, digits, - and _, and must start with a letter"), nw.start, nw.end, at_cursor);
                None
            } else if lib.templates.iter().any(|t| !t.builtin && t.name == new) || lib.templates.iter().any(|t| t.aliases.iter().any(|al| al == new)) {
                ctx.error(tr!(loc, "`{new}` 已被使用", "`{new}` is already taken"), nw.start, nw.end, at_cursor);
                None
            } else {
                let mut d = doc::from_template(src);
                d.name = new.to_string();
                d.aliases.clear();
                let source = doc::serialize_doc(&d);
                a.preview = Some(plain_preview(&source));
                let from = src.name.clone();
                a.hint = tr!(loc, "↩ 复制 {from} 为 {new}", "↩ copy {from} as {new}");
                user_dir.map(|dir| Plan::Write {
                    path: dir.join(format!("{new}.md")),
                    source,
                    message: tr!(loc, "已创建 {new}", "Created {new}"),
                    open: Some(new.to_string()),
                })
            }
        }
        Cmd::Alias => {
            suggest_templates(&mut a, 1, true);
            let Some(w) = arg(1) else { return finish(ctx, a, None) };
            ctx.token(TokenKind::Positional, w.start, w.end);
            let Some(t) = find_template(lib, w.text) else {
                let n = w.text;
                ctx.error(tr!(loc, "没有模板 `{n}`", "No template `{n}`"), w.start, w.end, ws.len() == 2 && !ends_ws);
                return finish(ctx, a, None);
            };
            a.template = Some(template_info(t));
            if t.builtin {
                let n = &t.name;
                ctx.error(tr!(loc, "内置模板不能直接修改，先用 /dup {n} 新名称 复制一份", "Built-in templates can't be changed. Copy it first with /dup {n} new-name"), w.start, w.end, false);
                return finish(ctx, a, None);
            }
            let Some(aw) = arg(2) else {
                a.hint = tr!(loc, "输入别名", "Type an alias");
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::OptionValue, aw.start, aw.end);
            let alias = aw.text;
            let at_cursor = ws.len() == 3 && !ends_ws;
            if alias.starts_with('-') || alias.chars().any(char::is_control) {
                ctx.error(tr!(loc, "别名不能以 - 开头", "Aliases can't start with -"), aw.start, aw.end, at_cursor);
                None
            } else if name_taken(lib, alias) {
                ctx.error(tr!(loc, "`{alias}` 已被使用", "`{alias}` is already taken"), aw.start, aw.end, at_cursor);
                None
            } else if let Some(path) = &t.path {
                let mut d = doc::from_template(t);
                d.aliases.push(alias.to_string());
                let source = doc::serialize_doc(&d);
                a.preview = Some(plain_preview(&source));
                let n = t.name.clone();
                a.hint = tr!(loc, "↩ 给 {n} 添加别名 {alias}", "↩ add alias {alias} to {n}");
                Some(Plan::Write { path: path.clone(), source, message: tr!(loc, "{n} 的别名：{alias}", "{n} now also answers to {alias}"), open: None })
            } else {
                None
            }
        }
        Cmd::Rm => {
            suggest_templates(&mut a, 1, true);
            let Some(w) = arg(1) else { return finish(ctx, a, None) };
            ctx.token(TokenKind::Positional, w.start, w.end);
            match find_template(lib, w.text) {
                None => {
                    let n = w.text;
                    ctx.error(tr!(loc, "没有模板 `{n}`", "No template `{n}`"), w.start, w.end, ws.len() == 2 && !ends_ws);
                    None
                }
                Some(t) if t.builtin => {
                    ctx.error(tr!(loc, "内置模板不能删除", "Built-in templates can't be deleted"), w.start, w.end, false);
                    a.template = Some(template_info(t));
                    None
                }
                Some(t) => {
                    let n = t.name.clone();
                    a.template = Some(template_info(t));
                    a.preview = Some(plain_preview(&t.source));
                    let (zh, en) = trash_word();
                    a.hint = tr!(loc, "↩ 删除 {n}（移到{zh}）", "↩ delete {n} (moves it to the {en})");
                    t.path.clone().map(|path| Plan::Trash { path, name: n })
                }
            }
        }
        Cmd::Sync | Cmd::Settings => {
            if let Some(w) = arg(1) {
                ctx.error(tr!(loc, "这个指令不需要参数", "This command takes no arguments"), w.start, input.len(), false);
                return finish(ctx, a, None);
            }
            let (action, hint) = match cmd {
                Cmd::Sync => ("sync", tr!(loc, "↩ 立即同步", "↩ sync now")),
                _ => ("settings", tr!(loc, "↩ 打开设置", "↩ open settings")),
            };
            a.hint = hint;
            Some(Plan::Action { action: action.into(), message: String::new() })
        }
        Cmd::Library => {
            suggest_templates(&mut a, 1, false);
            match arg(1) {
                None => {
                    a.hint = tr!(loc, "↩ 打开模板库", "↩ open the template library");
                    Some(Plan::Action { action: "library".into(), message: String::new() })
                }
                Some(w) => {
                    ctx.token(TokenKind::Positional, w.start, w.end);
                    match find_template(lib, w.text) {
                        None => {
                            let n = w.text;
                            ctx.error(tr!(loc, "没有模板 `{n}`", "No template `{n}`"), w.start, w.end, ws.len() == 2 && !ends_ws);
                            None
                        }
                        Some(t) => {
                            let n = t.name.clone();
                            a.template = Some(template_info(t));
                            a.preview = Some(plain_preview(&t.source));
                            a.hint = tr!(loc, "↩ 在模板库中打开 {n}", "↩ open {n} in the library");
                            Some(Plan::Open { name: n })
                        }
                    }
                }
            }
        }
        Cmd::Lang => {
            let current = arg(1).filter(|_| ws.len() == 2 && !ends_ws);
            if current.is_some() || (ws.len() == 1 && ends_ws) {
                let prefix = current.map(|w| w.text.to_lowercase()).unwrap_or_default();
                let (start, end) = current.map(|w| (w.start, w.end)).unwrap_or((input.len(), input.len()));
                for (v, zh, en) in LANGS.iter().filter(|(v, _, _)| v.starts_with(&prefix)) {
                    a.candidates.push(Candidate {
                        kind: CandidateKind::Choice,
                        key: v.to_string(),
                        title: v.to_string(),
                        subtitle: if loc == Locale::Zh { zh.to_string() } else { en.to_string() },
                        detail: String::new(),
                        edit: Edit { start: u16(input, start), end: u16(input, end), text: v.to_string() },
                        tier: Some(MatchTier::Prefix),
                        active: false,
                        section: String::new(),
                    });
                }
                if let (Some(w), Some(c)) = (current, a.candidates.first())
                    && c.title.len() > w.text.len()
                {
                    a.ghost = Some(c.title[w.text.len()..].to_string());
                }
            }
            let Some(w) = arg(1) else {
                a.hint = tr!(loc, "选择 auto、zh 或 en", "Choose auto, zh or en");
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::OptionValue, w.start, w.end);
            match LANGS.iter().find(|(v, _, _)| *v == w.text.to_lowercase()) {
                None => {
                    ctx.error(tr!(loc, "可选：auto、zh、en", "Options: auto, zh, en"), w.start, w.end, ws.len() == 2 && !ends_ws);
                    None
                }
                Some((v, zh, en)) => {
                    let label = if loc == Locale::Zh { *zh } else { *en };
                    a.hint = tr!(loc, "↩ 内置模板改用：{label}", "↩ built-in templates in: {label}");
                    Some(Plan::Action { action: format!("lang:{v}"), message: tr!(loc, "内置模板语言：{label}", "Built-in templates: {label}") })
                }
            }
        }
    };
    finish(ctx, a, plan)
}

fn finish(ctx: Ctx, mut a: Analysis, plan: Option<Plan>) -> Outcome {
    let errors = ctx.diags.iter().any(|d| d.severity == Severity::Error);
    let hard_errors = ctx.diags.iter().any(|d| d.severity == Severity::Error && !d.at_cursor);
    a.tokens = ctx.tokens;
    a.diagnostics = ctx.diags;
    let plan = plan.filter(|_| !errors);
    a.enter = match &plan {
        Some(Plan::Trash { .. }) => EnterAction::Run { confirm: true },
        Some(_) => EnterAction::Run { confirm: false },
        // 正在输入指令名：回车补全为第一个候选
        None => match a.candidates.first() {
            // 正在输入的词上的错误不妨碍补全
            Some(c) if !hard_errors => EnterAction::Complete { edit: c.edit.clone() },
            _ => EnterAction::Blocked,
        },
    };
    Outcome { analysis: a, plan }
}

/// 解析指令输入（面板实时调用）。`clipboard`：`/new 名称` 不写正文时使用的文字。
pub fn analyze(lib: &Library, user_dir: Option<&Path>, input: &str, clipboard: Option<&str>) -> Analysis {
    parse(lib, user_dir, input, clipboard).analysis
}

/// 执行指令。写入用户模板目录；`/rm` 只返回要移到废纸篓的路径，由外壳完成。
pub fn run(lib: &Library, user_dir: Option<&Path>, input: &str, clipboard: Option<&str>) -> Result<CommandOutcome, String> {
    let loc = lib.locale;
    let out = parse(lib, user_dir, input, clipboard);
    let Some(plan) = out.plan else {
        let msg = out.analysis.diagnostics.iter().find(|d| d.severity == Severity::Error).map(|d| d.message.clone());
        return Err(msg.unwrap_or_else(|| tr!(loc, "指令不完整", "The command is incomplete")));
    };
    match plan {
        Plan::Write { path, source, message, open } => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let tmp = path.with_extension("md.tmp");
            std::fs::write(&tmp, &source).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| e.to_string())?;
            Ok(CommandOutcome { message, open_editor: open, trash_path: None, action: None })
        }
        Plan::Open { name } => Ok(CommandOutcome { message: String::new(), open_editor: Some(name), trash_path: None, action: None }),
        Plan::Action { action, message } => Ok(CommandOutcome { message, open_editor: None, trash_path: None, action: Some(action) }),
        Plan::Trash { path, name } => Ok(CommandOutcome {
            message: {
                let (zh, en) = trash_word();
                tr!(loc, "已删除 {name}，可在{zh}中找回", "Deleted {name}. You can restore it from the {en}")
            },
            open_editor: None,
            trash_path: Some(path.to_string_lossy().to_string()),
            action: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("qantrip-cmd-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rand_suffix() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
    }

    #[test]
    fn placeholders_become_params() {
        let d = build_template("tr", "把下面的内容翻译成{lang:英文|日文}，语气{tone:正式|口语}。关注 {topic}", Locale::Zh).unwrap();
        assert_eq!(d.params.len(), 3);
        assert_eq!(d.params[0].kind, ParamKind::Enum);
        assert!(d.params[0].positional && !d.params[1].positional);
        assert_eq!(d.params[0].default_value.as_deref(), Some("英文"));
        assert_eq!(d.params[2].kind, ParamKind::String);
        assert!(d.body.contains("{{ lang }}") && d.body.contains("{{ topic }}"));
        assert_eq!(d.description, "把下面的内容翻译成{lang}，语气{tone}。关注 {topic}");
        let source = doc::serialize_doc(&d);
        let t = template::parse(&source, false).unwrap();
        let lib = Library::with_override(None, &source).0;
        let mut vals = BTreeMap::new();
        vals.insert("lang".to_string(), crate::template::Value::Str("日文".into()));
        let out = lib.render(lib.get(&t.name).unwrap(), &vals, None).unwrap();
        assert!(out.starts_with("把下面的内容翻译成日文，语气正式。"), "{out}");
    }

    #[test]
    fn literal_jinja_is_kept_verbatim() {
        let d = build_template("t", "保留 {{ x }} 和 {% if %} 原样；JSON {\"a\": 1}", Locale::Zh).unwrap();
        let source = doc::serialize_doc(&d);
        let lib = Library::with_override(None, &source).0;
        let out = lib.render(lib.get("t").unwrap(), &BTreeMap::new(), None).unwrap();
        assert_eq!(out, "保留 {{ x }} 和 {% if %} 原样；JSON {\"a\": 1}");
    }

    #[test]
    fn new_from_text_and_clipboard() {
        let dir = tmp();
        let lib = Library::load(Some(&dir));
        let a = analyze(&lib, Some(&dir), "/new su 总结我昨天做了什么", None);
        assert_eq!(a.enter, EnterAction::Run { confirm: false }, "{:?}", a.diagnostics);
        assert!(a.preview.unwrap().text.contains("总结我昨天做了什么"));
        let a = analyze(&lib, Some(&dir), "/new su", None);
        assert_eq!(a.enter, EnterAction::Blocked);
        let a = analyze(&lib, Some(&dir), "/new su", Some("剪贴板里的 Prompt"));
        assert_eq!(a.enter, EnterAction::Run { confirm: false });
        run(&lib, Some(&dir), "/new su 总结 {style:简短|详细}", None).unwrap();
        let lib = Library::load(Some(&dir));
        assert!(lib.problems.is_empty(), "{:?}", lib.problems);
        assert!(lib.get("su").is_some());
        // 再次创建同名：报错
        let a = analyze(&lib, Some(&dir), "/new su x", None);
        assert_eq!(a.enter, EnterAction::Blocked);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dup_alias_rm() {
        let dir = tmp();
        let lib = Library::load(Some(&dir));
        let out = run(&lib, Some(&dir), "/dup review my-review", None).unwrap();
        assert_eq!(out.open_editor.as_deref(), Some("my-review"));
        let lib = Library::load(Some(&dir));
        assert!(lib.problems.is_empty(), "{:?}", lib.problems);
        // 内置模板不能加别名、不能删
        assert_eq!(analyze(&lib, Some(&dir), "/alias review rv", None).enter, EnterAction::Blocked);
        assert_eq!(analyze(&lib, Some(&dir), "/rm review", None).enter, EnterAction::Blocked);
        // 已被占用的别名
        assert_eq!(analyze(&lib, Some(&dir), "/alias my-review r", None).enter, EnterAction::Blocked);
        run(&lib, Some(&dir), "/alias my-review mr", None).unwrap();
        let lib = Library::load(Some(&dir));
        assert_eq!(lib.get("my-review").unwrap().aliases, vec!["mr".to_string()]);
        let a = analyze(&lib, Some(&dir), "/rm mr", None);
        assert_eq!(a.enter, EnterAction::Run { confirm: true });
        let out = run(&lib, Some(&dir), "/rm my-review", None).unwrap();
        assert!(out.trash_path.unwrap().ends_with("my-review.md"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn app_actions() {
        let lib = Library::load(None);
        let act = |input: &str| run(&lib, None, input, None).map(|o| o.action);
        assert_eq!(act("/sync").unwrap().as_deref(), Some("sync"));
        assert_eq!(act("/settings").unwrap().as_deref(), Some("settings"));
        assert_eq!(act("/library").unwrap().as_deref(), Some("library"));
        assert_eq!(run(&lib, None, "/library review", None).unwrap().open_editor.as_deref(), Some("review"));
        assert_eq!(act("/lang EN").unwrap().as_deref(), Some("lang:en"));
        assert!(act("/lang fr").is_err());
        assert!(act("/sync now").is_err());
        let a = analyze(&lib, None, "/lang ", None);
        assert_eq!(a.candidates.len(), 3);
        assert_eq!(analyze(&lib, None, "/lang z", None).ghost.as_deref(), Some("h"));
        assert!(matches!(analyze(&lib, None, "/lang z", None).enter, EnterAction::Complete { .. }));
        assert_eq!(analyze(&lib, None, "/s", None).candidates.len(), 2);
    }

    #[test]
    fn command_completion() {
        let lib = Library::load(None);
        let a = analyze(&lib, None, "/", None);
        assert_eq!(a.candidates.len(), 9);
        let a = analyze(&lib, None, "/n", None);
        assert_eq!(a.ghost.as_deref(), Some("ew "));
        assert!(matches!(a.enter, EnterAction::Complete { .. }));
        let a = analyze(&lib, None, "/edit rev", None);
        assert_eq!(a.ghost.as_deref(), Some("iew"));
        let a = analyze(&lib, None, "/bogus", None);
        assert!(a.diagnostics.iter().any(|d| d.severity == Severity::Error));
    }
}
