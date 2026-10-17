using System.IO.Compression;
using System.Text;
using Qantrip.Core;
using Qantrip.Platform;

namespace Qantrip.UI;

/// <summary>
/// 模板库状态与文件操作（移植自 macOS LibraryWindow.swift 的 LibraryModel）。模板是用户模板目录中的 Markdown 文件（D11），
/// 这里只做读写、移到回收站与导入导出；解析、校验、序列化都在核心。只在 UI 线程上使用。
/// </summary>
internal sealed class LibraryModel
{
    public readonly Engine Engine;
    public List<TemplateEntry> Entries { get; private set; } = new();
    public string Search { get; set; } = "";
    public string? Selection { get; private set; }
    public EditorModel? Editor { get; private set; }
    /// 列表或选中项变化（重建侧栏与编辑区）
    public event Action? Changed;

    public static string Dir => Settings.TemplatesDir;

    public LibraryModel(Engine engine) => Engine = engine;

    public void Reload()
    {
        Engine.Reload();
        Entries = Engine.Entries();
        // 当前编辑的模板未修改时，跟随磁盘内容刷新（外部编辑器修改、同步拉取）
        if (Editor is { Dirty: false } e && Entries.FirstOrDefault(x => x.Name == e.Entry.Name) is { } fresh
            && (fresh.Source != e.Entry.Source || fresh.Path != e.Entry.Path || fresh.OverridesBuiltin != e.Entry.OverridesBuiltin))
            Editor = new EditorModel(fresh, this);
        Changed?.Invoke();
    }

    public IEnumerable<TemplateEntry> Filtered
    {
        get
        {
            var q = Search.Trim().ToLowerInvariant();
            if (q.Length == 0) return Entries;
            return Entries.Where(e => e.Name.ToLowerInvariant().Contains(q) || e.Description.ToLowerInvariant().Contains(q)
                || e.Aliases.Any(a => a.ToLowerInvariant().Contains(q)) || e.Category.ToLowerInvariant().Contains(q));
        }
    }

    /// 选中模板（调用方负责未保存修改的确认）。
    public void Select(string? name)
    {
        Selection = name;
        Editor = name is null ? null : Entries.FirstOrDefault(e => e.Name == name) is { } entry ? new EditorModel(entry, this) : null;
        Changed?.Invoke();
    }

    // MARK: 文件

    public static string FilePath(string name) => Path.Combine(Dir, name + ".md");

    public TemplateEntry? UserEntry(string name) => Entries.FirstOrDefault(e => e.Name == name && !e.Builtin);

    public string UniqueName(string baseName)
    {
        string n = baseName;
        int i = 2;
        while (Entries.Any(e => e.Name == n) || File.Exists(FilePath(n))) n = $"{baseName}-{i++}";
        return n;
    }

    /// 写入模板文件（UTF-8 无 BOM，先写临时文件再替换）；改名时把旧文件移到回收站。
    public string Write(string source, string name, string? replacing)
    {
        Directory.CreateDirectory(Dir);
        var path = FilePath(name);
        var tmp = path + ".tmp";
        File.WriteAllText(tmp, source, new UTF8Encoding(false));
        File.Move(tmp, path, overwrite: true);
        if (replacing != null && !string.Equals(Path.GetFullPath(replacing), Path.GetFullPath(path), StringComparison.OrdinalIgnoreCase) && File.Exists(replacing))
            RecycleBin.Move(replacing);
        return path;
    }

    public void NewTemplate()
    {
        var name = UniqueName("my-template");
        Write(Engine.SerializeDoc(Engine.NewDoc(name, Settings.UiLocale)), name, null);
        Reload();
        Select(name);
        Log.Event("library", new() { ["action"] = "new" });
    }

    public void Duplicate(TemplateEntry e)
    {
        if (Engine.ParseDoc(e.Source) is not { } doc) return;
        doc.Name = UniqueName(e.Partial ? e.Name : e.Name + "-copy");
        doc.Aliases = new();
        Write(Engine.SerializeDoc(doc), doc.Name, null);
        Reload();
        Select(doc.Name);
        Log.Event("library", new() { ["action"] = "duplicate" });
    }

