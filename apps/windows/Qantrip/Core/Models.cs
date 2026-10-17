using System.Text.Json.Serialization;

namespace Qantrip.Core;

// 与 core/src/grammar.rs 的 serde 输出一一对应（snake_case）。枚举值保留为字符串，常量见各类型。

public sealed class Token
{
    public string Kind { get; set; } = "";
    public int Start { get; set; }
    public int End { get; set; }
}

public sealed class Edit
{
    public int Start { get; set; }
    public int End { get; set; }
    public string Text { get; set; } = "";
}

public sealed class Diagnostic
{
    public string Severity { get; set; } = "error";
    public string Message { get; set; } = "";
    public int Start { get; set; }
    public int End { get; set; }
    public bool AtCursor { get; set; }
    public Edit? Fix { get; set; }
    public bool IsError => Severity == "error";
}

public sealed class Candidate
{
    /// template / param / choice / invocation
    public string Kind { get; set; } = "";
    public string Key { get; set; } = "";
    public string Title { get; set; } = "";
    public string Subtitle { get; set; } = "";
    public string Detail { get; set; } = "";
    public Edit Edit { get; set; } = new();
    public string? Tier { get; set; }
    public bool Active { get; set; }
    public string Section { get; set; } = "";
}

public sealed class ChoiceState
{
    public string Value { get; set; } = "";
    public string Label { get; set; } = "";
    public bool On { get; set; }
}

public sealed class ParamState
{
    public string Name { get; set; } = "";
    /// bool / enum / multi / string / number / text
    public string Kind { get; set; } = "";
    public string? Short { get; set; }
    public string Description { get; set; } = "";
    public string Display { get; set; } = "";
    public bool IsSet { get; set; }
    public bool Required { get; set; }
    public bool Positional { get; set; }
    public List<ChoiceState> Choices { get; set; } = new();
}

public sealed class Highlight
{
    /// param / extra
    public string Kind { get; set; } = "";
    public int Start { get; set; }
    public int End { get; set; }
}

public sealed class Preview
{
    public string Text { get; set; } = "";
    public List<Highlight> Highlights { get; set; } = new();
    public int Chars { get; set; }
    public int Lines { get; set; }
    public bool CollapsesInClaudeCode { get; set; }
}

public sealed class TemplateInfo
{
    public string Name { get; set; } = "";
    public List<string> Aliases { get; set; } = new();
    public string Description { get; set; } = "";
    public string Category { get; set; } = "";
    public bool Builtin { get; set; }
}

public sealed class EnterAction
{
    /// insert / complete / blocked
    public string Kind { get; set; } = "blocked";
    public Edit? Edit { get; set; }
}

public sealed class Analysis
{
    public List<Token> Tokens { get; set; } = new();
    /// command / params / extra
    public string Phase { get; set; } = "command";
    public TemplateInfo? Template { get; set; }
    public bool Locked { get; set; }
    public List<Candidate> Candidates { get; set; } = new();
    public string? Ghost { get; set; }
    public List<Diagnostic> Diagnostics { get; set; } = new();
    public List<ParamState> Params { get; set; } = new();
    public Preview? Preview { get; set; }
    public string? Canonical { get; set; }
    public EnterAction Enter { get; set; } = new();
    public string Hint { get; set; } = "";

    public static readonly Analysis Empty = new();
}

public sealed class EditResult
{
    public string Text { get; set; } = "";
    public int Caret { get; set; }
}

/// 鼠标对参数的操作（core ParamEdit，`{"kind": ...}` 标签枚举）。
public sealed class ParamEdit
{
    public string Kind { get; set; } = "";
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Value { get; set; }

    public static ParamEdit Toggle() => new() { Kind = "toggle" };
    public static ParamEdit Set(string v) => new() { Kind = "set", Value = v };
    public static ParamEdit ToggleChoice(string v) => new() { Kind = "toggle_choice", Value = v };
    public static ParamEdit Clear() => new() { Kind = "clear" };
    public static ParamEdit Begin() => new() { Kind = "begin" };
}
