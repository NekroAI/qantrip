use super::*;
use crate::library::Library;

fn lib() -> Library {
    Library::load(None)
}

fn values(a: &Analysis) -> Vec<(String, String)> {
    a.params.iter().filter(|p| p.is_set).map(|p| (p.name.clone(), p.display.clone())).collect()
}

fn errors(a: &Analysis) -> Vec<String> {
    a.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect()
}

fn extra(a: &Analysis) -> Option<String> {
    a.canonical.as_ref().and_then(|c| c.split_once(" -- ").map(|(_, e)| e.to_string()))
}

// docs/input-grammar.md §4.4 示例表
#[test]
fn spec_examples() {
    let l = lib();
    let a = analyze(&l, "r", None);
    assert_eq!(a.enter, EnterAction::Insert);
    assert_eq!(a.canonical.as_deref(), Some("review"));

    let a = analyze(&l, "r --scope=staged --strict --focus=security", None);
    assert!(errors(&a).is_empty(), "{:?}", errors(&a));
    assert_eq!(a.canonical.as_deref(), Some("review --focus=security --scope=staged --strict"));

    let a = analyze(&l, "r security 重点检查认证和权限模块", None);
    assert_eq!(a.canonical.as_deref(), Some("review --focus=security -- 重点检查认证和权限模块"));
    assert_eq!(a.phase, Phase::Extra);

    let a = analyze(&l, "r security performance", None);
    assert_eq!(a.canonical.as_deref(), Some("review --focus=security,performance"));

    let a = analyze(&l, "r 安全相关的改动", None);
    assert_eq!(extra(&a).as_deref(), Some("安全相关的改动"));
    assert!(values(&a).is_empty());

    let a = analyze(&l, "r -- security 是重点", None);
    assert_eq!(a.canonical.as_deref(), Some("review -- security 是重点"));

    let a = analyze(&l, "r ——strict", None);
    assert_eq!(a.canonical.as_deref(), Some("review --strict"));

    let a = analyze(&l, "r --focus=security，performance", None);
    assert_eq!(a.canonical.as_deref(), Some("review --focus=security,performance"));

    let a = analyze(&l, "r --scop=x ", None);
    assert!(errors(&a)[0].contains("没有候选值 `x`"), "{:?}", errors(&a));
    assert_eq!(a.enter, EnterAction::Blocked);

    let a = analyze(&l, "r --foo ", None);
    assert!(errors(&a)[0].starts_with("未知参数 --foo"), "{:?}", errors(&a));
}

#[test]
fn fullwidth_variants() {
    let l = lib();
    for input in ["r —strict", "r －－strict", "r --scope＝staged", "r　--strict"] {
        let a = analyze(&l, input, None);
        assert!(errors(&a).is_empty(), "{input}: {:?}", errors(&a));
    }
    // 附加文本保持原样
    let a = analyze(&l, "zh 保留，原样＝", None);
    assert_eq!(extra(&a).as_deref(), Some("保留，原样＝"));
}

