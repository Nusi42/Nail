<#
.SYNOPSIS
  Installer smoke test beside an existing Nail installation (ADR 0186): checks
  the NSIS setup and runs exactly the files it installs, without running the
  setup.

.DESCRIPTION
  `installer-smoke.ps1` installs, updates and uninstalls for real, so it
  refuses to run where Nail is installed for the user (the same HKCU keys).
  This script never runs the setup, never reads or writes Nail's registry
  keys, never touches an installed Nail, its profile, its WebView2 data or
  its processes:

  1. The setup: name, PE header, NSIS marker, its SHA-256 in the build's
     SHA256SUMS file, the copy the bundler wrote (bundle\nsis) is the same
     file, the Authenticode state (reported; -RequireSignature fails an
     unsigned setup).
  2. The payload: the bundler's installer.nsi of that build lists every
     file the setup installs (the main binary plus `File /oname=<target>
     <source>`). Every source must exist; they are copied into a temporary
     directory in the installed layout ($INSTDIR = <tmp>\app).
  3. bin\nail.exe --version runs (static C runtime).
  4. bin\nail.exe serve --seed demo (port 0, temporary data / config, the
     payload's ui\ folder, no MCP server) prints its URL; the page and one
     of its scripts load.
  5. Nail.exe --seed demo with temporary directories: a seeded run uses no
     single-instance hand-over, an in-memory registry, no Run key or MAPI
     pipe, and its own WebView2 profile (WEBVIEW2_USER_DATA_FOLDER also
     points into the temporary directory). Its WebView2 must load the built
     UI (remote debugging on a free loopback port) and the app must stay
     up; then this process tree (and only it) is stopped.

  Not verified here (the owner's own install / upgrade does it): the NSIS
  script's registry writes, shortcuts, PATH entry, file associations,
  uninstaller and update-in-place.

.PARAMETER Installer
  Path to the setup .exe. Default: the newest in <build dir>\dist.

.PARAMETER TargetDir
  The cargo target directory of the installer build. Default:
  <build dir>\installer-target.

.EXAMPLE
  powershell -File scripts/installer-payload-smoke.ps1
#>
[CmdletBinding()]
param(
  [string]$Installer,
  [string]$TargetDir,
  [switch]$RequireSignature,
  [int]$StartTimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Fail([string]$message) { throw "installer payload smoke: $message" }
function Check([bool]$condition, [string]$message) {
  if (-not $condition) { Fail $message }
  Write-Host "  ok  $message"
}
function Free-Port {
  $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $listener.Start()
  try { return $listener.LocalEndpoint.Port } finally { $listener.Stop() }
}
function Stop-Tree([System.Diagnostics.Process]$process) {
  if ($null -eq $process -or $process.HasExited) { return }
  # Only the process this script started, with its WebView2 children.
  & taskkill.exe /PID $process.Id /T /F | Out-Null
  $process.WaitForExit(10000) | Out-Null
}

$base = if ($env:NAIL_BUILD_DIR) { $env:NAIL_BUILD_DIR } else { Join-Path $PSScriptRoot '..\.nail-build' }
if (-not $Installer) {
  $Installer = Get-ChildItem (Join-Path $base 'dist') -Filter 'Nail_*-setup.exe' |
    Where-Object { $_.Name -notlike '*-offline-setup.exe' } |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
  if (-not $Installer) { Fail "no installer in $base\dist; run node scripts/build-installer.mjs" }
}
$Installer = (Resolve-Path $Installer).Path
if ((Split-Path -Leaf $Installer) -notmatch '^Nail_(.+)_(x64|arm64)-setup\.exe$') {
  Fail "unexpected installer name $Installer"
}
$version = $Matches[1]
$arch = $Matches[2]
if (-not $TargetDir) { $TargetDir = Join-Path $base 'installer-target' }
$triple = if ($arch -eq 'x64') { 'x86_64-pc-windows-msvc' } else { 'aarch64-pc-windows-msvc' }
$release = Join-Path $TargetDir "$triple\release"
$nsi = Join-Path $release "nsis\$arch\installer.nsi"

$work = Join-Path ([IO.Path]::GetTempPath()) ('nail-payload-smoke-' + [Guid]::NewGuid().ToString('N').Substring(0, 8))
$app = Join-Path $work 'app'
New-Item -ItemType Directory -Force $app | Out-Null
Write-Host "installer: $Installer ($version, $arch)"
Write-Host "work dir:  $work"

$serve = $null
$desktop = $null
try {
  # --- 1. the setup ---------------------------------------------------------
  Write-Host 'setup'
  $bytes = [IO.File]::ReadAllBytes($Installer)
  Check ($bytes.Length -gt 1MB) "size $([math]::Round($bytes.Length / 1MB, 1)) MiB"
  Check ($bytes[0] -eq 0x4D -and $bytes[1] -eq 0x5A) 'PE header (MZ)'
  $latin = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
  Check ($latin.Contains('NullsoftInst')) 'NSIS installer (Nullsoft marker)'
  $hash = (Get-FileHash -Algorithm SHA256 $Installer).Hash.ToLowerInvariant()
  $sums = Join-Path (Split-Path $Installer) "Nail_${version}_$arch-SHA256SUMS.txt"
  Check (Test-Path $sums) 'SHA256SUMS of the build present'
  Check ((Get-Content $sums -Raw) -match "(?im)^$hash\s+\*?$([regex]::Escape((Split-Path -Leaf $Installer)))\s*$") 'SHA-256 matches SHA256SUMS'
  $bundled = Get-ChildItem (Join-Path $release 'bundle\nsis') -Filter "*_$arch-setup.exe" -ErrorAction SilentlyContinue |
    Where-Object { (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLowerInvariant() -eq $hash }
  Check ($null -ne $bundled) "the setup is the bundler's output of this build ($release\bundle\nsis)"
  $signature = Get-AuthenticodeSignature -FilePath $Installer
  Write-Host "  --  Authenticode: $($signature.Status)"
  if ($RequireSignature) { Check ($signature.Status -eq 'Valid') 'Authenticode signature valid' }

  # --- 2. the payload ---------------------------------------------------------
  Write-Host 'payload'
  Check (Test-Path $nsi) "installer.nsi of the build ($nsi)"
  Check ((Get-Item $nsi).LastWriteTime -le (Get-Item $Installer).LastWriteTime) 'installer.nsi is not newer than the setup'
  $script = Get-Content $nsi
  $main = $script | Select-String -Pattern '^!define MAINBINARYSRCPATH "(.+)"$' | Select-Object -First 1
  Check ($null -ne $main) 'main binary named'
  $mainSource = $main.Matches[0].Groups[1].Value
  Check (Test-Path $mainSource) "main binary present ($mainSource)"
  Copy-Item $mainSource (Join-Path $app 'Nail.exe')
  $count = 0
  foreach ($line in $script) {
    if ($line -match '^\s*File /a "/oname=([^"]+)" "([^"]+)"\s*$') {
      $target = Join-Path $app $Matches[1]
      $source = $Matches[2]
      if (-not (Test-Path $source)) { Fail "payload file missing: $source (for $($Matches[1]))" }
      New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
      Copy-Item $source $target
      $count++
    }
  }
  Check ($count -gt 10) "$count further payload files staged"
  foreach ($file in 'Nail.exe', 'bin\nail.exe', 'ui\index.html', "mapi\$arch\NailMapi.dll") {
    Check (Test-Path (Join-Path $app $file)) "installed layout has $file"
  }

  # --- 3. the CLI ------------------------------------------------------------
  Write-Host 'cli'
  $cli = Join-Path $app 'bin\nail.exe'
  $cliVersion = & $cli --version
  Check ($LASTEXITCODE -eq 0 -and $cliVersion -match "^nail $([regex]::Escape($version))") "nail --version: $cliVersion"

  # --- 4. nail serve on the payload's UI -------------------------------------------
  Write-Host 'serve'
  $serveOut = Join-Path $work 'serve.stdout.log'
  $serveErr = Join-Path $work 'serve.stderr.log'
  $serve = Start-Process -FilePath $cli -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput $serveOut -RedirectStandardError $serveErr -ArgumentList @(
      'serve', '--seed', 'demo', '--port', '0', '--no-mcp',
      '--data-dir', (Join-Path $work 'serve-data'), '--config-dir', (Join-Path $work 'serve-config'),
      '--ui-dir', (Join-Path $app 'ui'))
  $deadline = (Get-Date).AddSeconds($StartTimeoutSeconds)
  $url = $null
  while (-not $url -and -not $serve.HasExited -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 250
    if (Test-Path $serveOut) {
      $first = Get-Content $serveOut -TotalCount 1 -ErrorAction SilentlyContinue
      if ($first -match '^https?://127\.0\.0\.1:\d+') { $url = $first.Trim() }
    }
  }
  Check ([bool]$url) "nail serve printed its URL ($url)"
  $page = Invoke-WebRequest -UseBasicParsing -Uri $url
  Check ($page.StatusCode -eq 200 -and $page.Content -match '<script[^>]+src="([^"]+\.js)"') 'the page loads'
  $asset = Invoke-WebRequest -UseBasicParsing -Uri ([Uri]::new([Uri]$url, $Matches[1]))
  Check ($asset.StatusCode -eq 200 -and $asset.Content.Length -gt 1000) "its script loads ($($Matches[1]))"
  Stop-Tree $serve
  $serve = $null

  # --- 5. the desktop app, seeded -------------------------------------------------
  Write-Host 'desktop'
  $port = Free-Port
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$port"
  $env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $work 'webview'
  $env:NAIL_LOG = 'warn'
  $desktop = Start-Process -FilePath (Join-Path $app 'Nail.exe') -PassThru `
    -RedirectStandardError (Join-Path $work 'desktop.stderr.log') -RedirectStandardOutput (Join-Path $work 'desktop.stdout.log') `
    -ArgumentList @('--seed', 'demo', '--data-dir', (Join-Path $work 'desktop-data'), '--config-dir', (Join-Path $work 'desktop-config'))
  Remove-Item Env:\WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, Env:\WEBVIEW2_USER_DATA_FOLDER, Env:\NAIL_LOG
  $deadline = (Get-Date).AddSeconds($StartTimeoutSeconds)
  $loaded = $null
  while (-not $loaded -and -not $desktop.HasExited -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 500
    try {
      $targets = Invoke-RestMethod -Uri "http://127.0.0.1:$port/json" -TimeoutSec 2
      $loaded = @($targets | Where-Object { $_.type -eq 'page' -and $_.url -match '^(tauri://localhost|https?://tauri\.localhost)' }) | Select-Object -First 1
    } catch { $loaded = $null }
  }
  Check ($null -ne $loaded) "Nail.exe's WebView2 loaded the built UI ($($loaded.url))"
  Start-Sleep -Seconds 3
  Check (-not $desktop.HasExited) 'Nail.exe is still running'
  Check (Test-Path (Join-Path $work 'desktop-data\webview')) 'its WebView2 profile is in the temporary data directory'
  Stop-Tree $desktop
  $desktop = $null
  Write-Host 'installer payload smoke: passed'
}
finally {
  Stop-Tree $serve
  Stop-Tree $desktop
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
