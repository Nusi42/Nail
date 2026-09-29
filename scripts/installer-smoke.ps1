<#
.SYNOPSIS
  Installer smoke test (plan M2b WP44/WP45): install -> launch -> uninstall,
  per-user, into a temporary directory, without admin rights.

.DESCRIPTION
  1. Silent install: Nail_<v>_x64-setup.exe /S /NS /AUTOSTART=0 /ADDTOPATH=1 /D=<tmp>\app
  2. Checks the files, the uninstall entry, the Windows registration
     (RegisteredApplications, Capabilities, ProgIDs) and the PATH entry.
  3. bin\nail.exe --version runs (static C runtime, no redistributable).
  4. Launch: a `portable` marker is put next to Nail.exe first, so the app
     keeps its data, config and WebView2 profile inside the temporary
     directory, never in the real profile. Nail.exe "mailto:..." must start
     the core (the MCP endpoint file appears), stay up, and log the pending
     launch request.
  5. Silent uninstall: files, registration and PATH entry are gone; the
     app's data directory is kept (uninstall keeps data by default).

  Refuses to run when a Nail installation already exists for this user.
  Writes only per-user registry keys and removes them again.

.PARAMETER Installer
  Path to the setup .exe. Default: the newest in <build dir>\dist.

.EXAMPLE
  powershell -File scripts/installer-smoke.ps1 -Installer C:\...\Nail_0.1.0_x64-setup.exe
