# Clipboard helpers for tests (dot-source; requires STA powershell)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Runtime.WindowsRuntime
[Windows.ApplicationModel.DataTransfer.Clipboard,Windows.ApplicationModel.DataTransfer,ContentType=WindowsRuntime] | Out-Null
$script:asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($op, [Type]$t) { $task = $script:asTask.MakeGenericMethod($t).Invoke($null, @($op)); $task.Wait(5000) | Out-Null; $task.Result }

function SetRichClip([string]$text) {
  $d = New-Object System.Windows.Forms.DataObject
  $d.SetData([System.Windows.Forms.DataFormats]::UnicodeText, $text)
  $html = "Version:0.9`r`nStartHTML:00000097`r`nEndHTML:00000170`r`nStartFragment:00000131`r`nEndFragment:00000134`r`n<html><body><!--StartFragment--><b>$text</b><!--EndFragment--></body></html>"
  $d.SetData([System.Windows.Forms.DataFormats]::Html, $html)
  $d.SetData("Qantrip.Test.Custom", [byte[]](1,2,3,4,5))
  for ($i = 0; $i -lt 10; $i++) {
    try { [System.Windows.Forms.Clipboard]::SetDataObject($d, $true, 20, 100); return } catch { "clipboard busy, retry $i"; Start-Sleep -Milliseconds 500 }
  }
}

function ClipDigest {
  $d = [System.Windows.Forms.Clipboard]::GetDataObject()
  $parts = @()
  foreach ($f in ($d.GetFormats($false) | Sort-Object)) {
    try { $v = $d.GetData($f) } catch { $v = $null }
    $s = if ($v -is [string]) { $v } elseif ($v -is [System.IO.MemoryStream]) { [Convert]::ToBase64String($v.ToArray()) } elseif ($v -is [byte[]]) { [Convert]::ToBase64String($v) } else { "$v" }
    $h = [BitConverter]::ToString((New-Object Security.Cryptography.SHA1Managed).ComputeHash([Text.Encoding]::UTF8.GetBytes([string]$s))).Replace("-","").Substring(0,10)
    $parts += "$f=$h"
  }
  $parts -join " "
}

function CleanupHistory {
  $r = Await ([Windows.ApplicationModel.DataTransfer.Clipboard]::GetHistoryItemsAsync()) ([Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult])
  $n = 0
  foreach ($it in @($r.Items)) {
    try { if ($it.Content.Contains("Text")) { $t = Await ($it.Content.GetTextAsync()) ([string]); if ($t -like "QANTRIP-ORIG-*") { [Windows.ApplicationModel.DataTransfer.Clipboard]::DeleteItemFromHistory($it) | Out-Null; $n++ } } } catch {}
  }
  "removed $n test items from clipboard history"
}

function HistoryTexts([int]$n = 8) {
  $r = Await ([Windows.ApplicationModel.DataTransfer.Clipboard]::GetHistoryItemsAsync()) ([Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult])
  if ($null -eq $r) { return @("history: null") }
  $out = @("history status=$($r.Status) count=$($r.Items.Count)")
  $i = 0
  foreach ($it in $r.Items) {
    if ($i -ge $n) { break }
    $t = ""
    try { if ($it.Content.Contains("Text")) { $t = Await ($it.Content.GetTextAsync()) ([string]) } } catch { $t = "<err>" }
    $t = $t -replace "`r?`n", "\n"
    # only show our own test strings; never print the owner's clipboard contents
    if ($t -notmatch "QANTRIP|qantrip|审查|翻译|严格|Review|review") { $t = "<other item>" }
    if ($t.Length -gt 60) { $t = $t.Substring(0, 60) + "..." }
    $out += "  [$i] $t"
    $i++
  }
  $out
}
