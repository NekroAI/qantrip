//! Spike 用的硬编码模板与简单匹配，阶段 2 会整体替换。

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, uniffi::Enum)]
pub enum MatchTier {
    Exact,
    Prefix,
    Fuzzy,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TemplateSummary {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub tier: MatchTier,
}

struct Template {
    id: &'static str,
    name: &'static str,
    aliases: &'static [&'static str],
    description: &'static str,
    body: fn() -> String,
}

const TEMPLATES: &[Template] = &[
    Template {
        id: "zh",
        name: "zh",
        aliases: &["中文"],
        description: "翻译成中文",
        body: || "帮我翻译成中文。".into(),
    },
    Template {
        id: "en",
        name: "en",
        aliases: &[],
        description: "翻译成英文",
        body: || "Translate the following into natural English.".into(),
    },
    Template {
        id: "review",
        name: "review",
        aliases: &["r"],
        description: "代码审查（多行）",
        body: || {
            [
                "请审查当前改动，按以下要求输出：",
                "",
                "1. 先理解改动意图，再找问题。",
                "2. 重点关注：正确性、边界条件、安全、可维护性。",
                "3. 区分\"确认的问题\"和\"需要验证的疑点\"。",
                "4. 每个问题给出文件与行号、原因和修改建议。",
                "",
                "不要直接修改代码。",
            ]
            .join("\n")
        },
    },
    Template {
        id: "unicode",
        name: "unicode",
        aliases: &["uni"],
        description: "测试：Unicode / 特殊字符",
        body: || {
            [
                "中文、日本語、한국어、Ελληνικά、עברית",
                "Emoji: 🧙‍♂️✨🪄 👩🏽‍💻 🇨🇳",
                "符号: `code` $HOME %s {{x}} \\n \\t \"quote\" 'single' <tag> & | ;",
                "全角：，。！？「」——＝",
                "行尾空格→   ",
                "\ttab 开头",
            ]
            .join("\n")
        },
    },
    Template {
        id: "long",
        name: "long",
        aliases: &[],
        description: "测试：长文本（约 8000 字符、200 行）",
        body: || {
            (1..=200)
                .map(|i| format!("{i:03} 长文本测试行：The quick brown fox jumps over the lazy dog. 敏捷的棕色狐狸。"))
                .collect::<Vec<_>>()
                .join("\n")
        },
    },
];

fn command_word(input: &str) -> &str {
    input.trim_start().split_whitespace().next().unwrap_or("")
}

fn extra_text(input: &str) -> &str {
    let s = input.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => s[i..].trim(),
        None => "",
    }
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

fn tier(t: &Template, q: &str) -> Option<MatchTier> {
    let names = std::iter::once(t.name).chain(t.aliases.iter().copied());
    let mut best: Option<MatchTier> = None;
    for n in names {
        let n = n.to_lowercase();
        let m = if n == q {
            Some(MatchTier::Exact)
        } else if n.starts_with(q) {
            Some(MatchTier::Prefix)
        } else if is_subsequence(q, &n) {
            Some(MatchTier::Fuzzy)
        } else {
            None
        };
        best = match (best, m) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    best
}

pub fn search(input: &str) -> Vec<TemplateSummary> {
    let q = command_word(input).to_lowercase();
    let mut out: Vec<TemplateSummary> = TEMPLATES
        .iter()
        .filter_map(|t| {
            let tier = if q.is_empty() { Some(MatchTier::Prefix) } else { tier(t, &q) }?;
            Some(TemplateSummary {
                id: t.id.into(),
                name: t.name.into(),
                aliases: t.aliases.iter().map(|a| a.to_string()).collect(),
                description: t.description.into(),
                tier,
            })
        })
        .collect();
    out.sort_by(|a, b| a.tier.cmp(&b.tier).then_with(|| a.name.cmp(&b.name)));
    out
}

pub fn render(template_id: &str, input: &str) -> Option<String> {
    let t = TEMPLATES.iter().find(|t| t.id == template_id)?;
    let mut body = (t.body)();
    let extra = extra_text(input);
    if !extra.is_empty() {
        body.push_str("\n\n附加要求：");
        body.push_str(extra);
    }
    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_order() {
        let r = search("r");
        assert_eq!(r[0].id, "review");
        assert_eq!(r[0].tier, MatchTier::Exact);
        assert_eq!(search("rev")[0].tier, MatchTier::Prefix);
        assert_eq!(search("rvw")[0].tier, MatchTier::Fuzzy);
        assert!(search("xyz").is_empty());
    }

    #[test]
    fn extra_text_appended() {
        let s = render("zh", "zh  重点 保留术语 ").unwrap();
        assert!(s.ends_with("附加要求：重点 保留术语"));
        assert_eq!(render("zh", "zh").unwrap(), "帮我翻译成中文。");
    }
}