#>
[CmdletBinding()]
param(
  [string]$Installer,
  [int]$StartTimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Nail'

function Fail([string]$message) { throw "installer smoke: $message" }
function Check([bool]$condition, [string]$message) {
  if (-not $condition) { Fail $message }
  Write-Host "  ok  $message"
}
function User-Path { [Environment]::GetEnvironmentVariable('Path', 'User') }
function Path-Has([string]$dir) {
  $p = User-Path
  if (-not $p) { return $false }
  return @($p.Split(';') | Where-Object { $_.TrimEnd('\') -ieq $dir.TrimEnd('\') }).Count -gt 0
}
function Reg-Value([string]$key, [string]$name) {
  $item = Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
  if ($null -eq $item) { return $null }
  $property = $item.PSObject.Properties[$name]
  if ($property) { return $property.Value } else { return $null }
}

if (-not $Installer) {
  $base = if ($env:NAIL_BUILD_DIR) { $env:NAIL_BUILD_DIR } else { Join-Path $env:LOCALAPPDATA 'nail-build' }
  $Installer = Get-ChildItem (Join-Path $base 'dist') -Filter 'Nail_*-setup.exe' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
  if (-not $Installer) { Fail "no installer in $base\dist; run node scripts/build-installer.mjs" }
}
$Installer = (Resolve-Path $Installer).Path
if (Test-Path $uninstallKey) {
  Fail "Nail is already installed for this user ($uninstallKey); refusing to touch it"
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("nail-installer-smoke-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
$app = Join-Path $work 'app'
$bin = Join-Path $app 'bin'
New-Item -ItemType Directory -Force $work | Out-Null
Write-Host "installer: $Installer"
Write-Host "work dir:  $work"

$proc = $null
try {
  # --- install -------------------------------------------------------------
  Write-Host 'install'
  # /D must be last and unquoted (NSIS).
  $p = Start-Process -FilePath $Installer -ArgumentList @('/S', '/NS', '/AUTOSTART=0', '/ADDTOPATH=1', "/D=$app") -Wait -PassThru
  Check ($p.ExitCode -eq 0) "installer exit code 0 (was $($p.ExitCode))"
  Check (Test-Path (Join-Path $app 'Nail.exe')) 'Nail.exe installed'
  Check (Test-Path (Join-Path $bin 'nail.exe')) 'bin\nail.exe installed'
  Check (Test-Path (Join-Path $app 'ui\index.html')) 'ui\index.html installed (remote UI, nail serve)'
  Check (Test-Path (Join-Path $app 'uninstall.exe')) 'uninstall.exe written'
  Check (Test-Path $uninstallKey) 'uninstall entry (HKCU)'

  $caps = 'HKCU:\Software\Nail\Capabilities'
  Check ((Reg-Value 'HKCU:\Software\RegisteredApplications' 'Nail') -eq 'Software\Nail\Capabilities') 'listed in RegisteredApplications'
  foreach ($scheme in 'mailto', 'webcal', 'webcals') {
    Check ([bool](Reg-Value "$caps\URLAssociations" $scheme)) "URL association $scheme"
  }
  foreach ($ext in '.eml', '.ics', '.vcf') {
    $progid = Reg-Value "$caps\FileAssociations" $ext
    Check ([bool]$progid) "file association $ext"
    $command = Reg-Value "HKCU:\Software\Classes\$progid\shell\open\command" '(default)'
    Check ($command -like "*$app\Nail.exe*") "$progid opens with the installed Nail.exe"
    Check ($null -ne (Reg-Value "HKCU:\Software\Classes\$ext\OpenWithProgids" $progid)) "$ext lists $progid under Open with"
  }
  Check (Path-Has $bin) 'bin\ is on the user PATH'
  Check (-not (Reg-Value 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' 'Nail')) 'no autostart entry (/AUTOSTART=0)'

  $version = & (Join-Path $bin 'nail.exe') --version
  Check ($LASTEXITCODE -eq 0 -and $version -match '^nail ') "nail --version: $version"

  # --- launch --------------------------------------------------------------
  Write-Host 'launch'
  Set-Content -Path (Join-Path $app 'portable') -Value '' -NoNewline
  $stderr = Join-Path $work 'nail-app.stderr.log'
  $stdout = Join-Path $work 'nail-app.stdout.log'
  $env:NAIL_LOG = 'info'
  $proc = Start-Process -FilePath (Join-Path $app 'Nail.exe') -ArgumentList @('--', 'mailto:smoke@example.org?subject=Smoke') `
    -RedirectStandardError $stderr -RedirectStandardOutput $stdout -PassThru
  $endpoint = Join-Path $app 'data\mcp-endpoint.json'
  $deadline = (Get-Date).AddSeconds($StartTimeoutSeconds)
  while (-not (Test-Path $endpoint) -and -not $proc.HasExited -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 250
  }
  Check (Test-Path $endpoint) 'the core started (portable data dir has mcp-endpoint.json)'
  Start-Sleep -Seconds 3
  Check (-not $proc.HasExited) 'Nail.exe is still running (window up)'
  $logText = (Get-Content $stdout -Raw) + (Get-Content $stderr -Raw)
  Check ($logText -match 'launch requests pending') 'the mailto: argument became a launch request'
  Stop-Process -Id $proc.Id -Force
  $proc.WaitForExit(10000) | Out-Null
  $proc = $null

  # --- uninstall -----------------------------------------------------------
  Write-Host 'uninstall'
  # _?= runs the uninstaller in place and waits for it.
  $p = Start-Process -FilePath (Join-Path $app 'uninstall.exe') -ArgumentList @('/S', "_?=$app") -Wait -PassThru
  Check ($p.ExitCode -eq 0) "uninstaller exit code 0 (was $($p.ExitCode))"
  Check (-not (Test-Path (Join-Path $app 'Nail.exe'))) 'Nail.exe removed'
  Check (-not (Test-Path (Join-Path $bin 'nail.exe'))) 'bin\nail.exe removed'
  Check (-not (Test-Path $uninstallKey)) 'uninstall entry removed'
  Check (-not (Reg-Value 'HKCU:\Software\RegisteredApplications' 'Nail')) 'RegisteredApplications entry removed'
  Check (-not (Test-Path 'HKCU:\Software\Nail')) 'Capabilities and the remembered install directory removed'
  Check (-not (Test-Path 'HKCU:\Software\Classes\Nail.Url.mailto')) 'mailto ProgID removed'
  Check (-not (Path-Has $bin)) 'bin\ removed from the user PATH'
  Check (Test-Path (Join-Path $app 'data\nail.sqlite3')) 'data kept (no /DELETEDATA)'
  Write-Host 'installer smoke: passed'
}
finally {
  if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
  # Leave nothing behind if a step failed half-way.
  if (Test-Path (Join-Path $app 'uninstall.exe')) {
    Start-Process -FilePath (Join-Path $app 'uninstall.exe') -ArgumentList @('/S', "_?=$app") -Wait -ErrorAction SilentlyContinue | Out-Null
  }
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
