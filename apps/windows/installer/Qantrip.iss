; Nekro Qantrip Windows 安装程序（Inno Setup 6.6+ / 7，按用户安装，不需要管理员权限）。
;
; 由 apps\windows\build.ps1 调用（找到 ISCC 时）：
;   ISCC /DAppVersion=<版本> /DSourceDir=<发布目录> /O<输出目录> apps\windows\installer\Qantrip.iss
; 输出 Qantrip-Windows-x64-Setup.exe。安装到 %LOCALAPPDATA%\Programs\Nekro Qantrip，开始菜单快捷方式，
; 可选"登录时启动"（与设置中的开关写同一个注册表值），安装完成后启动；卸载程序出现在"应用和功能"中。
; 用户数据（%APPDATA%\Qantrip、%LOCALAPPDATA%\Qantrip）卸载时保留。
; 向导图片由 make-images.py 生成；中文界面文字优先使用编译器自带的 ChineseSimplified.isl（Inno Setup 7 起为官方翻译），
; 没有时使用本目录中的副本（取自 Inno Setup 源码仓库，Inno Setup 许可证，保留原文件头）。

#if Ver < EncodeVer(6, 6, 0)
  #error 需要 Inno Setup 6.6 或更新版本（WizardStyle dynamic、PNG 向导图片）
#endif
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\..\dist\windows\Qantrip"
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\ChineseSimplified.isl")
  #define ChineseMessages "compiler:Languages\ChineseSimplified.isl"
#else
  #define ChineseMessages "ChineseSimplified.isl"
#endif

[Setup]
; AppId 固定不变：升级与卸载都依赖它
AppId={{6F1C2B7E-3D4A-4E8B-9C51-0A7D2E5F9B13}
AppName=Nekro Qantrip
AppVersion={#AppVersion}
AppVerName=Nekro Qantrip {#AppVersion}
AppPublisher=NekroAI
AppPublisherURL=https://qantrip.nekro.ai
AppSupportURL=https://github.com/NekroAI/qantrip/issues
AppUpdatesURL=https://github.com/NekroAI/qantrip/releases
AppCopyright=NekroAI · GPL-3.0-or-later
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Nekro Qantrip
VersionInfoDescription=Nekro Qantrip Setup
; 按用户安装，不请求提升权限，也不提供"为所有用户安装"
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\Nekro Qantrip
DisableDirPage=auto
DisableProgramGroupPage=yes
DisableReadyPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 2004（19041）起（与 TargetPlatformMinVersion 一致）
MinVersion=10.0.19041
OutputBaseFilename=Qantrip-Windows-x64-Setup
SetupIconFile=..\Qantrip\Assets\Qantrip.ico
UninstallDisplayIcon={app}\Qantrip.exe
UninstallDisplayName=Nekro Qantrip
WizardStyle=modern dynamic
WizardImageFile=images\wizard-202.png,images\wizard-336.png,images\wizard-430.png,images\wizard-534.png
WizardSmallImageFile=images\small-58.png,images\small-97.png,images\small-124.png,images\small-159.png
ShowLanguageDialog=no
Compression=lzma2/ultra64
SolidCompression=yes
LZMAUseSeparateProcess=yes
; 运行中的 Qantrip 由 [Code] 先请它退出（见 StopRunningApp），不使用重启管理器
CloseApplications=no
RestartApplications=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "zh"; MessagesFile: "{#ChineseMessages}"

[CustomMessages]
en.AutostartTask=Launch Nekro Qantrip when I sign in to Windows
zh.AutostartTask=登录 Windows 时启动 Nekro Qantrip
en.LaunchApp=Launch Nekro Qantrip
zh.LaunchApp=启动 Nekro Qantrip
en.AppComment=Press Ctrl+Q anywhere to cast a prompt
zh.AppComment=在任意程序中按 Ctrl+Q 施放 Prompt

[Tasks]
Name: "autostart"; Description: "{cm:AutostartTask}"; Check: IsFirstInstall

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\Nekro Qantrip"; Filename: "{app}\Qantrip.exe"; Comment: "{cm:AppComment}"

[Registry]
; 与应用内"登录时启动"相同的位置与格式（Platform/Shell.cs Autostart）
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "NekroQantrip"; ValueData: """{app}\Qantrip.exe"""; Tasks: autostart

[Run]
Filename: "{app}\Qantrip.exe"; Description: "{cm:LaunchApp}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  ApprovedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run';
  RunValue = 'NekroQantrip';
  AppMutexName = 'Local\NekroQantrip.Instance';

function RegisterWindowMessage(Name: String): Cardinal;
  external 'RegisterWindowMessageW@user32.dll stdcall';

{ 请正在运行的 Qantrip 退出（与托盘菜单"退出"相同，托盘图标随之移除）；旧版本不认识这条消息时，5 秒后强制结束。 }
procedure StopRunningApp();
var
  Wnd: HWND;
  I, Code: Integer;
begin
  if not CheckForMutexes(AppMutexName) then Exit;
  Wnd := FindWindowByClassName('NekroQantrip.MessageWindow');
  if Wnd <> 0 then PostMessage(Wnd, RegisterWindowMessage('NekroQantrip.Quit'), 0, 0);
  for I := 1 to 50 do
  begin
    if not CheckForMutexes(AppMutexName) then Exit;
    Sleep(100);
  end;
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM Qantrip.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(500);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopRunningApp();
  Result := '';
end;

{ 升级时不再显示"登录时启动"：保留用户之后在应用设置中的选择（键名为 AppId + _is1） }
function IsFirstInstall(): Boolean;
begin
  Result := not RegKeyExists(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{6F1C2B7E-3D4A-4E8B-9C51-0A7D2E5F9B13}_is1');
end;

function InitializeUninstall(): Boolean;
begin
  StopRunningApp();
  Result := True;
end;

{ 卸载时删除指向本安装目录的"登录时启动"登记（无论是安装程序还是应用设置写入的） }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Value: String;
begin
  if CurUninstallStep = usPostUninstall then
    if RegQueryStringValue(HKCU, RunKey, RunValue, Value) and
       (CompareText(RemoveQuotes(Value), ExpandConstant('{app}\Qantrip.exe')) = 0) then
    begin
      RegDeleteValue(HKCU, RunKey, RunValue);
      RegDeleteValue(HKCU, ApprovedKey, RunValue);
    end;
end;