    /// 自定义内置模板：原样复制源码到用户目录，覆盖内置版本。
    public void Customize(TemplateEntry e)
    {
        Write(e.Source, e.Name, null);
        Reload();
        Select(e.Name);
        Log.Event("library", new() { ["action"] = "customize" });
    }

    /// 删除用户模板（移到回收站）；覆盖内置的模板删除后恢复内置版本。返回是否成功。
    public bool Delete(TemplateEntry e)
    {
        if (e.Path is not { } path) return false;
        bool ok = RecycleBin.Move(path);
        if (Editor != null) Editor.Dirty = false;
        Reload();
        if (e.OverridesBuiltin) Select(e.Name);
        else Select(null);
        Log.Event("library", new() { ["action"] = e.OverridesBuiltin ? "restore" : "delete", ["ok"] = ok });
        return ok;
    }

    // MARK: 导入导出

    /// 把模板文件夹整体导出为 zip（含顶层 templates\ 文件夹，与 macOS 的 ditto --keepParent 相同）。
    public void ExportAll(string zipPath)
    {
        Directory.CreateDirectory(Dir);
        if (File.Exists(zipPath)) File.Delete(zipPath);
        // 只打包模板文件（跳过写入中的临时文件）
        using var zip = ZipFile.Open(zipPath, ZipArchiveMode.Create);
        var top = new DirectoryInfo(Dir).Name;
        foreach (var f in Directory.EnumerateFiles(Dir, "*.md", SearchOption.AllDirectories))
            zip.CreateEntryFromFile(f, top + "/" + Path.GetRelativePath(Dir, f).Replace('\\', '/'), CompressionLevel.Optimal);
        Log.Event("library", new() { ["action"] = "export_all" });
    }

    public void ExportOne(TemplateEntry e, string path) => File.WriteAllText(path, e.Source, new UTF8Encoding(false));

    public enum Conflict { Replace, Rename, Skip }

    public sealed record ImportResult(int Imported, int Skipped, List<string> Failures);

    /// 导入 .md 文件、文件夹或 zip。与已有用户模板重名时调用 ask 决定（返回值与"对其余冲突执行相同操作"）。
    public async Task<ImportResult> Import(IEnumerable<string> paths, Func<string, Task<(Conflict choice, bool sticky)>> ask)
    {
        var files = new List<string>();
        var tmp = Path.Combine(Path.GetTempPath(), "qantrip-import-" + Guid.NewGuid().ToString("N"));
        int imported = 0, skipped = 0;
        var failures = new List<string>();
        try
        {
            foreach (var p in paths)
            {
                try
                {
                    if (Directory.Exists(p)) files.AddRange(MdFiles(p));
                    else if (Path.GetExtension(p).Equals(".zip", StringComparison.OrdinalIgnoreCase))
                    {
                        var dest = Path.Combine(tmp, files.Count.ToString() + "-" + Path.GetFileNameWithoutExtension(p));
                        ZipFile.ExtractToDirectory(p, dest);
                        files.AddRange(MdFiles(dest));
                    }
                    else files.Add(p);
                }
                catch (Exception ex) { Log.Error("import", ex); failures.Add(Path.GetFileName(p)); }
            }
            Conflict? sticky = null;
            foreach (var f in files)
            {
                string source;
                try { source = File.ReadAllText(f); } catch { failures.Add(Path.GetFileName(f)); continue; }
                if (Engine.ParseDoc(source) is not { } doc) { failures.Add(Path.GetFileName(f)); continue; }
                var text = source;
                if (UserEntry(doc.Name) != null)
                {
                    var choice = sticky;
                    if (choice is null)
                    {
                        var (c, s) = await ask(doc.Name);
                        choice = c;
                        if (s) sticky = c;
                    }
                    if (choice == Conflict.Skip) { skipped++; continue; }
                    if (choice == Conflict.Rename)
                    {
                        doc.Name = UniqueName(doc.Name);
                        text = Engine.SerializeDoc(doc);
                    }
                }
                try
                {
                    Write(text, doc.Name, null);
                    imported++;
                    Reload();
                }
                catch (Exception ex) { Log.Error("import_write", ex); failures.Add(Path.GetFileName(f)); }
            }
        }
        finally
        {
            try { if (Directory.Exists(tmp)) Directory.Delete(tmp, true); } catch { }
        }
        Log.Event("library", new() { ["action"] = "import", ["imported"] = imported, ["skipped"] = skipped, ["failed"] = failures.Count });
        return new ImportResult(imported, skipped, failures);
    }

