//! 命令行（不启动界面）：`qantrip list / render / complete / version / help`。
//! 与面板共用同一个核心、模板目录与语言设置。

use std::sync::Arc;

use qantrip_core::{Engine, Severity};

use crate::paths;
use crate::settings::Settings;
use crate::t;

fn engine() -> Arc<Engine> {
    let settings = Settings::load();
    let ui = settings.ui_locale();
    crate::i18n::set_ui_locale(ui);
    let e = Engine::new(Some(paths::templates_dir().to_string_lossy().into()), Some(paths::usage_file().to_string_lossy().into()));
    e.set_locale(ui, settings.template_locale());
    e
}

pub fn help() -> String {
    t!(
        "用法：qantrip [命令]\n\n\
         不带命令时启动 Qantrip（已在运行时打开模板库与设置）。\n\n\
         命令：\n  \
         toggle              打开或关闭面板（可绑定到系统快捷键）\n  \
         settings            打开设置\n  \
         library [名称]      打开模板库\n  \
         list [--json]       列出模板\n  \
         render <输入…>      按面板语法渲染 Prompt 并输出，如 qantrip render r security\n  \
         complete <输入…>    输出补全候选（每行：替换后的输入 TAB 说明）\n  \
         quit                退出正在运行的 Qantrip\n  \
         version             显示版本\n\n\
         toggle 每次调用执行一次动作，不提供物理释放事件。合成器绑定请关闭长按重复。\n",
        "Usage: qantrip [command]\n\n\
         Without a command, starts Qantrip (or opens the template library and settings if it is running).\n\n\
         Commands:\n  \
         toggle              Open or close the panel (bind this to a system shortcut)\n  \
         settings            Open Settings\n  \
         library [name]      Open the template library\n  \
         list [--json]       List templates\n  \
         render <input…>     Render a prompt using the panel syntax, e.g. qantrip render r security\n  \
         complete <input…>   Print completions (one per line: completed input TAB description)\n  \
         quit                Quit the running Qantrip\n  \
         version             Show the version\n\n\
         toggle invokes one action per call, without a physical release event. Disable key repeat in compositor bindings.\n"
    )
}

/// 处理不需要界面的命令；返回 None 表示交给图形界面处理。
pub fn run(args: &[String]) -> Option<i32> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let rest = if args.len() > 1 { args[1..].join(" ") } else { String::new() };
    match cmd {
        "help" | "--help" | "-h" => {
            print!("{}", help());
            Some(0)
        }
        "version" | "--version" | "-V" => {
            println!("Nekro Qantrip {} (core {})", crate::VERSION, qantrip_core::core_version());
            Some(0)
        }
        "list" => {
            let e = engine();
            let json = args.iter().any(|a| a == "--json");
            let list = e.templates();
            if json {
                let v: Vec<_> = list
                    .iter()
                    .map(|t| serde_json::json!({"name": t.name, "aliases": t.aliases, "description": t.description, "category": t.category, "builtin": t.builtin}))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                for t in list {
                    let aliases = if t.aliases.is_empty() { String::new() } else { format!(" ({})", t.aliases.join(", ")) };
                    println!("{}{}\t{}", t.name, aliases, t.description);
                }
            }
            for p in e.problems() {
                eprintln!("! {p}");
            }
            Some(0)
        }
        "render" => {
            let e = engine();
            if rest.trim().is_empty() {
                eprintln!("{}", t!("用法：qantrip render <输入>", "Usage: qantrip render <input>"));
                return Some(2);
            }
            match e.render(rest.clone(), None) {
                Some(text) => {
                    print!("{text}");
                    if !text.ends_with('\n') {
                        println!();
                    }
                    Some(0)
                }
                None => {
                    let a = e.analyze(rest, None);
                    let mut any = false;
                    for d in a.diagnostics.iter().filter(|d| d.severity == Severity::Error) {
                        eprintln!("{}", d.message);
                        any = true;
                    }
                    if !any {
                        eprintln!("{}", if a.hint.is_empty() { t!("无法渲染", "Couldn't render") } else { a.hint.clone() });
                    }
                    Some(1)
                }
            }
        }
        "complete" => {
            let e = engine();
            let a = e.analyze(rest.clone(), None);
            for c in a.candidates {
                if let Some((text, _)) = crate::util::replace_u16(&rest, c.edit.start, c.edit.end, &c.edit.text) {
                    let desc = if c.subtitle.is_empty() { c.detail.clone() } else { c.subtitle.clone() };
                    println!("{text}\t{desc}");
                }
            }
            if let Some(g) = a.ghost.filter(|g| !g.is_empty()) {
                println!("{rest}{g}\t{}", t!("灰字补全", "inline completion"));
            }
            Some(0)
        }
        _ => None,
    }
}
