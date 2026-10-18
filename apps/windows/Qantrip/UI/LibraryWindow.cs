using System.Diagnostics;
using System.Globalization;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Qantrip.Core;
using Qantrip.Platform;
using Windows.ApplicationModel.DataTransfer;
using Windows.Graphics;
using Windows.Storage;
using Windows.Storage.Pickers;
using Windows.System;
using static Qantrip.Platform.Win32;

namespace Qantrip.UI;

/// <summary>
/// 模板库与编辑器窗口（PRD 4.5 / 11.2，docs/decisions.md D19；移植自 macOS LibraryWindow.swift）。
/// 左侧：搜索 + 我的模板 / 内置（按分类）/ 公共片段，底部固定"设置"入口；右侧：表单或源码编辑 + 试用预览（核心 check_source 实时校验），
/// 选中"设置"时显示设置页（<see cref="SettingsView"/>，与 macOS 相同：设置与模板库在同一个窗口中）。
/// 控件带 AutomationId，回归脚本通过 UI 自动化操作（tests/windows/t-library.ps1）。
/// </summary>
internal sealed class LibraryWindow : Window
{
    private readonly LibraryModel model;
    private readonly App app;
    private readonly IntPtr hwnd;
    private readonly Grid root = new();
    // 侧栏与详情区在语言切换时整体重建（WinUI 中不能把已在界面树中的元素移到新的父元素）
    private TextBox search = new();
    private ListView list = new();
    private Grid detail = new();
    private StackPanel tools = new();
    private Button? settingsButton;
    private TextBlock? titleLabel;
    /// 选中设置页时的设置界面（离开设置页时释放）
    private SettingsView? settingsView;
    /// 最近一次选中的模板（从设置回到模板库时恢复）
    private string? lastTemplate;
    private bool suppressSelection;
    private bool dialogOpen;

    // 编辑区
    private EditorModel? editor;
    private TextBlock? titleText, statusText;
    private Button? saveButton;
    private ContentControl editHost = new();
    // 试用区（每次构建编辑区时新建：WinUI 中把元素移到新父元素容易失败）
    private StackPanel tryResults = new() { Spacing = 10 };
    private TextBox tryInput = new();
    private TextBox? sourceBox;
    /// 当前参数检查器中参数标题的文本（表单修改时就地更新）
    private readonly Dictionary<int, TextBlock> paramTitles = new();

    public LibraryWindow(LibraryModel model, App app)
    {
        this.model = model;
        this.app = app;
        hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        ExtendsContentIntoTitleBar = true;
        if (MicaController.IsSupported()) SystemBackdrop = new MicaBackdrop();
        else root.Background = SettingsView.Res("SolidBackgroundFillColorBaseBrush");
        var icon = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        if (File.Exists(icon)) AppWindow.SetIcon(icon);
        Place();
        Content = root;
        model.Changed += Rebuild;
        Closed += (_, _) =>
        {
            model.Changed -= Rebuild;
            settingsView?.Detach();
            settingsView = null;
        };
        Build();
    }

    /// 在鼠标所在显示器居中，按该显示器的缩放计算尺寸。
    private void Place()
    {
        GetCursorPos(out var pt);
        var mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        var mi = new MONITORINFO { cbSize = System.Runtime.InteropServices.Marshal.SizeOf<MONITORINFO>() };
        GetMonitorInfo(mon, ref mi);
        GetDpiForMonitor(mon, 0, out uint dpi, out _);
        double scale = dpi == 0 ? 1 : dpi / 96.0;
        var wa = mi.rcWork;
        int w = (int)Math.Min(Math.Round(1240 * scale), (wa.Right - wa.Left) * 0.92), h = (int)Math.Min(Math.Round(800 * scale), (wa.Bottom - wa.Top) * 0.9);
        AppWindow.MoveAndResize(new RectInt32(wa.Left + (wa.Right - wa.Left - w) / 2, wa.Top + (wa.Bottom - wa.Top - h) / 2, w, h));
    }

    public void ShowAndActivate()
    {
        AppWindow.Show(true);
        Activate();
        Foreground.Activate(hwnd, out _);
    }

    /// 打开时选中某个模板（/edit、/dup）。有未保存修改时先确认。
    public async void SelectTemplate(string name)
    {
        if (model.Selection == name) return;
        if (!await ConfirmDiscard()) return;
        model.Select(name);
    }

    /// 打开设置页（托盘"设置…"、面板 Ctrl+,、/settings、同步提醒）。有未保存修改时先确认。
    public async void ShowSettings(string page)
    {
        var key = SettingsView.Key(SettingsView.PageOf(SettingsView.Key(page)) ?? "general");
        if (model.Selection == key) return;
        if (!await ConfirmDiscard()) return;
        model.Select(key);
    }

    /// 正显示设置时回到模板库（恢复最近选中的模板）。
    public void ShowTemplates()
    {
        if (SettingsView.PageOf(model.Selection) is null) return;
        model.Select(lastTemplate is { } t && model.Entries.Any(e => LibraryModel.Key(e) == t) ? t : null);
    }

    /// 界面语言变化：整个窗口重建（推迟到当前事件处理结束，避免在设置页的下拉框事件中移除它自己）。
    public void ApplyLanguage() => DispatcherQueue.TryEnqueue(() =>
    {
        var offset = settingsView?.ScrollOffset ?? 0;
        settingsView?.Detach();
        settingsView = null;
        editorBuilt = false;
        root.Children.Clear();
        root.RowDefinitions.Clear();
        Build();
        if (offset > 0) settingsView?.RestoreOffset(offset);
    });

    /// 排除的程序列表变化（设置或托盘菜单修改）
    public void RefreshExcludedApps() => settingsView?.RefreshExcludedApps();

    // MARK: 构建

