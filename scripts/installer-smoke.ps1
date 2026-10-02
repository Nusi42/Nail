<#
.SYNOPSIS
  Installer smoke test (plan M2b WP44/WP45, M4b WP112): install -> launch ->
  update -> uninstall of the per-user NSIS installer, into a temporary
  directory, without admin rights.

.DESCRIPTION
  1. Silent install: Nail_<v>_<arch>-setup.exe /S /NS /AUTOSTART=0 /ADDTOPATH=1 /MAPI=0 /D=<tmp>\app
  2. Checks the files, the uninstall entry, the Windows registration
     (RegisteredApplications, Capabilities, ProgIDs), the PATH entry and the
     installation facts the app reads (HKCU\Software\Nail\Install:
     Installer=nsis, Scope=per-user, InstallDir, Version, Mapi=0).
  3. bin\nail.exe --version runs (static C runtime, no redistributable).
  4. Launch: a `portable` marker is put next to Nail.exe first, so the app
     keeps its data, config and WebView2 profile inside the temporary
     directory, never in the real profile. Nail.exe "mailto:..." must start
     the core (the MCP endpoint file appears), stay up, and log the pending
     launch request.
  5. Update (with -UpdateEndpoint): the channel manifest on the loopback
     update server is read the way the app's updater does
     (scripts/update-check.mjs: newer version, windows-<arch>-nsis entry,
     signature of the update key for that version), the verified installer
     runs with /S /UPDATE like the Tauri updater's, and the installation
     is checked again: new version in the facts and the uninstall entry,
     same directory, the options (PATH, no autostart) and the data kept, the
     updated app starts.
  6. Silent uninstall: files, registration, facts and PATH entry are gone;
     the app's data directory is kept (uninstall keeps data by default).

  Refuses to run when a Nail installation already exists for this user.
  Writes only per-user registry keys and removes them again.

.PARAMETER Installer
  Path to the setup .exe. Default: the newest in <build dir>\dist.

