[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class DeviceBotWindow {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
}
'@

$processId = [uint32]__PROCESS_ID__
if ($processId -eq 0) {
  $foreground = [DeviceBotWindow]::GetForegroundWindow()
  if ($foreground -ne [IntPtr]::Zero) {
    [void][DeviceBotWindow]::GetWindowThreadProcessId($foreground, [ref]$processId)
  }
}
if ($processId -eq 0) {
  throw 'The foreground Windows process id is not available.'
}

$root = [System.Windows.Automation.AutomationElement]::RootElement
$condition = New-Object System.Windows.Automation.PropertyCondition -ArgumentList @(
  [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
  [int]$processId
)
$target = $root.FindFirst([System.Windows.Automation.TreeScope]::Subtree, $condition)
if ($null -eq $target) {
  throw "UI Automation could not find a window for process id $processId. The target may be elevated or running in another desktop session."
}

$walker = [System.Windows.Automation.TreeWalker]::RawViewWalker
$nodes = New-Object 'System.Collections.Generic.List[object]'
$maxResults = [int]__MAX_RESULTS__
$maxNodes = [int]__MAX_NODES__
$maxDepth = [int]__MAX_DEPTH__

function Normalize-Text($value) {
  if ($null -eq $value) { return $null }
  $text = [string]$value
  if ([string]::IsNullOrWhiteSpace($text)) { return $null }
  return $text
}

function Get-Role([int]$controlType) {
  switch ($controlType) {
    50000 { return 'button' }
    50002 { return 'checkbox' }
    50003 { return 'combobox' }
    50004 { return 'textField' }
    50005 { return 'link' }
    50006 { return 'image' }
    50007 { return 'listItem' }
    50008 { return 'list' }
    50009 { return 'menu' }
    50011 { return 'menuItem' }
    50012 { return 'progressBar' }
    50013 { return 'radio' }
    50014 { return 'scrollView' }
    50015 { return 'slider' }
    50018 { return 'list' }
    50020 { return 'text' }
    50023 { return 'list' }
    50024 { return 'listItem' }
    50032 { return 'window' }
    default { return 'unknown' }
  }
}

function Add-Element([System.Windows.Automation.AutomationElement]$element, [int]$depth) {
  if ($null -eq $element -or $depth -gt $maxDepth -or $nodes.Count -ge $maxNodes) { return }

  $role = 'unknown'
  $automationId = $null
  $name = $null
  $helpText = $null
  $left = 0
  $top = 0
  $right = 0
  $bottom = 0
  $enabled = $true

  try { $role = Get-Role ([int]$element.Current.ControlType.Id) } catch { }
  try { $automationId = Normalize-Text $element.Current.AutomationId } catch { }
  try { $name = Normalize-Text $element.Current.Name } catch { }
  try { $helpText = Normalize-Text $element.Current.HelpText } catch { }
  try {
    $rectangle = $element.Current.BoundingRectangle
    $left = [int][Math]::Round($rectangle.Left)
    $top = [int][Math]::Round($rectangle.Top)
    $right = [int][Math]::Round($rectangle.Right)
    $bottom = [int][Math]::Round($rectangle.Bottom)
  } catch { }
  try { $enabled = [bool]$element.Current.IsEnabled } catch { }
  $focused = $null
  try { $focused = [bool]$element.Current.HasKeyboardFocus } catch { }
  $checked = $null
  try {
    $toggle = $element.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    if ($null -ne $toggle) {
      $checked = ([System.Windows.Automation.TogglePattern]$toggle).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On
    }
  } catch { }
  $selected = $null
  try {
    $selection = $element.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
    if ($null -ne $selection) {
      $selected = [bool]([System.Windows.Automation.SelectionItemPattern]$selection).Current.IsSelected
    }
  } catch { }
  $nativeValue = $null
  try {
    $valuePattern = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    if ($null -ne $valuePattern) {
      $nativeValue = Normalize-Text ([System.Windows.Automation.ValuePattern]$valuePattern).Current.Value
    }
  } catch { }

  if ($right -gt $left -and $bottom -gt $top -and $nodes.Count -lt $maxResults) {
    $contentDescription = $helpText
    if ($null -eq $contentDescription) { $contentDescription = $name }
    [void]$nodes.Add([pscustomobject]@{
      className = $role
      resourceId = $automationId
      text = $name
      contentDescription = $contentDescription
      left = $left
      top = $top
      right = $right
      bottom = $bottom
      clickable = @('button', 'link', 'menuItem', 'radio', 'checkbox') -contains $role
      enabled = $enabled
      focused = $focused
      checked = $checked
      selected = $selected
      value = $nativeValue
    })
  }

  if ($nodes.Count -ge $maxResults -or $nodes.Count -ge $maxNodes) { return }
  try {
    $child = $walker.GetFirstChild($element)
    while ($null -ne $child -and $nodes.Count -lt $maxResults -and $nodes.Count -lt $maxNodes) {
      Add-Element $child ($depth + 1)
      $next = $walker.GetNextSibling($child)
      if ($null -eq $next -or $next.Equals($child)) { break }
      $child = $next
    }
  } catch { }
}

Add-Element $target 0
if ($nodes.Count -eq 0) { '[]' } else { $nodes | ConvertTo-Json -Depth 4 -Compress }
