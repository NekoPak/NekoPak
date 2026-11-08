param([Parameter(Mandatory=$true)][int]$TargetProcessId,[Parameter(Mandatory=$true)][ValidateSet('choose','cancel')][string]$Action,[string]$Directory)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class NativeDialogTest {
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageW(IntPtr h,uint m,IntPtr w,string l);
 [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
}
'@
$processCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$TargetProcessId)
$until=[DateTime]::UtcNow.AddSeconds(12)
$dialog=$null
while([DateTime]::UtcNow -lt $until){
  $windows=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$processCondition)
  $dialog=$windows | Where-Object {$_.Current.Name -like '*Choose download folder'} | Select-Object -First 1
  if($dialog){break}
  Start-Sleep -Milliseconds 100
}
if(!$dialog){$windows | ForEach-Object {$_.Current.Name};throw 'Native folder dialog did not appear in the isolated process.'}
Start-Sleep -Milliseconds 400
$children=$dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
if($Action -eq 'cancel'){
  $button=$children | Where-Object { $_.Current.AutomationId -eq '2' } | Select-Object -First 1
  if(!$button){$children | ForEach-Object {[pscustomobject]@{Name=$_.Current.Name;Id=$_.Current.AutomationId;Type=$_.Current.ControlType.ProgrammaticName}} | ConvertTo-Json;throw 'Cancel button not found'}
  [void][NativeDialogTest]::SendMessageW([IntPtr]$button.Current.NativeWindowHandle,0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
  Write-Output '{"native_dialog":true,"action":"cancel","passed":true}'
  exit 0
}
$resolved=[System.IO.Path]::GetFullPath($Directory)
if(!(Test-Path -LiteralPath $resolved -PathType Container)){throw 'Fixture folder not found'}
$edit=$children | Where-Object { $_.Current.AutomationId -eq '1152' } | Select-Object -First 1
if(!$edit){
  $children | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit } | ForEach-Object { [pscustomobject]@{Name=$_.Current.Name;Id=$_.Current.AutomationId} } | ConvertTo-Json
  throw 'Folder input not found'
}
[void][NativeDialogTest]::SendMessageW([IntPtr]$edit.Current.NativeWindowHandle,0xC,[IntPtr]::Zero,$resolved)
$button=$children | Where-Object { $_.Current.AutomationId -eq '1' } | Select-Object -First 1
if(!$button){throw 'Select folder button not found'}
[void][NativeDialogTest]::SendMessageW([IntPtr]$button.Current.NativeWindowHandle,0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
Write-Output '{"native_dialog":true,"action":"choose","passed":true}'