    private static IEnumerable<string> MdFiles(string dir) =>
        Directory.EnumerateFiles(dir, "*.md", SearchOption.AllDirectories).Where(f => !f.Contains("__MACOSX"));
}

/// <summary>
/// 单个模板的编辑状态。表单与源码互相同步：表单修改 → 重新生成源码；源码修改 → 能解析时更新表单。
/// </summary>
internal sealed class EditorModel
{
    public readonly TemplateEntry Entry;
    private readonly LibraryModel library;
    public string Source { get; private set; }
    public TemplateDoc? Doc { get; private set; }
    public SourceCheck Check { get; private set; }
    /// 当前文档序列化后的源码（判断表单编辑是否真的改变了内容）
    private string? docSource;
    public string TryInput { get; private set; } = "";
    public bool Dirty { get; set; }
    public bool SourceMode { get; set; }
    public int? SelectedParam { get; set; }
    public string? SaveError { get; private set; }
    /// 校验结果变化（试用区、标题栏刷新）
    public event Action? Checked;

    public bool ReadOnly => Entry.Builtin;

    public EditorModel(TemplateEntry entry, LibraryModel library)
    {
        Entry = entry;
        this.library = library;
        Source = entry.Source;
        Doc = Engine.ParseDoc(entry.Source);
        docSource = Doc is null ? null : Engine.SerializeDoc(Doc);
        Check = library.Engine.CheckSource(entry.Source, "");
    }

    /// WinUI 文本框用 \r 作换行
    public static string Normalize(string s) => s.Replace("\r\n", "\n").Replace('\r', '\n');

    /// 源码模式下的编辑
    public void SetSource(string s)
    {
        s = Normalize(s);
        if (s == Source) return;
        Source = s;
        Dirty = true;
        if (Engine.ParseDoc(s) is { } d)
        {
            Doc = d;
            docSource = Engine.SerializeDoc(d);
        }
        Recheck();
    }

    /// 表单模式下的编辑
    public void UpdateDoc(Action<TemplateDoc> change)
    {
        if (Doc is null) return;
        change(Doc);
        var s = Engine.SerializeDoc(Doc);
        // 文本框的 TextChanged 对程序赋值也会（异步）触发：文档没有实际变化时不算修改（也不改写手写的源码格式）
        if (s == docSource) return;
        docSource = s;
        Source = s;
        Dirty = true;
        Recheck();
    }

    public void SetTryInput(string s)
    {
        TryInput = s;
        Recheck();
    }

    public void Recheck()
    {
        Check = library.Engine.CheckSource(Source, TryInput);
        Checked?.Invoke();
    }

    public bool CanSave => Dirty && !ReadOnly && Check.Problems.Count == 0 && Check.Name != null;

    /// 保存。成功返回 true（之后模板库会重新加载并重新选中）。
    public bool Save()
    {
        if (!CanSave || Check.Name is not { } name) return false;
        // 改名时不得与其他用户模板重名
        if (name != Entry.Name && library.UserEntry(name) != null)
        {
            SaveError = Loc.T($"已存在名为「{name}」的模板", $"A template named “{name}” already exists");
            Checked?.Invoke();
            return false;
        }
        try
        {
            library.Write(Source, name, Entry.Path);
            SaveError = null;
            Dirty = false;
            library.Reload();
            library.Select(name);
            Log.Event("library", new() { ["action"] = "save", ["renamed"] = name != Entry.Name });
            return true;
        }
        catch (Exception e)
        {
            SaveError = e.Message;
            Checked?.Invoke();
            return false;
        }
    }
}
