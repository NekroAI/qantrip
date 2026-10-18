//! 面板状态。解析、补全、校验、预览和首屏全部来自 Rust 核心；这里只管选择、编辑与动效状态。
//! 命令行文本是唯一的状态来源：键盘与鼠标操作都通过改写它生效（docs/decisions.md D16）。
//! 与 macOS 的 LauncherModel.swift 逐项对应。

use std::sync::Arc;

use qantrip_core::{Analysis, CandidateKind, Diagnostic, Edit, Engine, EnterAction, ParamEdit, Severity};

use qantrip_core::command::CommandOutcome;

use crate::foreground::AppRef;
use crate::util;

/// 模型操作的结果，由界面执行
pub enum Effect {
    None,
    /// 输入框需要改为指定文本与光标（UTF-16）
    SetInput {
        text: String,
        caret: u32,
    },
    /// 插入：(最终文本, 是否展开粘贴)
    Submit {
        text: String,
        expand: bool,
    },
    Command(CommandOutcome),
    /// 回车被阻止：只做视觉反馈
    Reject,
}

pub struct Model {
    engine: Arc<Engine>,
    pub query: String,
    pub analysis: Analysis,
    pub selected_index: usize,
    /// 需要再按一次回车确认的指令（如 /rm）；输入改变后失效
    pub armed_command: Option<String>,
    pub target: Option<AppRef>,
    /// 回车被阻止的次数（触发抖动）
    pub reject_count: u32,
    /// 界面高亮的模板名或首屏调用；命令词改变时清空
    selected_key: Option<String>,
    command_word: String,
    clipboard_loaded: bool,
}

impl Model {
    pub fn new(engine: Arc<Engine>) -> Model {
        let analysis = engine.analyze(String::new(), None);
        Model {
            engine,
            query: String::new(),
            analysis,
            selected_index: 0,
            armed_command: None,
            target: None,
            reject_count: 0,
            selected_key: None,
            command_word: String::new(),
            clipboard_loaded: false,
        }
    }

    /// 候选列表的"类型"：首屏 / 命令 / 参数 / 取值。类型切换时整组替换（D15）。
    pub fn list_kind(&self) -> String {
        let Some(c) = self.analysis.candidates.first() else { return "empty".into() };
        if self.query.trim().is_empty() {
            return "home".into();
        }
        if self.analysis.locked { format!("locked-{:?}", c.kind) } else { "command".into() }
    }

    pub fn first_error(&self) -> Option<&Diagnostic> {
        let d = &self.analysis.diagnostics;
        d.iter().find(|d| d.severity == Severity::Error && !d.at_cursor).or_else(|| d.iter().find(|d| d.severity == Severity::Error))
    }

    pub fn is_command_mode(&self) -> bool {
        util::is_command_mode(&self.query)
    }

    /// 面板打开：以当前前台应用取使用记录快照，回到首屏。
    pub fn begin(&mut self, target: Option<AppRef>) {
        self.engine.begin_session(target.as_ref().map(|t| t.id.clone()));
        self.target = target;
        self.selected_key = None;
        self.command_word.clear();
        self.clipboard_loaded = false;
        self.engine.set_clipboard(None);
        self.query.clear();
        self.armed_command = None;
        self.reanalyze(true);
    }

    /// `/new 名称` 不写正文时用剪贴板文字：只在输入 /new 时读取剪贴板，且每次唤起只读一次。
    /// 返回 true 表示需要界面异步读取剪贴板后调用 `set_clipboard_text`。
    pub fn wants_clipboard(&self) -> bool {
        !self.clipboard_loaded && self.query.trim_start().starts_with("/new ")
    }

    pub fn set_clipboard_text(&mut self, text: Option<String>) {
        self.clipboard_loaded = true;
        self.engine.set_clipboard(text);
        self.reanalyze(false);
    }

    pub fn mark_clipboard_requested(&mut self) {
        self.clipboard_loaded = true;
    }

    pub fn text_did_change(&mut self, text: &str) {
        self.query = text.to_string();
        self.armed_command = None;
        let cmd = util::first_word(text);
        if cmd != self.command_word {
            self.command_word = cmd;
            self.selected_key = None;
        }
        self.reanalyze(true);
    }

    fn reanalyze(&mut self, reset_selection: bool) {
        self.analysis = self.engine.analyze(self.query.clone(), self.selected_key.clone());
        if reset_selection {
            self.selected_index = 0;
        }
        let n = self.analysis.candidates.len();
        self.selected_index = self.selected_index.min(n.saturating_sub(1));
    }

    // MARK: 键盘

