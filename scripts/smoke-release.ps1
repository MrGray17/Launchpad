param(
  [string]$BinaryPath = "src-tauri\target\release\launchpad.exe"
)

$resolvedBinary = (Resolve-Path -LiteralPath $BinaryPath).Path
$launchpadProcess = Start-Process -FilePath $resolvedBinary -PassThru
$secondProcess = $null
try {
  Start-Sleep -Seconds 8
  $running = Get-Process -Id $launchpadProcess.Id -ErrorAction Stop
  if ($running.Path -ne $resolvedBinary) {
    throw "Unexpected process path: $($running.Path)"
  }
  if (-not $running.Responding) {
    throw "Launchpad started but its Windows process is not responding."
  }
  if ($running.MainWindowTitle -ne "Launchpad") {
    throw "Launchpad started without its expected main window title."
  }

  $secondProcess = Start-Process -FilePath $resolvedBinary -PassThru
  Start-Sleep -Seconds 3
  $secondProcess.Refresh()
  if (-not $secondProcess.HasExited) {
    throw "A second Launchpad process remained alive."
  }
  $matchingInstances = @(Get-Process -Name "launchpad" -ErrorAction SilentlyContinue | Where-Object {
    $_.Path -eq $resolvedBinary
  })
  if ($matchingInstances.Count -ne 1 -or $matchingInstances[0].Id -ne $running.Id) {
    throw "Launchpad's single-instance contract was not preserved."
  }

  [pscustomobject]@{
    Id = $running.Id
    Path = $running.Path
    Responding = $running.Responding
    MainWindowTitle = $running.MainWindowTitle
    SecondInstanceExited = $secondProcess.HasExited
  }
}
finally {
  if ($null -ne $secondProcess -and -not $secondProcess.HasExited) {
    Stop-Process -Id $secondProcess.Id
  }
  if (-not $launchpadProcess.HasExited) {
    Stop-Process -Id $launchpadProcess.Id
  }
}