    private void Build()
    {
        titleLabel = new TextBlock { FontSize = 12, VerticalAlignment = VerticalAlignment.Center };
        search = new TextBox { Text = model.Search };
        list = new ListView();
        detail = new Grid();
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(40) });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var titleBar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10, Padding = new Thickness(16, 0, 0, 0), VerticalAlignment = VerticalAlignment.Center };
        var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        if (File.Exists(iconPath)) titleBar.Children.Add(new Image { Source = new BitmapImage(new Uri(iconPath)), Width = 16, Height = 16 });
        titleBar.Children.Add(titleLabel);
        var titleHost = new Grid { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
        titleHost.Children.Add(titleBar);
        root.Children.Add(titleHost);
        SetTitleBar(titleHost);

        var body = new Grid();
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(290) });
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetRow(body, 1);
        root.Children.Add(body);

        // 侧栏
        var side = new Grid { Padding = new Thickness(12, 0, 8, 12) };
        side.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        side.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        side.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        side.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        // 模板操作（新建 / 导入 / 导出）；显示设置时隐藏
        tools = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, Margin = new Thickness(0, 0, 0, 8) };
        tools.Children.Add(ToolButton("lib_new", "", Loc.T("新建", "New"), Loc.T("新建模板", "New template"), () => { _ = NewTemplate(); }));
        tools.Children.Add(ToolButton("lib_import", "", Loc.T("导入", "Import"), Loc.T("导入模板（.md、zip）", "Import templates (.md, zip)"), () => { _ = ImportFiles(); }));
        tools.Children.Add(ToolButton("lib_export", "", Loc.T("导出全部", "Export All"), Loc.T("把我的模板导出为 zip", "Export my templates as a zip"), () => { _ = ExportAll(); }));
        side.Children.Add(tools);
        search.PlaceholderText = Loc.T("搜索模板", "Search templates");
        AutomationProperties.SetAutomationId(search, "lib_search");
        AutomationProperties.SetName(search, search.PlaceholderText);
        search.TextChanged += (_, _) => { model.Search = search.Text; RebuildList(); };
        search.Margin = new Thickness(0, 0, 0, 8);
        Grid.SetRow(search, 1);
        side.Children.Add(search);
        list.SelectionMode = ListViewSelectionMode.Single;
        AutomationProperties.SetAutomationId(list, "lib_list");
        list.SelectionChanged += OnListSelection;
        Grid.SetRow(list, 2);
        side.Children.Add(list);
        // 设置：侧栏底部固定入口
        settingsButton = SettingsEntry();
        Grid.SetRow(settingsButton, 3);
        side.Children.Add(settingsButton);
        body.Children.Add(side);

        detail.Background = SettingsView.Res("LayerFillColorDefaultBrush");
        detail.BorderBrush = SettingsView.Res("CardStrokeColorDefaultBrush");
        detail.BorderThickness = new Thickness(1, 1, 0, 0);
        detail.CornerRadius = new CornerRadius(8, 0, 0, 0);
        Grid.SetColumn(detail, 1);
        body.Children.Add(detail);

        Rebuild();
        if (dropHooked) return;
        dropHooked = true;

        // 拖入文件导入（root 跨语言切换保留，只挂一次）
        root.AllowDrop = true;
        root.DragOver += (_, e) => { if (e.DataView.Contains(StandardDataFormats.StorageItems)) e.AcceptedOperation = DataPackageOperation.Copy; };
        root.Drop += async (_, e) =>
        {
            if (!e.DataView.Contains(StandardDataFormats.StorageItems)) return;
            var deferral = e.GetDeferral();
            IReadOnlyList<IStorageItem> items;
            try { items = await e.DataView.GetStorageItemsAsync(); }
            finally { deferral.Complete(); }
            await RunImport(items.Select(i => i.Path).Where(p => !string.IsNullOrEmpty(p)).ToList());
        };
    }

    private bool dropHooked;

    private static Button ToolButton(string id, string glyph, string text, string tip, Action click)
    {
        var sp = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        sp.Children.Add(new FontIcon { Glyph = glyph, FontFamily = Theme.Icons, FontSize = 14 });
        sp.Children.Add(new TextBlock { Text = text, FontSize = 12 });
        var b = new Button { Content = sp, Padding = new Thickness(8, 4, 8, 4) };
        ToolTipService.SetToolTip(b, tip);
        AutomationProperties.SetAutomationId(b, id);
        AutomationProperties.SetName(b, text);
        b.Click += (_, _) => click();
        return b;
    }

    /// 侧栏底部的"设置"入口：选中时高亮
    private Button SettingsEntry()
    {
        var sp = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        sp.Children.Add(new FontIcon { Glyph = "\uE713", FontFamily = Theme.Icons, FontSize = 16 });
        sp.Children.Add(new TextBlock { Text = Loc.T("设置", "Settings"), FontSize = 14 });
        var b = new Button
        {
            Content = sp, HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Left,
            Padding = new Thickness(12, 8, 12, 8), Margin = new Thickness(0, 8, 0, 0), BorderThickness = new Thickness(0),
        };
        ToolTipService.SetToolTip(b, Loc.T("设置（通用、插入、同步、隐私、关于）", "Settings (General, Insert, Sync, Privacy, About)"));
        AutomationProperties.SetAutomationId(b, "lib_settings");
        AutomationProperties.SetName(b, Loc.T("设置", "Settings"));
        b.Click += (_, _) => { if (SettingsView.PageOf(model.Selection) is null) ShowSettings("general"); };
        return b;
    }

    private void Rebuild()
    {
        RebuildList();
        var page = SettingsView.PageOf(model.Selection);
        tools.Visibility = page is null ? Visibility.Visible : Visibility.Collapsed;
        // 窗口标题随所在部分变化（任务栏与 Alt+Tab 中可区分）
        Title = page is null ? Loc.T("模板库", "Template Library") : Loc.T("Nekro Qantrip 设置", "Nekro Qantrip Settings");
        if (titleLabel != null) titleLabel.Text = Title;
        if (settingsButton != null)
            settingsButton.Background = page is null ? new SolidColorBrush(Microsoft.UI.Colors.Transparent) : new SolidColorBrush(Theme.WithAlpha(new Theme(root.ActualTheme == ElementTheme.Dark).Accent, 0.22));
        if (page != null)
        {
            if (settingsView is null) BuildSettings();
            settingsView!.Show(page);
            return;
        }
        if (model.Selection != null) lastTemplate = model.Selection;
        if (settingsView != null)
        {
            settingsView.Detach();
            settingsView = null;
            editorBuilt = false;
        }
        if (!editorBuilt || !ReferenceEquals(editor, model.Editor)) BuildEditor();
    }

    /// 详情区显示设置页
    private void BuildSettings()
    {
        editor = null;
        editorBuilt = false;
        sourceBox = null;
        paramTitles.Clear();
        detail.Children.Clear();
        detail.RowDefinitions.Clear();
        settingsView = new SettingsView(app, hwnd, root, p => ShowSettings(p));
        detail.Children.Add(settingsView.View);
    }

    private bool editorBuilt;

    // MARK: 侧栏

    private void RebuildList()
    {
        suppressSelection = true;
        try
        {
            list.Items.Clear();
            var items = model.Filtered.ToList();
            var groups = new (string title, List<TemplateEntry> items, bool byCategory)[]
            {
                (Loc.T("我的模板", "My templates"), items.Where(e => !e.Builtin && !e.Partial).ToList(), false),
                (Loc.T("内置", "Built-in"), items.Where(e => e.Builtin && !e.Partial && !e.Deleted).ToList(), true),
                (Loc.T("公共片段", "Shared snippets"), items.Where(e => e.Partial).ToList(), false),
                (Loc.T("已删除", "Deleted"), items.Where(e => e.Deleted).ToList(), false),
            };
            ListViewItem? selected = null;
            foreach (var (title, entries, byCategory) in groups)
            {
                if (entries.Count == 0) continue;
                list.Items.Add(Header(title, top: list.Items.Count > 0));
                string? category = null;
                foreach (var e in entries)
                {
                    if (byCategory && e.Category != category && !string.IsNullOrEmpty(e.Category))
                    {
                        category = e.Category;
                        list.Items.Add(SubHeader(category));
                    }
                    var item = Row(e);
                    list.Items.Add(item);
                    if (LibraryModel.Key(e) == model.Selection) selected = item;
                }
            }
            list.SelectedItem = selected;
        }
        finally { suppressSelection = false; }
    }

    private static ListViewItem Header(string text, bool top) => new()
    {
        Content = new TextBlock { Text = text, FontWeight = FontWeights.SemiBold, FontSize = 13, Margin = new Thickness(0, top ? 10 : 0, 0, 2) },
        IsHitTestVisible = false, IsTabStop = false, MinHeight = 28,
    };

    private static ListViewItem SubHeader(string text) => new()
    {
        Content = new TextBlock { Text = text, FontSize = 11, Foreground = SettingsView.Res("TextFillColorTertiaryBrush") },
        IsHitTestVisible = false, IsTabStop = false, MinHeight = 22, Padding = new Thickness(14, 2, 0, 0),
    };

    private ListViewItem Row(TemplateEntry e)
    {
        var sp = new StackPanel { Spacing = 1, Padding = new Thickness(0, 4, 0, 4) };
        var top = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        top.Children.Add(new TextBlock { Text = e.Name, FontFamily = Theme.Mono, FontWeight = FontWeights.SemiBold, FontSize = 13 });
        if (e.Aliases.Count > 0)
            top.Children.Add(new TextBlock { Text = string.Join(" · ", e.Aliases), FontSize = 11, VerticalAlignment = VerticalAlignment.Center, Foreground = SettingsView.Res("TextFillColorSecondaryBrush") });
        if (e.OverridesBuiltin)
            top.Children.Add(new Border
            {
                Child = new TextBlock { Text = Loc.T("已自定义", "Customized"), FontSize = 9, FontWeight = FontWeights.SemiBold },
                Padding = new Thickness(5, 1, 5, 1), CornerRadius = new CornerRadius(8), VerticalAlignment = VerticalAlignment.Center,
                Background = new SolidColorBrush(Theme.WithAlpha(new Theme(false).Accent, 0.2)),
            });
        sp.Children.Add(top);
        if (!string.IsNullOrEmpty(e.Description))
            sp.Children.Add(new TextBlock { Text = e.Description, FontSize = 11, TextTrimming = TextTrimming.CharacterEllipsis, Foreground = SettingsView.Res("TextFillColorSecondaryBrush") });
        var item = new ListViewItem { Content = sp, Tag = e };
        AutomationProperties.SetAutomationId(item, "lib_item_" + (e.Deleted ? "deleted_" : "") + e.Name);
        AutomationProperties.SetName(item, e.Name);
        item.ContextFlyout = Menu(e);
        return item;
    }

    private MenuFlyout Menu(TemplateEntry e)
    {
        var m = new MenuFlyout();
        void Add(string text, Action click)
        {
            var i = new MenuFlyoutItem { Text = text };
            i.Click += (_, _) => click();
            m.Items.Add(i);
        }
        Add(Loc.T("复制为新模板", "Duplicate as New Template"), () => _ = Guarded(() => model.Duplicate(e)));
        Add(Loc.T("导出…", "Export…"), () => _ = ExportOne(e));
        if (e.Path is { } path && !e.Deleted)
            Add(Loc.T("在资源管理器中显示", "Show in File Explorer"), () => ShowInExplorer(path));
        if (!e.Partial)
        {
            m.Items.Add(new MenuFlyoutSeparator());
            if (e.Deleted || e.OverridesBuiltin) Add(Loc.T("恢复内置版本", "Restore Built-in Version"), () => _ = Restore(e));
            if (!e.Deleted) Add(Loc.T("删除", "Delete"), () => _ = Delete(e));
        }
        return m;
    }

    private static void ShowInExplorer(string path)
    {
        try { Process.Start("explorer.exe", $"/select,\"{path}\""); }
        catch (Exception ex) { Log.Error("explorer", ex); }
    }

    private async void OnListSelection(object sender, SelectionChangedEventArgs e)
    {
        if (suppressSelection) return;
        var name = (list.SelectedItem as ListViewItem)?.Tag is TemplateEntry t ? LibraryModel.Key(t) : null;
        if (name is null || name == model.Selection) return;
        if (editor is { Dirty: true })
        {
            // 先恢复原选中项，确认放弃修改后再切换
            RebuildList();
            if (!await ConfirmDiscard()) return;
        }
        model.Select(name);
    }

    private async Task<bool> ConfirmDiscard()
    {
        if (editor is not { Dirty: true } e) return true;
        var r = await Dialog(new ContentDialog
        {
            Title = Loc.T($"放弃对「{e.Doc?.Name ?? e.Entry.Name}」的修改？", $"Discard changes to “{e.Doc?.Name ?? e.Entry.Name}”?"),
            PrimaryButtonText = Loc.T("放弃修改", "Discard Changes"),
            CloseButtonText = Loc.T("取消", "Cancel"),
            DefaultButton = ContentDialogButton.Close,
        });
        if (r != ContentDialogResult.Primary) return false;
        e.Dirty = false;
        return true;
    }

    // MARK: 编辑区

    private void BuildEditor()
    {
        editorBuilt = true;
        editor = model.Editor;
        detail.Children.Clear();
        detail.RowDefinitions.Clear();
        paramTitles.Clear();
        sourceBox = null;
        if (editor is null)
        {
            var empty = new StackPanel { Spacing = 10, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, MaxWidth = 420 };
            empty.Children.Add(new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 40, Foreground = SettingsView.Res("TextFillColorTertiaryBrush") });
            empty.Children.Add(new TextBlock { Text = Loc.T("选择或新建一个模板", "Select or create a template"), FontFamily = Theme.Display, FontSize = 22, HorizontalAlignment = HorizontalAlignment.Center });
            empty.Children.Add(new TextBlock
            {
                Text = Loc.T("模板保存为 Markdown 文件，也可以用任意编辑器直接修改。", "Templates are saved as Markdown files. You can also edit them in any editor."),
                TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center, Foreground = SettingsView.Res("TextFillColorSecondaryBrush"),
            });
            detail.Children.Add(empty);
            return;
        }
        var ed = editor;
        ed.Checked += () => { if (ReferenceEquals(ed, editor)) { UpdateHeader(); RenderTry(); } };

        detail.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        detail.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        detail.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        // 标题栏：名称、状态、表单 / 源码、保存、更多
        var header = new Grid { Padding = new Thickness(20, 12, 16, 12), ColumnSpacing = 10 };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var titles = new StackPanel { Spacing = 0, VerticalAlignment = VerticalAlignment.Center };
        titleText = new TextBlock { FontFamily = Theme.Display, FontSize = 22, TextTrimming = TextTrimming.CharacterEllipsis };
        AutomationProperties.SetAutomationId(titleText, "lib_title");
        statusText = new TextBlock { FontSize = 12, Foreground = SettingsView.Res("TextFillColorSecondaryBrush") };
        AutomationProperties.SetAutomationId(statusText, "lib_status");
        titles.Children.Add(titleText);
        titles.Children.Add(statusText);
        header.Children.Add(titles);

        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
        var modes = new SelectorBar();
        var formItem = new SelectorBarItem { Text = Loc.T("表单", "Form"), Tag = "form" };
        var sourceItem = new SelectorBarItem { Text = Loc.T("源码", "Source"), Tag = "source" };
        AutomationProperties.SetAutomationId(formItem, "lib_mode_form");
        AutomationProperties.SetAutomationId(sourceItem, "lib_mode_source");
        modes.Items.Add(formItem);
        modes.Items.Add(sourceItem);
        modes.SelectedItem = ed.SourceMode ? sourceItem : formItem;
        modes.SelectionChanged += (_, _) =>
        {
            ed.SourceMode = ReferenceEquals(modes.SelectedItem, sourceItem);
            BuildEditPane();
        };
        actions.Children.Add(modes);
        saveButton = new Button { Content = Loc.T("保存", "Save"), Style = (Style)Application.Current.Resources["AccentButtonStyle"] };
        AutomationProperties.SetAutomationId(saveButton, "lib_save");
        saveButton.KeyboardAccelerators.Add(new KeyboardAccelerator { Key = VirtualKey.S, Modifiers = VirtualKeyModifiers.Control });
        saveButton.Click += (_, _) => Save();
        actions.Children.Add(saveButton);
        var more = new DropDownButton { Content = new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 14 }, Flyout = MoreMenu(ed.Entry) };
        AutomationProperties.SetAutomationId(more, "lib_more");
        AutomationProperties.SetName(more, Loc.T("更多操作", "More actions"));
        actions.Children.Add(more);
        Grid.SetColumn(actions, 1);
        header.Children.Add(actions);
        detail.Children.Add(header);

        // 编辑 | 试用
        var split = new Grid();
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star), MinWidth = 420 });
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(380) });
        Grid.SetRow(split, 1);
        editHost = new ContentControl { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch };
        split.Children.Add(editHost);
        var tryScroll = new ScrollViewer { Content = BuildTryPanel(), HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled, Padding = new Thickness(14, 0, 14, 14) };
        var tryBorder = new Border { Child = tryScroll, BorderThickness = new Thickness(1, 0, 0, 0), BorderBrush = SettingsView.Res("CardStrokeColorDefaultBrush") };
        Grid.SetColumn(tryBorder, 1);
        split.Children.Add(tryBorder);
        detail.Children.Add(split);

        if (ed.Entry.Deleted)
        {
            var banner = new Grid { Padding = new Thickness(16, 8, 16, 8), ColumnSpacing = 10, Background = new SolidColorBrush(Theme.WithAlpha(Microsoft.UI.Colors.Orange, 0.12)) };
            banner.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            banner.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            banner.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            banner.Children.Add(new FontIcon { Glyph = "\uE74D", FontFamily = Theme.Icons, FontSize = 14 });
            var msg = new TextBlock
            {
                Text = Loc.T("这个内置模板已删除，它的名称和别名可以给你自己的模板使用。", "This built-in template is deleted; its name and aliases are free for your own templates."),
                TextWrapping = TextWrapping.Wrap, FontSize = 13, VerticalAlignment = VerticalAlignment.Center,
            };
            Grid.SetColumn(msg, 1);
            banner.Children.Add(msg);
            var restore = new Button { Content = Loc.T("恢复", "Restore") };
            AutomationProperties.SetAutomationId(restore, "lib_restore");
            restore.Click += (_, _) => _ = Restore(ed.Entry);
            Grid.SetColumn(restore, 2);
            banner.Children.Add(restore);
            Grid.SetRow(banner, 2);
            detail.Children.Add(banner);
        }
        else if (ed.ReadOnly)
        {
            var banner = new Grid { Padding = new Thickness(16, 8, 16, 8), ColumnSpacing = 10, Background = new SolidColorBrush(Theme.WithAlpha(new Theme(false).Accent, 0.10)) };
            banner.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            banner.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            banner.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            banner.Children.Add(new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 14 });
            var msg = new TextBlock
            {
                Text = Loc.T("内置模板为只读。自定义后会保存为你自己的模板并覆盖内置版本，随时可以恢复。", "Built-in templates are read-only. Customizing saves your own copy that overrides the built-in version; you can restore it anytime."),
                TextWrapping = TextWrapping.Wrap, FontSize = 13, VerticalAlignment = VerticalAlignment.Center,
            };
            Grid.SetColumn(msg, 1);
            banner.Children.Add(msg);
            var bs = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
            var customize = new Button { Content = Loc.T("自定义此模板", "Customize This Template") };
            AutomationProperties.SetAutomationId(customize, "lib_customize");
            customize.Click += (_, _) => _ = Guarded(() => model.Customize(ed.Entry));
            var dup = new Button { Content = Loc.T("复制为新模板", "Duplicate as New Template") };
            AutomationProperties.SetAutomationId(dup, "lib_duplicate");
            dup.Click += (_, _) => _ = Guarded(() => model.Duplicate(ed.Entry));
            bs.Children.Add(customize);
            bs.Children.Add(dup);
            Grid.SetColumn(bs, 2);
            banner.Children.Add(bs);
            Grid.SetRow(banner, 2);
            detail.Children.Add(banner);
        }

        BuildEditPane();
        UpdateHeader();
        RenderTry();
    }

    private MenuFlyout MoreMenu(TemplateEntry e)
    {
        var m = new MenuFlyout();
        void Add(string id, string text, string glyph, Action click)
        {
            var i = new MenuFlyoutItem { Text = text, Icon = new FontIcon { Glyph = glyph, FontFamily = Theme.Icons } };
            AutomationProperties.SetAutomationId(i, id);
            i.Click += (_, _) => click();
            m.Items.Add(i);
        }
        Add("lib_menu_duplicate", Loc.T("复制为新模板", "Duplicate as New Template"), "", () => _ = Guarded(() => model.Duplicate(e)));
        if (e.Builtin && !e.Deleted) Add("lib_menu_customize", Loc.T("自定义此模板", "Customize This Template"), "", () => _ = Guarded(() => model.Customize(e)));
        Add("lib_menu_export", Loc.T("导出…", "Export…"), "", () => _ = ExportOne(e));
        if (e.Path is { } path && !e.Deleted)
            Add("lib_menu_reveal", Loc.T("在资源管理器中显示", "Show in File Explorer"), "", () => ShowInExplorer(path));
        if (!e.Partial)
        {
            m.Items.Add(new MenuFlyoutSeparator());
            if (e.Deleted || e.OverridesBuiltin) Add("lib_menu_restore", Loc.T("恢复内置版本", "Restore Built-in Version"), "", () => _ = Restore(e));
            if (!e.Deleted) Add("lib_menu_delete", Loc.T("删除", "Delete"), "", () => _ = Delete(e));
        }
        return m;
    }

    private void UpdateHeader()
    {
        if (editor is not { } ed || titleText is null || statusText is null || saveButton is null) return;
        titleText.Text = ed.Doc?.Name ?? ed.Entry.Name;
        statusText.Text = ed.SaveError ?? (ed.Dirty ? Loc.T("未保存", "Unsaved")
            : ed.Entry.Deleted ? Loc.T("已删除的内置模板", "Deleted built-in template")
            : ed.ReadOnly ? Loc.T("内置模板", "Built-in template")
            : ed.Entry.OverridesBuiltin ? Loc.T("已自定义的内置模板", "Customized built-in template") : (ed.Entry.Path ?? ""));
        saveButton.IsEnabled = ed.CanSave;
        saveButton.Visibility = ed.ReadOnly ? Visibility.Collapsed : Visibility.Visible;
    }

    private void Save()
    {
        if (editor is not { } ed || !ed.CanSave) return;
        ed.Save();
        UpdateHeader();
    }

    private void BuildEditPane()
    {
        if (editor is not { } ed) return;
        paramTitles.Clear();
        if (ed.SourceMode)
        {
            sourceBox = CodeBox(ed.Source, ed.ReadOnly, "lib_source");
            var box = sourceBox;
            box.TextChanged += (_, _) => { if (ReferenceEquals(ed, editor)) ed.SetSource(box.Text); };
            editHost.Content = new Border { Child = sourceBox, Padding = new Thickness(16, 0, 16, 16) };
            return;
        }
        sourceBox = null;
        if (ed.Doc is null)
        {
            var sp = new StackPanel { Spacing = 8, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, MaxWidth = 380 };
            sp.Children.Add(new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 32 });
            sp.Children.Add(new TextBlock { Text = Loc.T("源码无法解析", "Source can’t be parsed"), FontSize = 18, HorizontalAlignment = HorizontalAlignment.Center });
            sp.Children.Add(new TextBlock { Text = Loc.T("请在「源码」中修正格式错误后再使用表单。", "Fix the errors in Source before using the form."), TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center });
            editHost.Content = sp;
            return;
        }
        editHost.Content = new ScrollViewer { Content = BuildForm(ed), HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
    }

    private static TextBox CodeBox(string text, bool readOnly, string id)
    {
        var t = new TextBox
        {
            Text = text, AcceptsReturn = true, TextWrapping = TextWrapping.NoWrap, FontFamily = Theme.Mono, FontSize = 13, IsReadOnly = readOnly,
            IsSpellCheckEnabled = false, IsTextPredictionEnabled = false, VerticalAlignment = VerticalAlignment.Stretch,
        };
        ScrollViewer.SetHorizontalScrollBarVisibility(t, ScrollBarVisibility.Auto);
        ScrollViewer.SetVerticalScrollBarVisibility(t, ScrollBarVisibility.Auto);
        AutomationProperties.SetAutomationId(t, id);
        return t;
    }

    // MARK: 表单

    private static TextBlock Caption(string text) => new() { Text = text, FontSize = 12, TextWrapping = TextWrapping.Wrap, Foreground = SettingsView.Res("TextFillColorSecondaryBrush") };

    private static Border Card(UIElement child) => new()
    {
        Child = child, Padding = new Thickness(16, 12, 16, 14), CornerRadius = new CornerRadius(6), BorderThickness = new Thickness(1),
        Background = SettingsView.Res("CardBackgroundFillColorDefaultBrush"), BorderBrush = SettingsView.Res("CardStrokeColorDefaultBrush"),
    };

    private TextBox Field(string id, string header, string text, bool readOnly, Action<string> changed, string? placeholder = null, bool mono = false)
    {
        var t = new TextBox { Header = header, Text = text, IsReadOnly = readOnly, IsSpellCheckEnabled = false, PlaceholderText = placeholder ?? "" };
        if (mono) t.FontFamily = Theme.Mono;
        AutomationProperties.SetAutomationId(t, id);
        AutomationProperties.SetName(t, header);
        t.TextChanged += (_, _) => { if (!readOnly) changed(t.Text); };
        return t;
    }

    private static List<string> SplitList(string s) => s.Split(',', '，').Select(x => x.Trim()).Where(x => x.Length > 0).ToList();

    private FrameworkElement BuildForm(EditorModel ed)
    {
        var doc = ed.Doc!;
        bool ro = ed.ReadOnly;
        var page = new StackPanel { Spacing = 8, Padding = new Thickness(20, 0, 20, 24) };

        page.Children.Add(SettingsView.Section(Loc.T("基本信息", "Basics")));
        var basics = new Grid { ColumnSpacing = 12, RowSpacing = 10 };
        basics.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        basics.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        basics.RowDefinitions.Add(new RowDefinition());
        basics.RowDefinitions.Add(new RowDefinition());
        void Place(FrameworkElement e, int r, int c) { Grid.SetRow(e, r); Grid.SetColumn(e, c); basics.Children.Add(e); }
        Place(Field("lib_name", Loc.T("名称", "Name"), doc.Name, ro, v => ed.UpdateDoc(d => d.Name = v.Trim()), mono: true), 0, 0);
        Place(Field("lib_aliases", Loc.T("别名", "Aliases"), string.Join(", ", doc.Aliases), ro, v => ed.UpdateDoc(d => d.Aliases = SplitList(v)), Loc.T("用逗号分隔，如 r, rev", "Separate with commas, e.g. r, rev")), 0, 1);
        Place(Field("lib_description", Loc.T("描述", "Description"), doc.Description, ro, v => ed.UpdateDoc(d => d.Description = v)), 1, 0);
        Place(Field("lib_category", Loc.T("分类", "Category"), doc.Category, ro, v => ed.UpdateDoc(d => d.Category = v), Loc.T("如 编程、日常", "e.g. Coding, Daily")), 1, 1);
        page.Children.Add(Card(basics));

        page.Children.Add(SettingsView.Section(Loc.T("参数", "Parameters")));
        var paramsPanel = new StackPanel { Spacing = 6 };
        for (int i = 0; i < doc.Params.Count; i++) paramsPanel.Children.Add(ParamRow(ed, i));
        if (!ro)
        {
            var add = new Button { Content = Loc.T("＋ 添加参数", "+ Add Parameter") };
            AutomationProperties.SetAutomationId(add, "lib_addparam");
            add.Click += (_, _) =>
            {
                ed.UpdateDoc(d =>
                {
                    string n = "param";
                    int k = 2;
                    while (d.Params.Any(p => p.Name == n)) n = $"param{k++}";
                    d.Params.Add(new ParamDoc { Name = n, Kind = "bool" });
                });
                ed.SelectedParam = doc.Params.Count - 1;
                BuildEditPane();
            };
            paramsPanel.Children.Add(add);
        }
        paramsPanel.Children.Add(Caption(Loc.T("参数顺序即命令补全和规范形式中的顺序。至多一个单选 / 多选参数可设为位置参数（可直接输入候选值，如 r security）。",
            "Parameter order determines the order in completion and the canonical form. At most one single- or multi-choice parameter can be positional (its values can be typed directly, e.g. r security).")));
        page.Children.Add(paramsPanel);

        page.Children.Add(SettingsView.Section(Loc.T("正文", "Body")));
        var bodyBox = CodeBox(doc.Body, ro, "lib_body");
        bodyBox.MinHeight = 260;
        bodyBox.TextChanged += (_, _) => { if (!ro) ed.UpdateDoc(d => d.Body = EditorModel.Normalize(bodyBox.Text)); };
        page.Children.Add(bodyBox);
        page.Children.Add(Caption(Loc.T("MiniJinja 语法：{{ 参数名 }}（参数名中的 - 写作 _）、{% if strict %}…{% endif %}、{% for x in focus %}…{% endfor %}；附加要求为 extra；{% include \"_extra\" %} 引用公共片段。",
            "MiniJinja syntax: {{ param_name }} (use _ in place of - in names), {% if strict %}…{% endif %}, {% for x in focus %}…{% endfor %}. Extra instructions are available as extra; {% include \"_extra\" %} pulls in a shared snippet.")));
        return page;
    }

    private static string KindLabel(string k) => k switch
    {
        "bool" => Loc.T("开关", "Switch"),
        "enum" => Loc.T("单选", "Single choice"),
        "multi" => Loc.T("多选", "Multiple choice"),
        "string" => Loc.T("文本", "Text"),
        "number" => Loc.T("数字", "Number"),
        "text" => Loc.T("多行文本", "Multiline text"),
        _ => k,
    };

    private static readonly string[] Kinds = { "bool", "enum", "multi", "string", "number", "text" };

    private string ParamTitle(ParamDoc p)
    {
        var parts = new List<string> { "--" + p.Name };
        if (!string.IsNullOrEmpty(p.Short)) parts.Add("-" + p.Short);
        parts.Add("· " + KindLabel(p.Kind));
        if (p.Positional) parts.Add("· " + Loc.T("位置", "Positional"));
        if (p.Required) parts.Add("· " + Loc.T("必填", "Required"));
        return string.Join("  ", parts);
    }

    private FrameworkElement ParamRow(EditorModel ed, int index)
    {
        var p = ed.Doc!.Params[index];
        bool open = ed.SelectedParam == index;
        var title = new TextBlock { Text = ParamTitle(p), FontFamily = Theme.Mono, FontSize = 13, Foreground = new SolidColorBrush(new Theme(Application.Current.RequestedTheme == ApplicationTheme.Dark).Option) };
        paramTitles[index] = title;
        var desc = new TextBlock { Text = p.Description, FontSize = 12, TextTrimming = TextTrimming.CharacterEllipsis, Foreground = SettingsView.Res("TextFillColorSecondaryBrush"), Margin = new Thickness(12, 0, 0, 0) };
        var head = new StackPanel { Orientation = Orientation.Horizontal };
        head.Children.Add(title);
        head.Children.Add(desc);
        var exp = new Expander
        {
            Header = head, IsExpanded = open, HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Stretch,
        };
        AutomationProperties.SetAutomationId(exp, "lib_param_" + index);
        AutomationProperties.SetName(exp, "--" + p.Name);
        exp.Expanding += (_, _) =>
        {
            ed.SelectedParam = index;
            if (exp.Content is null) exp.Content = Inspector(ed, index);
        };
        exp.Collapsed += (_, _) => { if (ed.SelectedParam == index) ed.SelectedParam = null; };
        if (open) exp.Content = Inspector(ed, index);
        return exp;
    }

    private void RefreshParamTitle(EditorModel ed, int index)
    {
        if (paramTitles.TryGetValue(index, out var t) && ed.Doc is { } d && index < d.Params.Count) t.Text = ParamTitle(d.Params[index]);
    }

    private FrameworkElement Inspector(EditorModel ed, int index)
    {
        bool ro = ed.ReadOnly;
        ParamDoc P() => ed.Doc!.Params[index];
        void Update(Action<ParamDoc> change, bool rebuild = false)
        {
            if (ro) return;
            ed.UpdateDoc(d => change(d.Params[index]));
            if (rebuild) BuildEditPane(); else RefreshParamTitle(ed, index);
        }
        var p = P();
        var sp = new StackPanel { Spacing = 10 };

        var row1 = new Grid { ColumnSpacing = 12 };
        row1.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row1.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(180) });
        row1.Children.Add(Field($"lib_p{index}_name", Loc.T("参数名", "Name"), p.Name, ro, v => Update(x => x.Name = v.Trim()), mono: true));
        var kind = new ComboBox { Header = Loc.T("类型", "Type"), HorizontalAlignment = HorizontalAlignment.Stretch, IsEnabled = !ro };
        foreach (var k in Kinds) kind.Items.Add(KindLabel(k));
        kind.SelectedIndex = Math.Max(0, Array.IndexOf(Kinds, p.Kind));
        AutomationProperties.SetAutomationId(kind, $"lib_p{index}_kind");
        kind.SelectionChanged += (_, _) =>
        {
            var k = Kinds[Math.Max(0, kind.SelectedIndex)];
            if (k == P().Kind) return;
            Update(x =>
            {
                x.Kind = k;
                x.DefaultValue = null;
                if (k != "bool") x.Short = null;
                if (k is not ("enum" or "multi")) { x.Positional = false; x.Choices = new(); }
                if (k != "number") { x.Min = null; x.Max = null; }
            }, rebuild: true);
        };
        Grid.SetColumn(kind, 1);
        row1.Children.Add(kind);
        sp.Children.Add(row1);

        sp.Children.Add(Field($"lib_p{index}_desc", Loc.T("说明", "Description"), p.Description, ro, v => Update(x => x.Description = v)));

        var flags = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 18 };
        var required = new CheckBox { Content = Loc.T("必填", "Required"), IsChecked = p.Required, IsEnabled = !ro };
        AutomationProperties.SetAutomationId(required, $"lib_p{index}_required");
        required.Click += (_, _) => Update(x => x.Required = required.IsChecked == true);
        flags.Children.Add(required);
        if (p.Kind == "bool")
        {
            var shortBox = new TextBox { Text = p.Short ?? "", MaxLength = 1, Width = 44, FontFamily = Theme.Mono, IsReadOnly = ro, PlaceholderText = "s" };
            AutomationProperties.SetAutomationId(shortBox, $"lib_p{index}_short");
            shortBox.TextChanged += (_, _) => Update(x => x.Short = shortBox.Text.Trim().Length == 0 ? null : shortBox.Text.Trim()[..1]);
            var l = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
            l.Children.Add(new TextBlock { Text = Loc.T("短名", "Short name"), VerticalAlignment = VerticalAlignment.Center });
            l.Children.Add(shortBox);
            flags.Children.Add(l);
        }
        if (p.Kind is "enum" or "multi")
        {
            var pos = new CheckBox { Content = Loc.T("位置参数", "Positional"), IsChecked = p.Positional, IsEnabled = !ro };
            AutomationProperties.SetAutomationId(pos, $"lib_p{index}_positional");
            pos.Click += (_, _) => Update(x => x.Positional = pos.IsChecked == true);
            flags.Children.Add(pos);
        }
        sp.Children.Add(flags);

        sp.Children.Add(DefaultEditor(ed, index, ro, Update));

        if (p.Kind == "number")
        {
            var mm = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
            mm.Children.Add(NumberField($"lib_p{index}_min", Loc.T("最小值", "Minimum"), p.Min, ro, v => Update(x => x.Min = v)));
            mm.Children.Add(NumberField($"lib_p{index}_max", Loc.T("最大值", "Maximum"), p.Max, ro, v => Update(x => x.Max = v)));
            sp.Children.Add(mm);
        }
        if (p.Kind is "enum" or "multi") sp.Children.Add(ChoicesEditor(ed, index, ro, Update));

        if (!ro)
        {
            var bar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, Margin = new Thickness(0, 4, 0, 0) };
            Button B(string id, string glyph, string tip, bool enabled, Action click)
            {
                var b = new Button { Content = new FontIcon { Glyph = glyph, FontFamily = Theme.Icons, FontSize = 12 }, IsEnabled = enabled, Padding = new Thickness(8, 5, 8, 5) };
                ToolTipService.SetToolTip(b, tip);
                AutomationProperties.SetAutomationId(b, id);
                AutomationProperties.SetName(b, tip);
                b.Click += (_, _) => click();
                return b;
            }
            int count = ed.Doc!.Params.Count;
            bar.Children.Add(B($"lib_p{index}_up", "", Loc.T("上移", "Move up"), index > 0, () => Move(ed, index, -1)));
            bar.Children.Add(B($"lib_p{index}_down", "", Loc.T("下移", "Move down"), index < count - 1, () => Move(ed, index, 1)));
            var del = new Button { Content = Loc.T("删除参数", "Delete Parameter") };
            AutomationProperties.SetAutomationId(del, $"lib_p{index}_delete");
            del.Click += (_, _) =>
            {
                ed.UpdateDoc(d => d.Params.RemoveAt(index));
                ed.SelectedParam = null;
                BuildEditPane();
            };
            bar.Children.Add(del);
            sp.Children.Add(bar);
        }
        return sp;
    }

    private void Move(EditorModel ed, int i, int delta)
    {
        ed.UpdateDoc(d => (d.Params[i], d.Params[i + delta]) = (d.Params[i + delta], d.Params[i]));
        ed.SelectedParam = i + delta;
        BuildEditPane();
    }

    private static string FormatNumber(double v) => v % 1 == 0 && Math.Abs(v) < 1e15 ? ((long)v).ToString(CultureInfo.InvariantCulture) : v.ToString(CultureInfo.InvariantCulture);

    private static NumberBox NumberField(string id, string header, double? value, bool ro, Action<double?> changed)
    {
        var n = new NumberBox { Header = header, Value = value ?? double.NaN, Width = 150, IsEnabled = !ro, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };
        AutomationProperties.SetAutomationId(n, id);
        n.ValueChanged += (_, e) => changed(double.IsNaN(e.NewValue) ? null : e.NewValue);
        return n;
    }

    private FrameworkElement DefaultEditor(EditorModel ed, int index, bool ro, Action<Action<ParamDoc>, bool> update)
    {
        var p = ed.Doc!.Params[index];
        var id = $"lib_p{index}_default";
        switch (p.Kind)
        {
            case "bool":
            {
                var c = new CheckBox { Content = Loc.T("默认开启", "On by default"), IsChecked = p.DefaultValue == "true", IsEnabled = !ro };
                AutomationProperties.SetAutomationId(c, id);
                c.Click += (_, _) => update(x => x.DefaultValue = c.IsChecked == true ? "true" : null, false);
                return c;
            }
            case "enum":
            {
                var c = new ComboBox { Header = Loc.T("默认值", "Default"), MinWidth = 200, IsEnabled = !ro };
                c.Items.Add(Loc.T("无", "None"));
                foreach (var ch in p.Choices) c.Items.Add($"{ch.Label}（{ch.Value}）");
                c.SelectedIndex = p.DefaultValue is { } dv ? Math.Max(0, p.Choices.FindIndex(x => x.Value == dv) + 1) : 0;
                AutomationProperties.SetAutomationId(c, id);
                c.SelectionChanged += (_, _) =>
                {
                    var i = c.SelectedIndex;
                    update(x => x.DefaultValue = i <= 0 || i > x.Choices.Count ? null : x.Choices[i - 1].Value, false);
                };
                return c;
            }
            case "multi":
            {
                var sp = new StackPanel { Spacing = 6 };
                sp.Children.Add(new TextBlock { Text = Loc.T("默认选中", "Selected by default") });
                var wrap = new WrapPanel { Spacing = 6 };
                var on = (p.DefaultValue ?? "").Split(',', StringSplitOptions.RemoveEmptyEntries).ToHashSet();
                foreach (var ch in p.Choices)
                {
                    var value = ch.Value;
                    var t = new ToggleButton { Content = string.IsNullOrEmpty(ch.Label) ? ch.Value : ch.Label, IsChecked = on.Contains(value), IsEnabled = !ro };
                    AutomationProperties.SetAutomationId(t, $"{id}_{value}");
                    t.Click += (_, _) => update(x =>
                    {
                        var set = (x.DefaultValue ?? "").Split(',', StringSplitOptions.RemoveEmptyEntries).ToList();
                        if (t.IsChecked == true) set.Add(value); else set.RemoveAll(v => v == value);
                        var ordered = x.Choices.Select(c => c.Value).Where(set.Contains).ToList();
                        x.DefaultValue = ordered.Count == 0 ? null : string.Join(",", ordered);
                    }, false);
                    wrap.Children.Add(t);
                }
                sp.Children.Add(wrap);
                return sp;
            }
            case "number":
                return NumberField(id, Loc.T("默认值", "Default"), double.TryParse(p.DefaultValue, NumberStyles.Float, CultureInfo.InvariantCulture, out var d) ? d : null, ro,
                    v => update(x => x.DefaultValue = v is double n ? FormatNumber(n) : null, false));
            default:
                return Field(id, Loc.T("默认值", "Default"), p.DefaultValue ?? "", ro, v => update(x => x.DefaultValue = v.Length == 0 ? null : v, false));
        }
    }

    private FrameworkElement ChoicesEditor(EditorModel ed, int index, bool ro, Action<Action<ParamDoc>, bool> update)
    {
        var p = ed.Doc!.Params[index];
        var sp = new StackPanel { Spacing = 6 };
        sp.Children.Add(new TextBlock { Text = Loc.T("候选值", "Choices"), FontSize = 12, Foreground = SettingsView.Res("TextFillColorSecondaryBrush") });
        for (int i = 0; i < p.Choices.Count; i++)
        {
            int ci = i;
            var c = p.Choices[i];
            var row = new Grid { ColumnSpacing = 8 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(140) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(130) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            var value = new TextBox { Text = c.Value, PlaceholderText = Loc.T("值", "Value"), FontFamily = Theme.Mono, IsReadOnly = ro, IsSpellCheckEnabled = false };
            AutomationProperties.SetAutomationId(value, $"lib_p{index}_c{ci}_value");
            value.TextChanged += (_, _) => update(x => x.Choices[ci].Value = value.Text.Trim(), false);
            // 值变化会影响默认值的候选：离开输入框后重建
            value.LostFocus += (_, _) => { if (!ro && ed.Doc is { } d && index < d.Params.Count && ci < d.Params[index].Choices.Count && d.Params[index].Choices[ci].Value != c.Value) BuildEditPane(); };
            var label = new TextBox { Text = c.Label, PlaceholderText = Loc.T("显示名", "Label"), IsReadOnly = ro };
            AutomationProperties.SetAutomationId(label, $"lib_p{index}_c{ci}_label");
            label.TextChanged += (_, _) => update(x => x.Choices[ci].Label = label.Text, false);
            var aliases = new TextBox { Text = string.Join(", ", c.Aliases), PlaceholderText = Loc.T("别名（逗号分隔）", "Aliases (comma-separated)"), IsReadOnly = ro };
            aliases.TextChanged += (_, _) => update(x => x.Choices[ci].Aliases = SplitList(aliases.Text), false);
            row.Children.Add(value);
            Grid.SetColumn(label, 1);
            row.Children.Add(label);
            Grid.SetColumn(aliases, 2);
            row.Children.Add(aliases);
            if (!ro)
            {
                var rm = new Button { Content = new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 12 }, Padding = new Thickness(8, 6, 8, 6) };
                ToolTipService.SetToolTip(rm, Loc.T("删除候选值", "Remove choice"));
                rm.Click += (_, _) => update(x => x.Choices.RemoveAt(ci), true);
                Grid.SetColumn(rm, 3);
                row.Children.Add(rm);
            }
            sp.Children.Add(row);
        }
        if (!ro)
        {
            var add = new HyperlinkButton { Content = Loc.T("＋ 添加候选值", "+ Add Choice") };
            AutomationProperties.SetAutomationId(add, $"lib_p{index}_addchoice");
            add.Click += (_, _) => update(x =>
            {
                string n = "option";
                int k = 2;
                while (x.Choices.Any(c => c.Value == n)) n = $"option{k++}";
                x.Choices.Add(new ChoiceDoc { Value = n, Label = n });
            }, true);
            sp.Children.Add(add);
        }
        return sp;
    }

    // MARK: 试用与预览

    private FrameworkElement BuildTryPanel()
    {
        var sp = new StackPanel { Spacing = 10, Padding = new Thickness(0, 12, 0, 0) };
        sp.Children.Add(new TextBlock { Text = Loc.T("试用", "Try it"), FontWeight = FontWeights.SemiBold, FontSize = 15 });
        var ed = editor;
        var input = new TextBox { FontFamily = Theme.Mono, IsSpellCheckEnabled = false, Text = ed?.TryInput ?? "" };
        AutomationProperties.SetAutomationId(input, "lib_try");
        input.TextChanged += (_, _) => { if (ed != null && ReferenceEquals(ed, editor) && input.Text != ed.TryInput) ed.SetTryInput(input.Text); };
        tryInput = input;
        tryResults = new StackPanel { Spacing = 10 };
        sp.Children.Add(tryInput);
        sp.Children.Add(tryResults);
        return sp;
    }

    private void RenderTry()
    {
        tryResults.Children.Clear();
        if (editor is not { } ed) return;
        var name = ed.Check.Name ?? "r";
        tryInput.PlaceholderText = Loc.T($"输入命令试用，如 {name} --strict 附加要求", $"Type a command, e.g. {name} --strict extra instructions");
        var theme = new Theme(Application.Current.RequestedTheme == ApplicationTheme.Dark);
        if (ed.Check.Problems.Count > 0)
        {
            var problems = new StackPanel { Spacing = 4 };
            foreach (var p in ed.Check.Problems)
            {
                var t = new TextBlock { Text = "✕ " + p, TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true, FontSize = 13, Foreground = new SolidColorBrush(theme.Error) };
                if (problems.Children.Count == 0) AutomationProperties.SetAutomationId(t, "lib_problem");
                problems.Children.Add(t);
            }
            tryResults.Children.Add(problems);
            return;
        }
        if (ed.Check.Analysis is not { } a) return;
        if (a.Diagnostics.FirstOrDefault(d => d.IsError) is { } err)
            tryResults.Children.Add(new TextBlock { Text = "! " + err.Message, TextWrapping = TextWrapping.Wrap, FontSize = 13, Foreground = new SolidColorBrush(theme.Warning) });
        if (a.Params.Count > 0)
        {
            var wrap = new WrapPanel { Spacing = 6 };
            foreach (var p in a.Params)
            {
                var chip = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
                chip.Children.Add(new TextBlock { Text = p.Name, FontFamily = Theme.Mono, FontSize = 11, Foreground = p.IsSet ? new SolidColorBrush(theme.Option) : SettingsView.Res("TextFillColorSecondaryBrush") });
                chip.Children.Add(new TextBlock { Text = p.Display, FontSize = 11, FontWeight = p.IsSet ? FontWeights.SemiBold : FontWeights.Normal });
                wrap.Children.Add(new Border { Child = chip, Padding = new Thickness(8, 3, 8, 3), CornerRadius = new CornerRadius(10), BorderThickness = new Thickness(1), BorderBrush = SettingsView.Res("ControlStrokeColorDefaultBrush") });
            }
            tryResults.Children.Add(wrap);
        }
        if (a.Canonical is { } c)
            tryResults.Children.Add(new TextBlock { Text = c, FontFamily = Theme.Mono, FontSize = 11, IsTextSelectionEnabled = true, TextWrapping = TextWrapping.Wrap, Foreground = SettingsView.Res("TextFillColorTertiaryBrush") });
        if (a.Preview is { } pv)
        {
            tryResults.Children.Add(new TextBlock { Text = Loc.T($"预览 · {pv.Chars} 字 · {pv.Lines} 行", $"Preview · {pv.Chars} chars · {pv.Lines} lines"), FontSize = 12, Foreground = SettingsView.Res("TextFillColorSecondaryBrush") });
            var text = new TextBlock { Text = pv.Text, TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true, FontSize = 13, LineHeight = 20 };
            AutomationProperties.SetAutomationId(text, "lib_preview");
            tryResults.Children.Add(new Border { Child = text, Padding = new Thickness(10), CornerRadius = new CornerRadius(8), Background = SettingsView.Res("CardBackgroundFillColorDefaultBrush") });
        }
    }

    // MARK: 操作

    private async Task<ContentDialogResult?> Dialog(ContentDialog d)
    {
        if (dialogOpen || root.XamlRoot is null) return null;
        dialogOpen = true;
        try
        {
            d.XamlRoot = root.XamlRoot;
            d.RequestedTheme = root.ActualTheme;
            return await d.ShowAsync();
        }
        finally { dialogOpen = false; }
    }

    private async Task Alert(string title, string? message = null)
    {
        await Dialog(new ContentDialog { Title = title, Content = message, CloseButtonText = Loc.T("好", "OK") });
    }

    /// 执行文件操作，失败时提示。
    private async Task Guarded(Action action)
    {
        if (!await ConfirmDiscard()) return;
        try { action(); }
        catch (Exception e)
        {
            Log.Error("library", e);
            await Alert(Loc.T("操作失败", "Something went wrong"), e.Message);
        }
    }

    private Task NewTemplate() => Guarded(model.NewTemplate);

    /// 删除模板（docs/decisions.md D34）：用户模板移到回收站；内置模板（含修改过的）删除后可以恢复。
    private async Task Delete(TemplateEntry e)
    {
        bool builtin = e.Builtin || e.OverridesBuiltin;
        var r = await Dialog(new ContentDialog
        {
            Title = builtin ? Loc.T($"删除内置模板「{e.Name}」？", $"Delete the built-in template “{e.Name}”?") : Loc.T($"删除模板「{e.Name}」？", $"Delete template “{e.Name}”?"),
            Content = new TextBlock
            {
                TextWrapping = TextWrapping.Wrap,
                Text = !builtin ? Loc.T("文件会被移到回收站。", "The file will be moved to the Recycle Bin.")
                    : e.OverridesBuiltin
                        ? Loc.T("你的修改会移到回收站。之后可以在“已删除”中恢复内置版本，它的名称和别名在此期间可以给你自己的模板使用。", "Your changes go to the Recycle Bin. You can restore the built-in version from “Deleted” later; meanwhile its name and aliases are free for your own templates.")
                        : Loc.T("之后可以在“已删除”中恢复。它的名称和别名在此期间可以给你自己的模板使用。", "You can restore it from “Deleted” later. Meanwhile its name and aliases are free for your own templates."),
            },
            PrimaryButtonText = Loc.T("删除", "Delete"),
            CloseButtonText = Loc.T("取消", "Cancel"),
            DefaultButton = ContentDialogButton.Close,
        });
        if (r != ContentDialogResult.Primary) return;
        if (model.Delete(e) is { } err) await Alert(Loc.T("删除失败", "Couldn't delete the template"), err);
    }

    /// 恢复内置模板：撤销删除，或丢弃修改（修改版本移到回收站，先确认）。
    private async Task Restore(TemplateEntry e)
    {
        if (e.OverridesBuiltin)
        {
            var r = await Dialog(new ContentDialog
            {
                Title = Loc.T($"恢复「{e.Name}」为内置版本？", $"Restore “{e.Name}” to the built-in version?"),
                Content = Loc.T("你的修改会移到回收站。", "Your changes will be moved to the Recycle Bin."),
                PrimaryButtonText = Loc.T("恢复内置", "Restore"),
                CloseButtonText = Loc.T("取消", "Cancel"),
                DefaultButton = ContentDialogButton.Close,
            });
            if (r != ContentDialogResult.Primary) return;
        }
        if (model.Restore(e) is { } err) await Alert(Loc.T("恢复失败", "Couldn't restore the template"), err);
    }

    private T Init<T>(T picker)
    {
        WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);
        return picker;
    }

    private async Task ExportAll()
    {
        try
        {
            var picker = Init(new FileSavePicker { SuggestedFileName = "qantrip-templates", SuggestedStartLocation = PickerLocationId.DocumentsLibrary });
            picker.FileTypeChoices.Add("Zip", new List<string> { ".zip" });
            var file = await picker.PickSaveFileAsync();
            if (file is null) return;
            model.ExportAll(file.Path);
        }
        catch (Exception e)
        {
            Log.Error("export", e);
            await Alert(Loc.T("导出失败", "Export failed"), e.Message);
        }
    }

    private async Task ExportOne(TemplateEntry entry)
    {
        try
        {
            var picker = Init(new FileSavePicker { SuggestedFileName = entry.Name, SuggestedStartLocation = PickerLocationId.DocumentsLibrary });
            picker.FileTypeChoices.Add("Markdown", new List<string> { ".md" });
            var file = await picker.PickSaveFileAsync();
            if (file is null) return;
            model.ExportOne(entry, file.Path);
        }
        catch (Exception e)
        {
            Log.Error("export", e);
            await Alert(Loc.T("导出失败", "Export failed"), e.Message);
        }
    }

    private async Task ImportFiles()
    {
        try
        {
            var picker = Init(new FileOpenPicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary });
            picker.FileTypeFilter.Add(".md");
            picker.FileTypeFilter.Add(".zip");
            var files = await picker.PickMultipleFilesAsync();
            if (files is null || files.Count == 0) return;
            await RunImport(files.Select(f => f.Path).ToList());
        }
        catch (Exception e)
        {
            Log.Error("import", e);
            await Alert(Loc.T("导入失败", "Import failed"), e.Message);
        }
    }

    /// 导入并汇报结果（也供拖放使用）。
    public async Task RunImport(List<string> paths)
    {
        if (paths.Count == 0 || !await ConfirmDiscard()) return;
        var result = await model.Import(paths, AskConflict);
        var info = new List<string>();
        if (result.Skipped > 0) info.Add(Loc.T($"跳过 {result.Skipped} 个", $"{result.Skipped} skipped"));
        if (result.Failures.Count > 0) info.Add(Loc.T($"无法导入：{string.Join("、", result.Failures)}", $"Could not import: {string.Join(", ", result.Failures)}"));
        await Alert(Loc.T($"导入完成：{result.Imported} 个", $"Import complete: {result.Imported}"), info.Count > 0 ? string.Join("\n", info) : null);
    }

    private async Task<(LibraryModel.Conflict, bool)> AskConflict(string name)
    {
        var sticky = new CheckBox { Content = Loc.T("对其余冲突执行相同操作", "Apply to remaining conflicts") };
        var r = await Dialog(new ContentDialog
        {
            Title = Loc.T($"已存在名为「{name}」的模板", $"A template named “{name}” already exists"),
            Content = sticky,
            PrimaryButtonText = Loc.T("覆盖", "Replace"),
            SecondaryButtonText = Loc.T("重命名导入", "Import as Copy"),
            CloseButtonText = Loc.T("跳过", "Skip"),
        });
        var choice = r switch
        {
            ContentDialogResult.Primary => LibraryModel.Conflict.Replace,
            ContentDialogResult.Secondary => LibraryModel.Conflict.Rename,
            _ => LibraryModel.Conflict.Skip,
        };
        return (choice, sticky.IsChecked == true);
    }
}