#[test]
fn long_options() {
    let l = lib();
    // 唯一前缀
    assert_eq!(analyze(&l, "r --sc=st", None).canonical.as_deref(), Some("review --scope=staged"));
    // 布尔取值与否定
    assert_eq!(analyze(&l, "r --strict=false", None).canonical.as_deref(), Some("review"));
    assert_eq!(analyze(&l, "refactor --no-keep-api", None).canonical.as_deref(), Some("refactor --no-keep-api"));
    // 非布尔必须用 =
    let a = analyze(&l, "r --scope staged", None);
    assert!(errors(&a)[0].contains("需要一个值"));
    // 重复多选合并去重
    let a = analyze(&l, "r --focus=sec --focus=perf,security", None);
    assert_eq!(a.canonical.as_deref(), Some("review --focus=security,performance"));
    // 数字范围
    assert!(errors(&analyze(&l, "arch --options=9", None))[0].contains("取值范围"));
    assert_eq!(analyze(&l, "arch --options=4", None).canonical.as_deref(), Some("arch --options=4"));
    // 字符串带引号
    let a = analyze(&l, r#"test --framework="vitest run" 登录模块"#, None);
    assert_eq!(a.canonical.as_deref(), Some(r#"test --framework="vitest run" -- 登录模块"#));
}

#[test]
fn short_flags() {
    let l = lib();
    assert_eq!(analyze(&l, "r -s", None).canonical.as_deref(), Some("review --strict"));
    assert_eq!(analyze(&l, "zh -k", None).canonical.as_deref(), Some("zh --keep-terms"));
    assert!(errors(&analyze(&l, "r -sx", None)).iter().any(|e| e.contains("-x")));
}

#[test]
fn positional_overridden_by_long() {
    let l = lib();
    let a = analyze(&l, "zh literal --style=formal", None);
    assert_eq!(a.canonical.as_deref(), Some("zh --style=formal"));
    assert!(a.diagnostics.iter().any(|d| d.severity == Severity::Warning));
}

#[test]
fn command_tiers_and_enter() {
    let l = lib();
    let a = analyze(&l, "rev", None);
    assert_eq!(a.candidates[0].key, "review");
    assert_eq!(a.candidates[0].tier, Some(MatchTier::Prefix));
    assert_eq!(a.ghost.as_deref(), Some("iew"));
    assert_eq!(a.enter, EnterAction::Insert);

    let a = analyze(&l, "reveiw", None);
    assert_eq!(a.candidates[0].key, "review");
    assert!(matches!(a.enter, EnterAction::Complete { ref edit } if edit.text == "review"));

    // 锁定后显示已匹配提示
    let a = analyze(&l, "reveiw --strict", None);
    assert_eq!(a.hint, "已匹配为 review");
    assert_eq!(a.enter, EnterAction::Insert);

    // 中文别名
    assert_eq!(analyze(&l, "中文", None).template.unwrap().name, "zh");
    // 界面选中的模板优先
    assert_eq!(analyze(&l, "r", Some("refactor")).template.unwrap().name, "refactor");
    // 未知命令
    let a = analyze(&l, "qqqq ", None);
    assert!(a.template.is_none() && a.enter == EnterAction::Blocked);
}

#[test]
fn completions() {
    let l = lib();
    let a = analyze(&l, "r --", None);
    assert!(a.candidates.iter().any(|c| c.title == "--scope"));
    let a = analyze(&l, "r --sc", None);
    assert_eq!(a.ghost.as_deref(), Some("ope="));
    let a = analyze(&l, "r --scope=st", None);
    assert_eq!(a.ghost.as_deref(), Some("aged"));
    assert_eq!(a.candidates[0].edit.text, "--scope=staged ");
    let a = analyze(&l, "r --focus=security,p", None);
    assert_eq!(a.candidates[0].key, "performance");
    assert_eq!(a.candidates[0].edit.text, "--focus=security,performance ");
    // 位置参数候选
    let a = analyze(&l, "r sec", None);
    assert!(a.candidates.iter().any(|c| c.key == "security"));
    // 进入附加文本后不再补全
    let a = analyze(&l, "r 帮我看看 --", None);
    assert!(a.candidates.is_empty() && a.phase == Phase::Extra);
    // refactor 的 keep-api 默认开启，补全提示 --no-keep-api
    assert!(analyze(&l, "refactor --no", None).candidates.iter().any(|c| c.title == "--no-keep-api"));
}

#[test]
fn tokens_use_utf16_offsets() {
    let l = lib();
    let a = analyze(&l, "r 🧙 --x", None);
    // 🧙 是 2 个 UTF-16 单元，附加文本从第 2 个单元开始到末尾
    let extra = a.tokens.iter().find(|t| t.kind == TokenKind::Extra).unwrap();
    assert_eq!((extra.start, extra.end), (2, 8));
    let a = analyze(&l, "r --scope=staged", None);
    let kinds: Vec<_> = a.tokens.iter().map(|t| (t.kind, t.start, t.end)).collect();
    assert_eq!(kinds, vec![(TokenKind::Command, 0, 1), (TokenKind::OptionName, 2, 9), (TokenKind::OptionValue, 10, 16)]);
}

#[test]
fn preview_highlights() {
    let l = lib();
    let a = analyze(&l, "r --strict 注意并发", None);
    let p = a.preview.unwrap();
    let units: Vec<u16> = p.text.encode_utf16().collect();
    let extra = p.highlights.iter().find(|h| h.kind == HighlightKind::Extra).unwrap();
    assert_eq!(String::from_utf16(&units[extra.start as usize..extra.end as usize]).unwrap(), "注意并发");
    assert!(p.highlights.iter().any(|h| h.kind == HighlightKind::Param));
    // 默认值不产生高亮
    assert!(analyze(&l, "r", None).preview.unwrap().highlights.is_empty());
}

#[test]
fn claude_code_collapse_flag() {
    let l = lib();
    assert!(!analyze(&l, "zh", None).preview.unwrap().collapses_in_claude_code);
    assert!(analyze(&l, "r", None).preview.unwrap().collapses_in_claude_code);
}

#[test]
fn canonical_round_trip() {
    let l = lib();
    let inputs = [
        "r security perf 检查 \"引号\" 和 , 逗号",
        "r -s --scope=branch",
        "test unit edge --framework=\"jest --ci\" 登录",
        "refactor --no-keep-api dup naming",
        "commit plain --lang=zh --run",
        "pr --sections=risks,summary --create",
        "arch cost --options=5 选型",
        "zh -k formal",
        "investigate quick --hypotheses=2 为什么慢",
        "plan --steps=7 -e",
    ];
    for input in inputs {
        let a = analyze(&l, input, None);
        let c = a.canonical.clone().unwrap_or_else(|| panic!("{input}: {:?}", errors(&a)));
        let b = analyze(&l, &c, None);
        assert_eq!(b.canonical.as_deref(), Some(c.as_str()), "round trip of {input}");
        assert_eq!(a.preview.unwrap().text, b.preview.unwrap().text, "{input}");
    }
}

#[test]
fn render_final_requires_valid_input() {
    let l = lib();
    assert!(render_final(&l, "r --scope=nope", None).is_none());
    assert!(render_final(&l, "zh 你好", None).unwrap().ends_with("补充要求：你好"));
}