.PARAMETER UpdateEndpoint
  A channel manifest URL (http://127.0.0.1:<port>/stable/latest.json) that
  announces a newer version; enables step 5.

.PARAMETER UpdatePubkey
  The update key's public half (Tauri form) the build embeds; needed with
  -UpdateEndpoint.

.EXAMPLE
  powershell -File scripts/installer-smoke.ps1 -Installer C:\...\Nail_0.1.0_x64-setup.exe
#>
[CmdletBinding()]
param(
  [string]$Installer,
  [string]$UpdateEndpoint,
  [string]$UpdatePubkey,
  [int]$StartTimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Nail'
$factsKey = 'HKCU:\Software\Nail\Install'
$repoRoot = Split-Path -Parent $PSScriptRoot

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
function Machine-Mapi-Dll {
  $view = [Microsoft.Win32.RegistryView]::Registry64
  $hklm = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, $view)
  $key = $hklm.OpenSubKey('Software\Clients\Mail\Nail')
  if ($null -eq $key) { return $null }
  try { return $key.GetValue('DLLPath') } finally { $key.Close() }
}

if (-not $Installer) {
  $base = if ($env:NAIL_BUILD_DIR) { $env:NAIL_BUILD_DIR } else { Join-Path $env:LOCALAPPDATA 'nail-build' }
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
if ($UpdateEndpoint -and -not $UpdatePubkey) { Fail '-UpdateEndpoint needs -UpdatePubkey' }
if (Test-Path $uninstallKey) {
  Fail "Nail is already installed for this user ($uninstallKey); refusing to touch it"
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("nail-installer-smoke-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
$app = Join-Path $work 'app'
$bin = Join-Path $app 'bin'
New-Item -ItemType Directory -Force $work | Out-Null
Write-Host "installer: $Installer ($version, $arch)"
Write-Host "work dir:  $work"

$proc = $null
function Launch-Nail([string]$label) {
  $stderr = Join-Path $work "nail-app-$label.stderr.log"
  $stdout = Join-Path $work "nail-app-$label.stdout.log"
  $endpoint = Join-Path $app 'data\mcp-endpoint.json'
  Remove-Item -Force $endpoint -ErrorAction SilentlyContinue
  $env:NAIL_LOG = 'info'
  $script:proc = Start-Process -FilePath (Join-Path $app 'Nail.exe') -ArgumentList @('--', 'mailto:smoke@example.org?subject=Smoke') `
    -RedirectStandardError $stderr -RedirectStandardOutput $stdout -PassThru
  $deadline = (Get-Date).AddSeconds($StartTimeoutSeconds)
  while (-not (Test-Path $endpoint) -and -not $script:proc.HasExited -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 250
  }
  Check (Test-Path $endpoint) "the core started ($label; portable data dir has mcp-endpoint.json)"
  Start-Sleep -Seconds 3
  Check (-not $script:proc.HasExited) "Nail.exe is still running ($label; window up)"
  $logText = (Get-Content $stdout -Raw) + (Get-Content $stderr -Raw)
  Check ($logText -match 'launch requests pending') "the mailto: argument became a launch request ($label)"
  Stop-Process -Id $script:proc.Id -Force
  $script:proc.WaitForExit(10000) | Out-Null
  $script:proc = $null
}

function Check-Facts([string]$expectedVersion) {
  Check ((Reg-Value $factsKey 'Installer') -eq 'nsis') 'facts: Installer = nsis'
  Check ((Reg-Value $factsKey 'Scope') -eq 'per-user') 'facts: Scope = per-user'
  Check ((Reg-Value $factsKey 'InstallDir') -ieq $app) "facts: InstallDir = $app"
  Check ((Reg-Value $factsKey 'Version') -eq $expectedVersion) "facts: Version = $expectedVersion"
  Check ((Reg-Value $factsKey 'Mapi') -eq 0) 'facts: Mapi = 0 (/MAPI=0)'
  Check ($null -eq (Reg-Value $factsKey 'DisableUpdates')) 'facts: no DisableUpdates (per-user installs update themselves)'
  Check ((Reg-Value $uninstallKey 'DisplayVersion') -eq $expectedVersion) "uninstall entry: DisplayVersion = $expectedVersion"
}

try {
  # --- install -------------------------------------------------------------
  Write-Host 'install'
  # /D must be last and unquoted (NSIS).
  $p = Start-Process -FilePath $Installer -ArgumentList @('/S', '/NS', '/AUTOSTART=0', '/ADDTOPATH=1', '/MAPI=0', "/D=$app") -Wait -PassThru
  Check ($p.ExitCode -eq 0) "installer exit code 0 (was $($p.ExitCode))"
  Check (Test-Path (Join-Path $app 'Nail.exe')) 'Nail.exe installed'
  Check (Test-Path (Join-Path $bin 'nail.exe')) 'bin\nail.exe installed'
  Check (Test-Path (Join-Path $app 'ui\index.html')) 'ui\index.html installed (remote UI, nail serve)'
  Check (Test-Path (Join-Path $app 'uninstall.exe')) 'uninstall.exe written'
  Check (Test-Path $uninstallKey) 'uninstall entry (HKCU)'
  if (Test-Path (Join-Path $app "mapi\$arch\NailMapi.dll")) {
    Write-Host "  ok  mapi\$arch\NailMapi.dll installed"
  } else {
    Write-Host "  --  no mapi\$arch\NailMapi.dll in this build (--no-mapi)"
  }
  Check ((Machine-Mapi-Dll) -notlike "$app\*") 'no machine-wide MAPI registration for this installation (/MAPI=0)'

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
  Check-Facts $version

  $cliVersion = & (Join-Path $bin 'nail.exe') --version
  Check ($LASTEXITCODE -eq 0 -and $cliVersion -match '^nail ') "nail --version: $cliVersion"

  # --- launch --------------------------------------------------------------
  Write-Host 'launch'
  Set-Content -Path (Join-Path $app 'portable') -Value '' -NoNewline
  Launch-Nail 'installed'

  # --- update --------------------------------------------------------------
  if ($UpdateEndpoint) {
    Write-Host "update ($UpdateEndpoint)"
    $download = Join-Path $work 'update-setup.exe'
    $result = & node (Join-Path $repoRoot 'scripts\update-check.mjs') --endpoint $UpdateEndpoint --current $version `
      --arch $arch --installer nsis --pubkey-file $UpdatePubkey --out $download
    Check ($LASTEXITCODE -eq 0) "update check succeeded: $result"
    $update = $result | ConvertFrom-Json
    Check ($update.available -eq $true) "an update is announced (version $($update.version), $($update.target))"
    Check (Test-Path $download) 'the update was downloaded and its signature verified'
    # What the Tauri updater runs (passive there; silent here). No /D: the
    # installer finds the installation itself.
    $p = Start-Process -FilePath $download -ArgumentList @('/S', '/UPDATE') -Wait -PassThru
    Check ($p.ExitCode -eq 0) "update installer exit code 0 (was $($p.ExitCode))"
    Check (Test-Path (Join-Path $app 'Nail.exe')) 'Nail.exe still in the same directory'
    Check-Facts $update.version
    Check (Path-Has $bin) 'bin\ still on the user PATH (update keeps the options)'
    Check ((Reg-Value $uninstallKey 'NailAddToPath') -eq 1) 'the PATH choice is still remembered'
    Check (-not (Reg-Value 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' 'Nail')) 'still no autostart entry'
    Check (Test-Path (Join-Path $app 'data\nail.sqlite3')) 'data kept across the update'
    Check (Test-Path (Join-Path $app 'portable')) 'the portable marker (not an installer file) kept'
    $version = $update.version
    Launch-Nail 'updated'
  }

  # --- uninstall -----------------------------------------------------------
  Write-Host 'uninstall'
  # _?= runs the uninstaller in place and waits for it.
  $p = Start-Process -FilePath (Join-Path $app 'uninstall.exe') -ArgumentList @('/S', "_?=$app") -Wait -PassThru
  Check ($p.ExitCode -eq 0) "uninstaller exit code 0 (was $($p.ExitCode))"
  Check (-not (Test-Path (Join-Path $app 'Nail.exe'))) 'Nail.exe removed'
  Check (-not (Test-Path (Join-Path $bin 'nail.exe'))) 'bin\nail.exe removed'
  Check (-not (Test-Path (Join-Path $app 'mapi'))) 'mapi\ removed'
  Check (-not (Test-Path $uninstallKey)) 'uninstall entry removed'
  Check (-not (Reg-Value 'HKCU:\Software\RegisteredApplications' 'Nail')) 'RegisteredApplications entry removed'
  Check (-not (Test-Path $factsKey)) 'installation facts removed'
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
