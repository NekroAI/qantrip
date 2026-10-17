using Qantrip.Core;
using Qantrip.Platform;

namespace Qantrip.UI;

/// <summary>
/// 面板状态（移植自 macOS LauncherModel.swift）。解析、补全、校验、预览和首屏全部来自 Rust 核心；
/// 这里只管选择与编辑。命令行文本是唯一的状态来源：键盘与鼠标操作都通过改写它生效（D16）。
/// </summary>
internal sealed class LauncherModel
{
    public string Query { get; private set; } = "";
    public Analysis Analysis { get; private set; }
    public int SelectedIndex { get; private set; }
    public Foreground.Target? Target { get; private set; }

    public readonly Engine Engine;
    /// 提交：(最终文本, 是否展开粘贴)
    public Action<string, bool>? OnSubmit;
    public Action? OnCancel;
    /// 指令执行完成（D31）
    public Action<CommandOutcome>? OnCommand;
    /// 读取剪贴板文字（`/new 名称` 不写正文时使用）
    public Func<string?>? ReadClipboard;
    /// 分析结果或选择变化（界面刷新）
    public event Action? Changed;
    /// 需要把输入框改为指定文本与光标位置
    public event Action<string, int>? SetField;
    /// 回车被阻止（抖动反馈）
    public event Action? Rejected;

    /// 需要再按一次回车确认的指令（如 /rm）；输入改变后失效
    public string? ArmedCommand { get; private set; }

    private string? selectedKey;
    private string commandWord = "";
    private bool clipboardLoaded;

    public LauncherModel(Engine engine)
    {
        Engine = engine;
        Analysis = engine.Analyze("", null);
    }

    public List<Candidate> Candidates => Analysis.Candidates;

    public Diagnostic? FirstError =>
        Analysis.Diagnostics.FirstOrDefault(d => d.IsError && !d.AtCursor) ?? Analysis.Diagnostics.FirstOrDefault(d => d.IsError);

    private static string FirstWord(string s) => s.Split((char[]?)null, 2, StringSplitOptions.RemoveEmptyEntries).FirstOrDefault() ?? "";

    /// 面板打开：以当前前台应用取使用记录快照，回到首屏。
    public void Begin(Foreground.Target? target)
    {
        Target = target;
        Engine.BeginSession(string.IsNullOrEmpty(target?.Exe) ? null : target!.Exe);
        selectedKey = null;
        commandWord = "";
        clipboardLoaded = false;
        Engine.SetClipboard(null);
        SetField?.Invoke("", 0);
        TextDidChange("");
    }

    /// 是否处于指令模式（输入以 / 或全角 ／ 开头）
    public bool IsCommandMode
    {
        get
        {
            var t = Query.TrimStart();
            return t.StartsWith('/') || t.StartsWith('／');
        }
    }

    public void TextDidChange(string text)
    {
        Query = text;
        ArmedCommand = null;
        LoadClipboardIfNeeded(text);
        var cmd = FirstWord(text);
        if (cmd != commandWord)
        {
            commandWord = cmd;
            selectedKey = null;
        }
        Reanalyze(resetSelection: true);
    }

    /// `/new 名称` 不写正文时用剪贴板文字：只在输入 /new 时读取剪贴板，且每次唤起只读一次。
    private void LoadClipboardIfNeeded(string text)
    {
        if (clipboardLoaded || !text.TrimStart().StartsWith("/new ")) return;
        clipboardLoaded = true;
        Engine.SetClipboard(ReadClipboard?.Invoke());
    }

    /// 重新解析当前输入（语言或模板库变化后）。
    public void Refresh() => Reanalyze(resetSelection: false);

    private void Reanalyze(bool resetSelection)
    {
        Analysis = Engine.Analyze(Query, selectedKey);
        if (resetSelection) SelectedIndex = 0;
        SelectedIndex = Math.Min(SelectedIndex, Math.Max(Candidates.Count - 1, 0));
        Changed?.Invoke();
    }

