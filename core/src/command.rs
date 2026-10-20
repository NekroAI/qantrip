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
    Restore,
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
    (Cmd::Restore, "restore"),
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
            Cmd::Restore => tr!(loc, "/restore 名称", "/restore name"),
            Cmd::Sync => "/sync".into(),
            Cmd::Settings => "/settings".into(),
            Cmd::Library => tr!(loc, "/library [名称]", "/library [name]"),
            Cmd::Lang => "/lang auto|zh|en".into(),
        }
    }

    fn summary(self, loc: Locale) -> String {
        match self {
            Cmd::New => tr!(
                loc,
                "新建模板。正文里 {{名}} 是填空，{{名:a=文字|b=文字}} 按选项输出不同文字，{{名?文字}} 是开关；不写正文就用剪贴板里的文字",
                "Create a template. In the text, {{name}} is a blank, {{name:a=text|b=text}} outputs text by option, {{name?text}} is a switch; with no text, the clipboard is used"
            ),
            Cmd::Edit => tr!(loc, "在模板库中打开一个模板，编辑参数和正文", "Open a template in the library to edit its parameters and text"),
            Cmd::Dup => tr!(loc, "复制一个模板并起新名字；内置模板要先复制才能修改", "Copy a template under a new name; copy a built-in one to change it"),
            Cmd::Alias => tr!(loc, "给自己的模板加一个简写，如给 review 加 r，之后输入 r 就等于 review", "Give one of your templates a short name, e.g. r for review"),
            Cmd::Rm => tr!(loc, "删除模板（回车两次确认）；删除的内置模板可以用 /restore 恢复", "Delete a template (press Enter twice); deleted built-in ones come back with /restore"),
            Cmd::Restore => tr!(loc, "恢复被删除或改过的内置模板", "Bring back a built-in template you deleted or changed"),
            Cmd::Sync => tr!(loc, "立即同步模板", "Sync templates now"),
            Cmd::Settings => tr!(loc, "打开设置", "Open settings"),
            Cmd::Library => tr!(loc, "打开模板库；带名称时直接打开那个模板", "Open the template library; with a name, open that template"),
            Cmd::Lang => tr!(loc, "切换内置模板的语言（auto 跟随界面语言）", "Switch the built-in templates' language (auto follows the interface)"),
        }
    }
}

/// 系统回收站的叫法（中文, 英文）：macOS 为废纸篓，Windows 为回收站，Linux 桌面（GNOME / KDE）为回收站 / Trash
fn trash_word() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("回收站", "Recycle Bin")
    } else if cfg!(target_os = "macos") {
        ("废纸篓", "Trash")
    } else {
        ("回收站", "Trash")
    }
}

fn is_builtin_name(lib: &Library, name: &str) -> bool {
    builtin_source(lib, name).is_some()
}

/// 当前内置模板语言下的内置模板源码
fn builtin_source(lib: &Library, name: &str) -> Option<&'static str> {
    crate::library::builtin_source_in(name, lib.template_locale)
}

/// 删除标记文件：`名称.deleted.md`（与可能存在的用户修改版本 `名称.md` 不冲突）
fn marker_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.deleted.md"))
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
    use crate::placeholder::{self as ph, Placeholder};
    let mut params: Vec<ParamDoc> = Vec::new();
    let mut seen: Vec<Placeholder> = Vec::new();
    let mut out = String::new();
    let mut pos = 0;
    for (s, e, p) in ph::scan(body, loc) {
        let p = p?;
        out.push_str(&ph::literal(&body[pos..s]));
        let pname = p.name().to_string();
        match seen.iter().find(|q| q.name() == pname) {
            Some(q) if !ph::compatible(q, &p) => {
                return Err(tr!(loc, "参数 `{pname}` 的写法前后不一致", "Parameter `{pname}` is written differently in two places"));
            }
            Some(_) => {}
            None => {
                let (kind, choices, default_value, description) = match &p {
                    Placeholder::Fill { .. } => (ParamKind::String, vec![], None, String::new()),
                    Placeholder::Flag { on, .. } => (ParamKind::Bool, vec![], None, short_label(on)),
                    Placeholder::Choice { options, unset, .. } => {
                        let unset_label = unset.as_deref().map(short_label);
                        (
                            ParamKind::Enum,
                            options.iter().map(|(k, t)| Choice { value: k.clone(), label: short_label(t), aliases: vec![] }).collect(),
                            if unset.is_some() { None } else { options.first().map(|(k, _)| k.clone()) },
                            unset_label.map(|u| tr!(loc, "不写时：{u}", "Left out: {u}")).unwrap_or_default(),
                        )
                    }
                };
                let positional = kind == ParamKind::Enum && !params.iter().any(|q| q.positional);
                params.push(ParamDoc {
                    name: pname.clone(),
                    kind,
                    description,
                    choices,
                    default_value,
                    positional,
                    short: None,
                    required: false,
                    requires: vec![],
                    conflicts: vec![],
                    min: None,
                    max: None,
                });
                seen.push(p.clone());
            }
        }
        out.push_str(&ph::render(&p, body[e..].starts_with('\n')));
        pos = e;
    }
    out.push_str(&ph::literal(&body[pos..]));
    let mut body_out = out.trim_end().to_string();
    body_out.push_str("\n{% include \"_extra\" %}\n");
    let first_line = literal_description(&placeholder_names(body, loc));
    Ok(TemplateDoc { name: name.to_string(), aliases: vec![], description: first_line, category: String::new(), params, body: body_out })
}

