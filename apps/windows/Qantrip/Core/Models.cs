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
    /// insert / complete / blocked / run（指令模式，D31）
    public string Kind { get; set; } = "blocked";
    public Edit? Edit { get; set; }
    /// run：需要再按一次回车确认（如 /rm）
    public bool Confirm { get; set; }
}

/// 指令执行结果（core command::CommandOutcome）。
public sealed class CommandOutcome
{
    /// 完成提示
    public string Message { get; set; } = "";
    /// 需要在模板库中打开的模板
    public string? OpenEditor { get; set; }
    /// 需要移到回收站的文件（/rm）
    public string? TrashPath { get; set; }
}

/// qt_run_command 的返回值：成功时带 outcome，失败时带 error。
public sealed class CommandResult
{
    public bool Ok { get; set; }
    public CommandOutcome? Outcome { get; set; }
    public string? Error { get; set; }
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

// MARK: 模板库与编辑器（lib.rs TemplateEntry / SourceCheck，doc.rs TemplateDoc，经 qantrip-ffi 的 JSON 镜像）

/// 模板库中的一项。
public sealed class TemplateEntry
{
    public string Name { get; set; } = "";
    public List<string> Aliases { get; set; } = new();
    public string Description { get; set; } = "";
    public string Category { get; set; } = "";
    public bool Builtin { get; set; }
    /// 用户模板覆盖了同名内置模板
    public bool OverridesBuiltin { get; set; }
    public bool Partial { get; set; }
    public string? Path { get; set; }
    public string Source { get; set; } = "";
}

public sealed class ChoiceDoc
{
    public string Value { get; set; } = "";
    public string Label { get; set; } = "";
    public List<string> Aliases { get; set; } = new();
}

/// 参数的结构化表示（core doc.rs ParamDoc）。
public sealed class ParamDoc
{
    public string Name { get; set; } = "";
    /// bool / enum / multi / string / number / text
    public string Kind { get; set; } = "bool";
    public string Description { get; set; } = "";
    public List<ChoiceDoc> Choices { get; set; } = new();
    /// 默认值的文本形式：开关为 true / false，多选用逗号分隔，未设置为 null
    public string? DefaultValue { get; set; }
    public bool Positional { get; set; }
    public string? Short { get; set; }
    public bool Required { get; set; }
    public List<string> Requires { get; set; } = new();
    public List<string> Conflicts { get; set; } = new();
    public double? Min { get; set; }
    public double? Max { get; set; }
}

/// 模板的结构化表示（图形化编辑器，core doc.rs TemplateDoc）。
public sealed class TemplateDoc
{
    public string Name { get; set; } = "";
    public List<string> Aliases { get; set; } = new();
    public string Description { get; set; } = "";
    public string Category { get; set; } = "";
    public List<ParamDoc> Params { get; set; } = new();
    public string Body { get; set; } = "";
}

/// 编辑器的实时校验与试用结果。
public sealed class SourceCheck
{
    public string? Name { get; set; }
    public List<string> Problems { get; set; } = new();
    public TemplateDoc? Doc { get; set; }
    /// 以试用输入解析的结果（预览、参数状态等）
    public Analysis? Analysis { get; set; }
}