    pub fn move_selection(&mut self, delta: i32) {
        let n = self.analysis.candidates.len() as i32;
        if n == 0 {
            return;
        }
        let i = ((self.selected_index as i32 + delta) % n + n) % n;
        self.select(i as usize);
    }

    pub fn select(&mut self, index: usize) {
        if index >= self.analysis.candidates.len() {
            return;
        }
        self.selected_index = index;
        // 命令阶段 / 首屏：切换高亮项时预览随之切换
        let c = &self.analysis.candidates[index];
        if !self.analysis.locked && matches!(c.kind, CandidateKind::Template | CandidateKind::Invocation) {
            self.selected_key = Some(c.key.clone());
            self.reanalyze(false);
        }
    }

    /// Tab：接受高亮的候选，没有候选时接受错误的修正建议。
    pub fn complete(&mut self) -> Effect {
        if let Some(c) = self.analysis.candidates.get(self.selected_index).cloned() {
            if c.kind == CandidateKind::Template {
                self.selected_key = Some(c.key.clone());
            }
            return self.apply(&c.edit);
        }
        if let Some(fix) = self.analysis.diagnostics.iter().find_map(|d| d.fix.clone()) {
            return self.apply(&fix);
        }
        Effect::None
    }

    /// 光标在末尾时接受灰字补全（→ / End）
    pub fn accept_ghost(&mut self) -> Effect {
        match self.analysis.ghost.clone().filter(|g| !g.is_empty()) {
            Some(g) => {
                let text = format!("{}{g}", self.query);
                let caret = text.encode_utf16().count() as u32;
                self.set_input(&text);
                Effect::SetInput { text, caret }
            }
            None => Effect::None,
        }
    }

    pub fn confirm(&mut self, expand: bool, record_usage: bool) -> Effect {
        match self.analysis.enter.clone() {
            EnterAction::Insert => {
                let Some(text) = self.engine.render(self.query.clone(), self.selected_key.clone()) else { return self.reject() };
                let collapses = self.analysis.preview.as_ref().map(|p| p.collapses_in_claude_code).unwrap_or(false);
                if record_usage {
                    self.engine.record_use(self.query.clone(), self.selected_key.clone(), self.target.as_ref().map(|t| t.id.clone()));
                }
                Effect::Submit { text, expand: expand && collapses }
            }
            EnterAction::Complete { edit } => {
                if !self.analysis.locked {
                    self.selected_key = self.analysis.template.as_ref().map(|t| t.name.clone());
                }
                self.apply(&edit)
            }
            EnterAction::Blocked => self.reject(),
            EnterAction::Run { confirm } => {
                if confirm && self.armed_command.as_deref() != Some(self.query.as_str()) {
                    self.armed_command = Some(self.query.clone());
                    return Effect::None;
                }
                self.armed_command = None;
                match self.engine.run_command(self.query.clone()) {
                    Ok(outcome) => Effect::Command(outcome),
                    Err(e) => {
                        crate::log::event("command_failed", serde_json::json!({"error": e.to_string()}));
                        self.reject()
                    }
                }
            }
        }
    }

    fn reject(&mut self) -> Effect {
        self.reject_count += 1;
        Effect::Reject
    }

    // MARK: 鼠标

    pub fn click(&mut self, index: usize) -> Effect {
        if index >= self.analysis.candidates.len() {
            return Effect::None;
        }
        self.select(index);
        self.complete()
    }

    pub fn double_click(&mut self, index: usize, record_usage: bool) -> Effect {
        let Some(c) = self.analysis.candidates.get(index).cloned() else { return Effect::None };
        if matches!(c.kind, CandidateKind::Template | CandidateKind::Invocation) && !self.analysis.locked {
            self.select(index);
            self.confirm(false, record_usage)
        } else {
            self.click(index)
        }
    }

    pub fn edit_param(&mut self, name: &str, edit: ParamEdit) -> Effect {
        let Some(r) = self.engine.edit_param(self.query.clone(), self.selected_key.clone(), name.to_string(), edit) else { return self.reject() };
        if self.query.trim().is_empty() {
            self.selected_key = self.analysis.template.as_ref().map(|t| t.name.clone());
        }
        self.set_input(&r.text);
        Effect::SetInput { text: r.text, caret: r.caret }
    }

    // MARK: 编辑

    fn apply(&mut self, edit: &Edit) -> Effect {
        let Some((text, caret)) = util::replace_u16(&self.query, edit.start, edit.end, &edit.text) else { return Effect::None };
        self.set_input(&text);
        Effect::SetInput { text, caret }
    }

    fn set_input(&mut self, text: &str) {
        self.command_word = util::first_word(text);
        self.query = text.to_string();
        self.armed_command = None;
        self.reanalyze(true);
    }
}
