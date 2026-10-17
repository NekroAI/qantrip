# Test driver helpers (dot-source in interactive-session scripts).
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text; using System.Threading;
public static class Drv {
  [StructLayout(LayoutKind.Explicit, Size=40)] public struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KI ki; }
  [StructLayout(LayoutKind.Sequential)] public struct KI { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int size);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder sb, int n);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint thread);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  static INPUT K(ushort vk, bool up, bool uni = false, ushort scan = 0) { var i = new INPUT(); i.type = 1; i.ki.vk = vk; i.ki.scan = scan; i.ki.flags = (up ? 2u : 0u) | (uni ? 4u : 0u); return i; }
  public static uint Combo(params ushort[] vks) { var l = new System.Collections.Generic.List<INPUT>(); foreach (var v in vks) l.Add(K(v,false)); for (int j = vks.Length-1; j>=0; j--) l.Add(K(vks[j], true)); return SendInput((uint)l.Count, l.ToArray(), 40); }
  public static void Type(string s, int delayMs) { foreach (var c in s) { var a = new INPUT[] { K(0,false,true,c), K(0,true,true,c) }; SendInput(2, a, 40); Thread.Sleep(delayMs); } }
  // Real virtual-key typing (goes through the IME), letters/digits/space only
  public static void TypeVk(string s, int delayMs) { foreach (var c in s.ToUpperInvariant()) { Combo((ushort)c); Thread.Sleep(delayMs); } }
  public static string Info(IntPtr h) { var sb = new StringBuilder(256); GetWindowText(h, sb, 256); var cb = new StringBuilder(256); GetClassName(h, cb, 256); uint pid; GetWindowThreadProcessId(h, out pid); string name = ""; try { name = System.Diagnostics.Process.GetProcessById((int)pid).ProcessName; } catch {} return String.Format("hwnd={0:X} pid={1} proc={2} class={3} title={4}", h.ToInt64(), pid, name, cb, sb); }
  public static string Fg() { return Info(GetForegroundWindow()); }
  // Force a window to the foreground from a background process (test setup only)
  public static bool Focus(IntPtr h) {
    if (SetForegroundWindow(h)) { Thread.Sleep(150); if (GetForegroundWindow() == h) return true; }
    uint pid; uint fgt = GetWindowThreadProcessId(GetForegroundWindow(), out pid); uint me = GetCurrentThreadId();
    AttachThreadInput(me, fgt, true); ShowWindow(h, 9); BringWindowToTop(h); SetForegroundWindow(h); AttachThreadInput(me, fgt, false);
    Thread.Sleep(150); return GetForegroundWindow() == h; }
  public static long WaitFgProc(string proc, int timeoutMs) { var sw = System.Diagnostics.Stopwatch.StartNew(); while (sw.ElapsedMilliseconds < timeoutMs) { uint pid; GetWindowThreadProcessId(GetForegroundWindow(), out pid); try { if (System.Diagnostics.Process.GetProcessById((int)pid).ProcessName == proc) return sw.ElapsedMilliseconds; } catch {} Thread.Sleep(1); } return -1; }
}
"@
[Drv]::SetProcessDPIAware() | Out-Null
$VK = @{ ALT=0x12; CTRL=0x11; SHIFT=0x10; ENTER=0x0D; ESC=0x1B; TAB=0x09; UP=0x26; DOWN=0x28; RIGHT=0x27; Q=0x51; V=0x56; S=0x53; A=0x41; SPACE=0x20; BACK=0x08; W=0x57 }
# Work dir (test files, test data dir). Override with QANTRIP_DEV.
$Dev = if ($env:QANTRIP_DEV) { $env:QANTRIP_DEV } else { "$env:USERPROFILE\qantrip-dev" }
# App under test. Override with QANTRIP_APP.
$App = if ($env:QANTRIP_APP) { $env:QANTRIP_APP } else { Join-Path $PSScriptRoot "..\..\dist\windows\Qantrip\Qantrip.exe" }
$LogFile = "$env:LOCALAPPDATA\Qantrip\logs\spike.jsonl"

function Shot([string]$name, [int]$x = 0, [int]$y = 0, [int]$w = 0, [int]$h = 0, [double]$scale = 0.5) {
  if ($w -eq 0) { $w = [Drv]::GetSystemMetrics(0); $h = [Drv]::GetSystemMetrics(1) }
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size); $g.Dispose()
  if ($scale -ne 1) {
    $sw = [int]($w * $scale); $sh = [int]($h * $scale)
    $s = New-Object System.Drawing.Bitmap $sw, $sh
    $g2 = [System.Drawing.Graphics]::FromImage($s); $g2.InterpolationMode = 7; $g2.DrawImage($bmp, 0, 0, $sw, $sh); $g2.Dispose(); $bmp.Dispose(); $bmp = $s
  }
  $bmp.Save("$PSScriptRoot\$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  "shot $name"
}

function EnsureApp([switch]$Restart, [hashtable]$Env = @{}) {
  if ($Restart) { Get-Process Qantrip -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 500 }
  if (-not (Get-Process Qantrip -ErrorAction SilentlyContinue)) {
    foreach ($k in $Env.Keys) { [Environment]::SetEnvironmentVariable($k, $Env[$k]) }
    Start-Process $App
    Start-Sleep -Seconds 3
  }
}

function LogTail([int]$n = 5) { if (Test-Path $LogFile) { Get-Content $LogFile -Tail $n -Encoding UTF8 } }