    // MARK: 键盘

    public void Move(int delta)
    {
        if (Candidates.Count == 0) return;
        Select((SelectedIndex + delta + Candidates.Count) % Candidates.Count);
    }

    private void Select(int index)
    {
        SelectedIndex = index;
        // 命令阶段 / 首屏：切换高亮项时预览随之切换
        var c = Candidates[index];
        if (!Analysis.Locked && c.Kind is "template" or "invocation")
        {
            selectedKey = c.Key;
            Reanalyze(resetSelection: false);
        }
        else Changed?.Invoke();
    }

    /// Tab：接受高亮的候选，没有候选时接受错误的修正建议。
    public void Complete()
    {
        if (SelectedIndex >= 0 && SelectedIndex < Candidates.Count)
        {
            var c = Candidates[SelectedIndex];
            if (c.Kind == "template") selectedKey = c.Key;
            Apply(c.Edit);
        }
        else if (Analysis.Diagnostics.FirstOrDefault(d => d.Fix != null)?.Fix is Edit fix)
        {
            Apply(fix);
        }
    }

    public void Confirm(bool expand)
    {
        switch (Analysis.Enter.Kind)
        {
            case "insert":
                var text = Engine.Render(Query, selectedKey);
                if (text is null) { Reject(); return; }
                bool collapses = Analysis.Preview?.CollapsesInClaudeCode ?? false;
                if (Settings.RecordUsage) Engine.RecordUse(Query, selectedKey, Target?.Exe);
                OnSubmit?.Invoke(text, expand && collapses);
                break;
            case "complete" when Analysis.Enter.Edit is Edit edit:
                if (!Analysis.Locked) selectedKey = Analysis.Template?.Name;
                Apply(edit);
                break;
            case "run":
                if (Analysis.Enter.Confirm && ArmedCommand != Query)
                {
                    ArmedCommand = Query;
                    Changed?.Invoke();
                    return;
                }
                ArmedCommand = null;
                var r = Engine.RunCommand(Query);
                if (r.Ok && r.Outcome is CommandOutcome o) OnCommand?.Invoke(o);
                else
                {
                    Log.Event("command_failed", new() { ["error"] = r.Error });
                    Reject();
                }
                break;
            default:
                Reject();
                break;
        }
    }

    private void Reject() => Rejected?.Invoke();

    // MARK: 鼠标

    public void Click(int index)
    {
        if (index < 0 || index >= Candidates.Count) return;
        Select(index);
        Complete();
    }

    public void DoubleClick(int index)
    {
        if (index < 0 || index >= Candidates.Count) return;
        var c = Candidates[index];
        if (c.Kind is "template" or "invocation" && !Analysis.Locked)
        {
            Select(index);
            Confirm(expand: false);
        }
        else Click(index);
    }

    public void EditParam(string name, ParamEdit edit)
    {
        var r = Engine.EditParam(Query, selectedKey, name, edit);
        if (r is null) { Reject(); return; }
        if (string.IsNullOrWhiteSpace(Query)) selectedKey = Analysis.Template?.Name;
        SetInput(r.Text, r.Caret);
    }

    /// 光标在末尾时接受灰字补全。
    public bool AcceptGhost()
    {
        if (string.IsNullOrEmpty(Analysis.Ghost)) return false;
        var t = Query + Analysis.Ghost;
        SetInput(t, t.Length);
        return true;
    }

    // MARK: 编辑

    private void Apply(Edit edit)
    {
        if (edit.End > Query.Length || edit.Start > edit.End) return;
        var t = Query[..edit.Start] + edit.Text + Query[edit.End..];
        SetInput(t, edit.Start + edit.Text.Length);
    }

    private void SetInput(string text, int caret)
    {
        SetField?.Invoke(text, caret);
        commandWord = FirstWord(text);
        Query = text;
        ArmedCommand = null;
        LoadClipboardIfNeeded(text);
        Reanalyze(resetSelection: true);
    }
}