/// 选项、开关的说明文字：最多 24 个字符
fn short_label(text: &str) -> String {
    let t = text.trim();
    let mut s: String = t.chars().take(24).collect();
    if t.chars().count() > 24 {
        s.push('…');
    }
    s
}

/// 描述用：占位符显示为 `{名}`
fn placeholder_names(body: &str, loc: Locale) -> String {
    let mut out = String::new();
    let mut pos = 0;
    for (s, e, p) in crate::placeholder::scan(body, loc) {
        out.push_str(&body[pos..s]);
        match p {
            Ok(p) => out.push_str(&format!("{{{}}}", p.name())),
            Err(_) => out.push_str(&body[s..e]),
        }
        pos = e;
    }
    out.push_str(&body[pos..]);
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
    /// 已确定指令的用法与说明（右栏没有其他内容时显示）
    help: String,
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
    /// 删除内置模板：写入删除标记；`trash` 为同名的用户修改版本（一并移到废纸篓）
    MarkDeleted { marker: PathBuf, name: String, trash: Option<PathBuf> },
    /// 恢复内置模板：删除标记或移走用户修改版本
    Restore { marker: Option<PathBuf>, trash: Option<PathBuf>, name: String },
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
    let mut ctx = Ctx { input, tokens: vec![], diags: vec![], help: String::new() };
    let mut a = empty_analysis();
    let ws = words(input);
    let ends_ws = input.ends_with(char::is_whitespace);
    let Some(first) = ws.first() else { return Outcome { analysis: a, plan: None } };
    let slash = first.text.chars().next().map(char::len_utf8).unwrap_or(1);
    let typed = &first.text[slash..];

    // 指令名
    // 完整指令名，或无歧义的前缀（如 /sy → /sync），回车即按它执行（D36）
    let typed_lc = typed.to_lowercase();
    let cmd = COMMANDS.iter().find(|(_, n)| *n == typed_lc).map(|(c, _)| *c).or_else(|| {
        let mut hits = COMMANDS.iter().filter(|(_, n)| !typed_lc.is_empty() && n.starts_with(&typed_lc));
        match (hits.next(), hits.next()) {
            (Some((c, _)), None) => Some(*c),
            _ => None,
        }
    });
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
        let typed = first.text;
        a.hint = match (typed.chars().count() <= 1, a.candidates.first()) {
            (true, _) => tr!(loc, "输入指令名，如 /new 新建模板；右侧是全部指令和说明", "Type a command, e.g. /new to create a template; all commands are listed on the right"),
            (false, Some(c)) => {
                let (k, n) = (a.candidates.len(), &c.title);
                tr!(loc, "有 {k} 个指令以 {typed} 开头：继续输入，或按 Tab 补全为 {n}", "{k} commands start with {typed}: keep typing, or press Tab for {n}")
            }
            (false, None) => tr!(loc, "右侧是全部可用的指令", "All commands are listed on the right"),
        };
        return finish(ctx, a, None);
    };
    ctx.token(TokenKind::Command, first.start, first.end);
    a.hint = cmd.usage(loc);
    ctx.help = format!("{}\n    {}", cmd.usage(loc), cmd.summary(loc));
    a.locked = true;
    // 还没输入参数时提示下一步（左栏为空时显示，右栏显示在用法之前）
    let pick = tr!(loc, "输入或选择模板名", "Type or pick a template name");
    let step = match cmd {
        Cmd::New => tr!(
            loc,
            "输入新模板的名称（英文字母开头，如 push），空格后写提示词正文；正文里可以用 {{…}} 加参数",
            "Type a name for the new template (starting with a letter, e.g. push), then a space and the prompt; use {{…}} in it for parameters"
        ),
        Cmd::Edit | Cmd::Dup | Cmd::Alias | Cmd::Rm => pick,
        _ => String::new(),
    };

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
            use crate::placeholder as ph;
            let Some(name_w) = arg(1) else {
                a.hint = step;
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::Positional, name_w.start, name_w.end);
            let name = name_w.text;
            let typing_name = ws.len() == 2 && !ends_ws;
            let at_cursor = typing_name;
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
            let body_start = input.len() - input[name_w.end..].trim_start().len();
            let raw_body = &input[body_start..];
            let from_clipboard = raw_body.trim().is_empty();
            let clip = clipboard.map(str::trim).filter(|c| !c.is_empty());
            // 还没写完的占位符（`{` 之后没有 `}`）：提示下一步，暂不创建
            let open = if from_clipboard { None } else { ph::open_at_end(raw_body) };
            if from_clipboard {
                a.hint = if typing_name {
                    match clip {
                        Some(_) => tr!(loc, "模板名 {name}。空格后写提示词正文；不写正文就用剪贴板里的文字", "Template name {name}. After a space, type the prompt; leave it empty to use the clipboard"),
                        None => tr!(loc, "模板名 {name}。空格后写提示词正文", "Template name {name}. After a space, type the prompt"),
                    }
                } else {
                    tr!(loc, "写提示词正文。需要参数时用 {{…}}：左侧是三种写法的示例，按 Tab 插入第一个", "Type the prompt. For parameters use {{…}}; the list shows three forms, Tab inserts the first")
                };
            } else {
                ctx.token(TokenKind::Extra, body_start, input.trim_end().len());
                for (s0, e0, p) in ph::scan(raw_body, loc) {
                    let (s0, e0) = (body_start + s0, body_start + e0);
                    match p {
                        Ok(_) => ctx.token(TokenKind::OptionName, s0, e0),
                        Err(m) => ctx.error(m, s0, e0, false),
                    }
                }
            }
            if let Some(o) = open {
                ctx.token(TokenKind::OptionName, body_start + o, input.len());
                let (hint, err) = ph::hint_inside(&raw_body[o + 1..], loc);
                match err {
                    Some(e) => ctx.error(e, body_start + o, input.len(), true),
                    None => a.hint = hint,
                }
            } else if !typing_name {
                new_snippets(&mut a, input, loc);
            }
            let body = match (from_clipboard, open) {
                (true, _) => clip.unwrap_or(""),
                (false, Some(o)) => raw_body[..o].trim(),
                (false, None) => raw_body.trim(),
            };
            if body.is_empty() {
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
                        a.preview = Some(plain_preview(&new_examples(&t, lib, loc)));
                        if open.is_some() {
                            return finish(ctx, a, None);
                        }
                        if !from_clipboard || !typing_name {
                            a.hint = if from_clipboard {
                                tr!(loc, "↩ 用剪贴板内容创建 {name}", "↩ create {name} from the clipboard")
                            } else {
                                tr!(loc, "↩ 创建 {name}", "↩ create {name}")
                            };
                        }
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
            let Some(w) = arg(1) else {
                a.hint = step;
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::Positional, w.start, w.end);
            match find_template(lib, w.text) {
                None => {
                    let n = w.text;
                    missing_template(&mut ctx, &mut a, n, w.start, w.end, ws.len() == 2 && !ends_ws, loc);
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
            let Some(w) = arg(1) else {
                a.hint = step;
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::Positional, w.start, w.end);
            let Some(src) = find_template(lib, w.text) else {
                let n = w.text;
                missing_template(&mut ctx, &mut a, n, w.start, w.end, ws.len() == 2 && !ends_ws, loc);
                return finish(ctx, a, None);
            };
            a.template = Some(template_info(src));
            let Some(nw) = arg(2) else {
                let from = &src.name;
                a.hint = tr!(loc, "输入新模板的名称（英文字母开头），会复制 {from} 的参数和正文", "Type the new template's name (starting with a letter); it copies {from}'s parameters and text");
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
                let from = src.name.clone();
                a.preview = Some(plain_preview(&tr!(
                    loc,
                    "按回车新建 {new}：参数和正文与 {from} 相同（不复制别名），随后在模板库中打开，可以接着修改。\n\n使用时输入 {new}，和 {from} 的用法一样。",
                    "Enter creates {new} with the same parameters and text as {from} (aliases aren't copied) and opens it in the library for editing.\n\nUse it by typing {new}, just like {from}."
                )));
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
            let Some(w) = arg(1) else {
                a.hint = if lib.templates.iter().any(|t| !t.builtin) {
                    tr!(loc, "选择要加简写的模板（只能是你自己的模板）", "Choose one of your templates to give a short name")
                } else {
                    tr!(loc, "别名只能加在你自己的模板上。还没有自己的模板：先用 /new 新建，或用 /dup 复制一个内置模板", "Short names only go on your own templates. You have none yet: create one with /new, or copy a built-in one with /dup")
                };
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::Positional, w.start, w.end);
            let Some(t) = find_template(lib, w.text) else {
                let n = w.text;
                missing_template(&mut ctx, &mut a, n, w.start, w.end, ws.len() == 2 && !ends_ws, loc);
                return finish(ctx, a, None);
            };
            a.template = Some(template_info(t));
            if t.builtin {
                let n = &t.name;
                ctx.error(tr!(loc, "内置模板不能直接修改，先用 /dup {n} 新名称 复制一份", "Built-in templates can't be changed. Copy it first with /dup {n} new-name"), w.start, w.end, false);
                return finish(ctx, a, None);
            }
            let Some(aw) = arg(2) else {
                let n = &t.name;
                a.hint = tr!(loc, "输入简写（如 r），之后输入它就等于输入 {n}", "Type a short name (e.g. r); typing it then means {n}");
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
                let n = t.name.clone();
                a.preview = Some(plain_preview(&tr!(
                    loc,
                    "按回车后，输入 {alias} 就等于输入 {n}，参数照常，例如 {alias} --参数=值。",
                    "After Enter, typing {alias} means {n}; parameters work the same, e.g. {alias} --option=value."
                )));
                a.hint = tr!(loc, "↩ 给 {n} 添加别名 {alias}", "↩ add alias {alias} to {n}");
                Some(Plan::Write { path: path.clone(), source, message: tr!(loc, "{n} 的别名：{alias}", "{n} now also answers to {alias}"), open: None })
            } else {
                None
            }
        }
        Cmd::Rm => {
            suggest_templates(&mut a, 1, false);
            let Some(w) = arg(1) else {
                a.hint = step;
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::Positional, w.start, w.end);
            match find_template(lib, w.text) {
                None => {
                    let n = w.text;
                    missing_template(&mut ctx, &mut a, n, w.start, w.end, ws.len() == 2 && !ends_ws, loc);
                    None
                }
                Some(t) => {
                    let n = t.name.clone();
                    a.template = Some(template_info(t));
                    a.preview = Some(plain_preview(&t.source));
                    let has_builtin = is_builtin_name(lib, &n);
                    if has_builtin {
                        a.hint = tr!(loc, "↩ 删除 {n}（之后可用 /restore {n} 恢复）", "↩ delete {n} (bring it back with /restore {n})");
                        user_dir.map(|dir| Plan::MarkDeleted { marker: marker_path(dir, &n), name: n.clone(), trash: t.path.clone() })
                    } else {
                        let (zh, en) = trash_word();
                        a.hint = tr!(loc, "↩ 删除 {n}（移到{zh}）", "↩ delete {n} (moves it to the {en})");
                        t.path.clone().map(|path| Plan::Trash { path, name: n })
                    }
                }
            }
        }
        Cmd::Restore => {
            // 候选：已删除的内置模板与被修改过的内置模板
            let restorable: Vec<(String, String)> = lib
                .deleted
                .iter()
                .map(|d| (d.name.clone(), tr!(loc, "已删除", "deleted")))
                .chain(lib.visible().filter(|t| !t.builtin && is_builtin_name(lib, &t.name)).map(|t| (t.name.clone(), tr!(loc, "已修改", "changed"))))
                .collect();
            let current = arg(1).filter(|_| ws.len() == 2 && !ends_ws);
            if current.is_some() || (ws.len() == 1 && ends_ws) {
                let prefix = current.map(|w| w.text.to_lowercase()).unwrap_or_default();
                let (start, end) = current.map(|w| (w.start, w.end)).unwrap_or((input.len(), input.len()));
                for (n, state) in restorable.iter().filter(|(n, _)| n.starts_with(&prefix)) {
                    a.candidates.push(Candidate {
                        kind: CandidateKind::Template,
                        key: n.clone(),
                        title: n.clone(),
                        subtitle: state.clone(),
                        detail: state.clone(),
                        edit: Edit { start: u16(input, start), end: u16(input, end), text: n.clone() },
                        tier: Some(MatchTier::Prefix),
                        active: false,
                        section: String::new(),
                    });
                }
                if let (Some(w), Some(c)) = (current, a.candidates.first())
                    && c.title.len() > w.text.len()
                    && c.title.starts_with(w.text)
                {
                    a.ghost = Some(c.title[w.text.len()..].to_string());
                }
            }
            let Some(w) = arg(1) else {
                a.hint = if restorable.is_empty() {
                    tr!(loc, "没有需要恢复的内置模板", "No built-in template to restore")
                } else {
                    tr!(loc, "选择要恢复的内置模板", "Choose a built-in template to restore")
                };
                return finish(ctx, a, None);
            };
            ctx.token(TokenKind::Positional, w.start, w.end);
            let n = w.text.to_string();
            let marker = lib.deleted.iter().find(|d| d.name == n).map(|d| d.marker.clone());
            let changed = lib.visible().find(|t| !t.builtin && t.name == n && is_builtin_name(lib, &n)).and_then(|t| t.path.clone());
            if marker.is_none() && changed.is_none() {
                let msg = if is_builtin_name(lib, &n) {
                    tr!(loc, "`{n}` 已是内置版本", "`{n}` is already the built-in version")
                } else {
                    tr!(loc, "`{n}` 不是内置模板", "`{n}` is not a built-in template")
                };
                ctx.error(msg, w.start, w.end, ws.len() == 2 && !ends_ws);
                None
            } else {
                if let Some(src) = builtin_source(lib, &n) {
                    a.preview = Some(plain_preview(src));
                }
                a.hint = if changed.is_some() {
                    let (zh, en) = trash_word();
                    tr!(loc, "↩ 恢复 {n} 的内置版本（你的修改移到{zh}）", "↩ restore the built-in {n} (your version goes to the {en})")
                } else {
                    tr!(loc, "↩ 恢复 {n}", "↩ restore {n}")
                };
                Some(Plan::Restore { marker, trash: changed, name: n })
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
                            missing_template(&mut ctx, &mut a, n, w.start, w.end, ws.len() == 2 && !ends_ws, loc);
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
            let lv = w.text.to_lowercase();
            let lang = LANGS.iter().find(|(v, _, _)| *v == lv).or_else(|| {
                let mut hits = LANGS.iter().filter(|(v, _, _)| v.starts_with(&lv));
                match (hits.next(), hits.next()) {
                    (Some(h), None) => Some(h),
                    _ => None,
                }
            });
            match lang {
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

/// 模板名还没打完时（光标在名称上）不报错：有同前缀的模板时提示 Tab 补全
#[allow(clippy::too_many_arguments)]
fn missing_template(ctx: &mut Ctx, a: &mut Analysis, typed: &str, start: usize, end: usize, typing: bool, loc: Locale) {
    if typing && let Some(c) = a.candidates.first() {
        let (k, n) = (a.candidates.len(), c.title.clone());
        a.hint = if k == 1 {
            tr!(loc, "按 Tab 补全为 {n}", "Press Tab for {n}")
        } else {
            tr!(loc, "有 {k} 个模板以 {typed} 开头：继续输入，或按 ↑↓ 选择、Tab 补全", "{k} templates start with {typed}: keep typing, or pick with ↑↓ and press Tab")
        };
        return;
    }
    ctx.error(tr!(loc, "没有模板 `{typed}`；新建用 /new {typed} 正文", "No template `{typed}`; create it with /new {typed} text"), start, end, typing);
}

/// `/new` 正文中可以用的三种参数写法：作为候选列出，Tab 在末尾插入示例
fn new_snippets(a: &mut Analysis, input: &str, loc: Locale) {
    // (列表中显示的写法, 说明, Tab 插入的完整示例)
    let examples = [
        (tr!(loc, "{{topic}}", "{{topic}}"), tr!(loc, "填空：--topic=文字", "Blank: --topic=text"), tr!(loc, "{{topic}}", "{{topic}}")),
        (
            tr!(loc, "{{env:test=…|prod=…|=…}}", "{{env:test=…|prod=…|=…}}"),
            tr!(loc, "选项：按 --env 输出不同文字", "Options: text per --env"),
            tr!(loc, "{{env:test=上线到测试环境|prod=上线到正式环境|=推送到仓库}}", "{{env:test=deploy to staging|prod=deploy to production|=push to the repository}}"),
        ),
        (
            tr!(loc, "{{strict?…|…}}", "{{strict?…|…}}"),
            tr!(loc, "开关：加上 --strict 时", "Switch: with --strict"),
            tr!(loc, "{{strict?逐行严格检查|只看重要问题}}", "{{strict?check every line|only major issues}}"),
        ),
    ];
    let at = u16(input, input.len());
    for (shown, about, code) in examples {
        let sep = if input.ends_with(char::is_whitespace) || input.ends_with(['，', '。', '、', '（']) { "" } else { " " };
        a.candidates.push(Candidate {
            kind: CandidateKind::Choice,
            key: shown.clone(),
            title: shown,
            subtitle: about.clone(),
            detail: about,
            edit: Edit { start: at, end: at, text: format!("{sep}{code}") },
            tier: None,
            active: false,
            section: tr!(loc, "参数写法（Tab 插入）", "Parameter forms (Tab inserts)"),
        });
    }
}

/// `/new` 的效果预览：不带参数、每个选项、每个开关各渲染一次
fn new_examples(t: &Template, lib: &Library, loc: Locale) -> String {
    let plib = Library::preview(t.clone(), lib.locale, lib.template_locale);
    let usage = crate::usage::Usage::load(None);
    let n = &t.name;
    let mut inputs: Vec<(String, Option<String>)> = vec![(n.clone(), None)];
    for p in &t.params {
        let pn = &p.name;
        match p.kind {
            ParamKind::Enum | ParamKind::Multi => {
                for c in p.choices.iter().take(4) {
                    let v = &c.value;
                    if p.positional {
                        inputs.push((format!("{n} {v}"), Some(format!("--{pn}={v}"))));
                    } else {
                        inputs.push((format!("{n} --{pn}={v}"), None));
                    }
                }
            }
            ParamKind::Bool => inputs.push((format!("{n} --{pn}"), None)),
            _ => {
                let sample = tr!(loc, "示例", "example");
                inputs.push((format!("{n} --{pn}={sample}"), None));
            }
        }
    }
    let mut out = tr!(loc, "创建后可以这样用：\n", "Once created, use it like this:\n");
    for (input, same) in inputs.into_iter().take(10) {
        let Some(text) = crate::grammar::render_final(&plib, &usage, &input, Some(n)) else { continue };
        out.push('\n');
        out.push_str(&input);
        if let Some(s) = same {
            out.push_str(&tr!(loc, "（即 {n} {s}）", " (same as {n} {s})"));
        }
        out.push('\n');
        for line in text.trim_end().lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn finish(ctx: Ctx, mut a: Analysis, plan: Option<Plan>) -> Outcome {
    // 还没有可预览的内容时，右栏显示当前这一步的提示（如"选择要恢复的内置模板"）与指令的用法、说明
    if a.template.is_none() && a.preview.is_none() && !(a.hint.is_empty() && ctx.help.is_empty()) {
        let text = match (a.hint.as_str(), ctx.help.as_str()) {
            (h, "") => h.to_string(),
            (h, help) if h.is_empty() || help.starts_with(h) => help.to_string(),
            (h, help) => format!("{h}\n\n{help}"),
        };
        a.preview = Some(plain_preview(&text));
    }
    let errors = ctx.diags.iter().any(|d| d.severity == Severity::Error);
    a.tokens = ctx.tokens;
    a.diagnostics = ctx.diags;
    let plan = plan.filter(|_| !errors);
    a.enter = match &plan {
        Some(Plan::Trash { .. } | Plan::MarkDeleted { .. }) => EnterAction::Run { confirm: true },
        Some(_) => EnterAction::Run { confirm: false },
        // 回车不改写输入；补全交给 Tab（D36）
        None => EnterAction::Blocked,
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
        Plan::MarkDeleted { marker, name, trash } => {
            if let Some(dir) = marker.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&marker, format!("---\nname: {name}\ndeleted: true\n---\n")).map_err(|e| e.to_string())?;
            Ok(CommandOutcome {
                message: tr!(loc, "已删除 {name}，可用 /restore {name} 恢复", "Deleted {name}. Bring it back with /restore {name}"),
                open_editor: None,
                trash_path: trash.map(|p| p.to_string_lossy().to_string()),
                action: None,
            })
        }
        Plan::Restore { marker, trash, name } => {
            if let Some(m) = marker {
                std::fs::remove_file(&m).map_err(|e| e.to_string())?;
            }
            Ok(CommandOutcome {
                message: tr!(loc, "已恢复内置模板 {name}", "Restored the built-in {name}"),
                open_editor: None,
                trash_path: trash.map(|p| p.to_string_lossy().to_string()),
                action: None,
            })
        }
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

    /// 测试并行运行：用计数器区分目录（macOS 的系统时钟只精确到微秒，按时间取名会撞车）
    fn rand_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::SeqCst)
    }

    /// `/new` 的选项与开关写法：生成的模板按参数输出不同文字，换行保留
    #[test]
    fn new_with_options_and_switches() {
        let d = build_template("push", "帮我{env:test=上线到测试环境|prod=上线到正式环境|=推送到仓库}。\n{strict?先跑完整测试|}\n结束", Locale::Zh).unwrap();
        let source = doc::serialize_doc(&d);
        let lib = Library::with_override(None, &source).0;
        let usage = crate::usage::Usage::load(None);
        let r = |i: &str| crate::grammar::render_final(&lib, &usage, i, None).unwrap();
        assert_eq!(r("push"), "帮我推送到仓库。\n\n结束");
        assert_eq!(r("push test"), "帮我上线到测试环境。\n\n结束");
        assert_eq!(r("push --env=prod --strict"), "帮我上线到正式环境。\n先跑完整测试\n结束");
        assert_eq!(d.params[0].default_value, None);
        assert_eq!(d.params[0].choices[0].label, "上线到测试环境");
        // 旧写法不变：取值原样填入，第一个为默认
        let d = build_template("tr", "翻译成{lang:英文|日文}", Locale::Zh).unwrap();
        assert_eq!(d.params[0].default_value.as_deref(), Some("英文"));
        assert!(d.body.contains("{{ lang }}"));
        // 写法有误时报错
        assert!(build_template("x", "{env:测试=a}", Locale::Zh).is_err());
        assert!(build_template("x", "{env:a|b} {env?c}", Locale::Zh).is_err());
    }

    /// `/new` 输入过程中：正文阶段列出三种写法；写到一半的占位符给出下一步；效果预览列出各种用法
    #[test]
    fn new_guides_while_typing() {
        let lib = Library::load(None);
        let a = analyze(&lib, None, "/new push ", None);
        assert_eq!(a.candidates.len(), 3);
        let a = analyze(&lib, None, "/new push 帮我{env:test=", None);
        assert!(a.hint.contains("--env=test"), "{}", a.hint);
        assert_eq!(a.enter, EnterAction::Blocked, "占位符没写完时不创建");
        let a = analyze(&lib, None, "/new push 帮我{env:测试=", None);
        assert!(a.diagnostics.iter().any(|d| d.message.contains("测试")));
        let a = analyze(&lib, None, "/new push 帮我{env:test=上线到测试环境|=推送到仓库}", None);
        let p = a.preview.unwrap().text;
        assert!(p.contains("push test") && p.contains("帮我上线到测试环境") && p.contains("帮我推送到仓库"), "{p}");
        // 模板名没打完时不报错，提示 Tab 补全
        let a = analyze(&lib, None, "/edit rev", None);
        assert!(a.diagnostics.is_empty() && a.hint.contains("Tab"), "{}", a.hint);
    }

    /// 指令确定、还没有参数时：左栏为空，提示下一步；右栏显示这一步、用法与说明
    #[test]
    fn command_without_args_explains_next_step() {
        let lib = Library::load(None);
        for input in ["/new", "/new "] {
            let a = analyze(&lib, None, input, None);
            assert!(a.hint.contains("名称"), "{input}: {}", a.hint);
        }
        let a = analyze(&lib, None, "/new ", None);
        assert!(a.candidates.is_empty());
        let p = a.preview.unwrap().text;
        assert!(p.starts_with(&a.hint) && p.contains("/new 名称 [正文]") && p.contains("新建模板"), "{p}");
        let a = analyze(&lib, None, "/dup review ", None);
        assert!(a.template.is_some());
        assert!(a.hint.contains("新模板的名称"), "{}", a.hint);
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
        assert_eq!(analyze(&lib, Some(&dir), "/rm review", None).enter, EnterAction::Run { confirm: true });
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
    fn delete_and_restore_builtins() {
        let dir = tmp();
        let lib = Library::load(Some(&dir));
        // 删除内置模板：写入标记，名称与别名空出来
        let out = run(&lib, Some(&dir), "/rm review", None).unwrap();
        assert!(out.trash_path.is_none());
        let lib = Library::load(Some(&dir));
        assert!(lib.get("review").is_none());
        assert_eq!(lib.deleted.len(), 1);
        assert!(lib.problems.is_empty(), "{:?}", lib.problems);
        // 空出来的名称和别名可以用于新模板
        run(&lib, Some(&dir), "/new r 我自己的 r", None).unwrap();
        let lib = Library::load(Some(&dir));
        assert!(lib.get("r").is_some());
        // 恢复后内置别名 r 与用户模板 r 冲突，按别名冲突规则停用该别名并报告
        let a = analyze(&lib, Some(&dir), "/restore ", None);
        assert_eq!(a.candidates.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["review"]);
        run(&lib, Some(&dir), "/restore review", None).unwrap();
        let lib = Library::load(Some(&dir));
        assert!(lib.get("review").is_some_and(|t| t.builtin));
        assert!(lib.deleted.is_empty());
        // 修改过的内置模板：/rm 把修改版本移到废纸篓并写入标记；/restore 恢复内置版本
        run(&lib, Some(&dir), "/dup explain explain2", None).unwrap();
        std::fs::write(dir.join("explain.md"), "---\nname: explain\n---\n我的解释\n").unwrap();
        let lib = Library::load(Some(&dir));
        assert!(!lib.get("explain").unwrap().builtin);
        let out = run(&lib, Some(&dir), "/rm explain", None).unwrap();
        assert!(out.trash_path.unwrap().ends_with("explain.md"));
        std::fs::remove_file(dir.join("explain.md")).unwrap();
        let lib = Library::load(Some(&dir));
        assert!(lib.get("explain").is_none());
        run(&lib, Some(&dir), "/restore explain", None).unwrap();
        let lib = Library::load(Some(&dir));
        assert!(lib.get("explain").is_some_and(|t| t.builtin));
        // 内置版本不需要恢复
        assert_eq!(analyze(&lib, Some(&dir), "/restore explain", None).enter, EnterAction::Blocked);
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
        // 无歧义前缀直接执行（D36）
        assert_eq!(act("/lang z").unwrap().as_deref(), Some("lang:zh"));
        assert_eq!(act("/sy").unwrap().as_deref(), Some("sync"));
        assert_eq!(analyze(&lib, None, "/s", None).candidates.len(), 2);
    }

    #[test]
    fn command_completion() {
        let lib = Library::load(None);
        let a = analyze(&lib, None, "/", None);
        assert_eq!(a.candidates.len(), 10);
        let a = analyze(&lib, None, "/n", None);
        assert_eq!(a.ghost.as_deref(), Some("ew "));
        assert_eq!(a.enter, EnterAction::Blocked);
        let a = analyze(&lib, None, "/edit rev", None);
        assert_eq!(a.ghost.as_deref(), Some("iew"));
        let a = analyze(&lib, None, "/bogus", None);
        assert!(a.diagnostics.iter().any(|d| d.severity == Severity::Error));
    }
}
