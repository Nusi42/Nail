<#
.SYNOPSIS
  MSI smoke test (plan M4b WP112): silent per-machine install with
  properties -> launch -> upgrade -> uninstall. Needs an elevated shell
  (CI: windows-latest runs elevated).

.DESCRIPTION
  1. msiexec /i Nail_<v>_<arch>_en-US.msi /qn INSTALLDIR=<tmp>\app ADDTOPATH=1 MAPI=1
     (DISABLEUPDATES and AUTOSTART left at their defaults).
  2. Checks the files and, under HKLM, the installation facts the app reads
     (Software\Nail\Install: Installer=msi, Scope=per-machine, InstallDir,
     DisableUpdates=1 - the in-app updater is off -, Mapi=1), the
     registration (RegisteredApplications, Capabilities, ProgIDs), the
     Simple MAPI client in both registry views (mapi\<arch> and mapi\x86
     DLLs), bin\ on the machine PATH, no Run entry.
  3. Launch with --data-dir / --config-dir in the temporary directory
     (no `portable` marker: the app must see itself as the MSI install):
     the core starts, and the shell reports kind msi with updates disabled
     (msi-managed, DISABLEUPDATES=1 - the updater is hidden; WP115).
  4. Upgrade (with -UpgradeMsi): msiexec /i <newer msi> /qn AUTOSTART=1 -
     the command line wins (Run entry), everything not given is kept
     (DisableUpdates=1, Mapi=1, PATH), the facts name the new version.
  5. msiexec /x: files, facts, registration, MAPI keys, PATH and Run entry
     are gone; the data directory is kept.

  Refuses to run when Nail is installed per machine already.

.PARAMETER Msi
  The MSI. Default: the newest *_en-US.msi in <build dir>\dist.

.PARAMETER UpgradeMsi
  A newer MSI of the same product; enables step 4.
#>
[CmdletBinding()]
param(
  [string]$Msi,
  [string]$UpgradeMsi,
  [int]$StartTimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Fail([string]$message) { throw "msi smoke: $message" }
function Check([bool]$condition, [string]$message) {
  if (-not $condition) { Fail $message }
  Write-Host "  ok  $message"
}
function Hklm-Value([string]$key, [string]$name, [string]$view = 'Registry64') {
  $hklm = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine,
    [Microsoft.Win32.RegistryView]::$view)
  $k = $hklm.OpenSubKey($key)
  if ($null -eq $k) { return $null }
  try { return $k.GetValue($name) } finally { $k.Close() }
}
function Hklm-Exists([string]$key, [string]$view = 'Registry64') {
  $hklm = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine,
    [Microsoft.Win32.RegistryView]::$view)
  $k = $hklm.OpenSubKey($key)
  if ($null -eq $k) { return $false }
  $k.Close()
  return $true
}
function Machine-Path-Has([string]$dir) {
  $p = [Environment]::GetEnvironmentVariable('Path', 'Machine')
  return @($p.Split(';') | Where-Object { $_.TrimEnd('\') -ieq $dir.TrimEnd('\') }).Count -gt 0
}
function Msiexec([string[]]$arguments, [string]$log) {
  $p = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList ($arguments + @('/qn', '/norestart', '/l*v', "`"$log`"")) -Wait -PassThru
  return $p.ExitCode
}

$identity = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $identity.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  Fail 'needs an elevated shell (per-machine install)'
}
if (-not $Msi) {
  $base = if ($env:NAIL_BUILD_DIR) { $env:NAIL_BUILD_DIR } else { Join-Path $PSScriptRoot '..\.nail-build' }
  $Msi = Get-ChildItem (Join-Path $base 'dist') -Filter 'Nail_*_en-US.msi' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
  if (-not $Msi) { Fail "no MSI in $base\dist; run node scripts/build-installer.mjs" }
}
$Msi = (Resolve-Path $Msi).Path
if ((Split-Path -Leaf $Msi) -notmatch '^Nail_(.+)_(x64|arm64)_en-US\.msi$') { Fail "unexpected MSI name $Msi" }
$arch = $Matches[2]
if (Hklm-Exists 'Software\Nail\Install') { Fail 'Nail is installed per machine already; refusing to touch it' }

$facts = 'Software\Nail\Install'
$work = Join-Path ([IO.Path]::GetTempPath()) ("nail-msi-smoke-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
$app = Join-Path $work 'app'
$bin = Join-Path $app 'bin'
New-Item -ItemType Directory -Force $work | Out-Null
Write-Host "msi:      $Msi"
Write-Host "work dir: $work"
$installed = $null
$proc = $null

try {
  # --- install -------------------------------------------------------------
  Write-Host 'install'
  $code = Msiexec @('/i', "`"$Msi`"", "INSTALLDIR=`"$app`"", 'ADDTOPATH=1', 'MAPI=1') (Join-Path $work 'install.log')
  Check ($code -eq 0 -or $code -eq 3010) "msiexec /i exit code 0 (was $code)"
  $installed = $Msi
  Check (Test-Path (Join-Path $app 'Nail.exe')) 'Nail.exe installed'
  Check (Test-Path (Join-Path $bin 'nail.exe')) 'bin\nail.exe installed'
  Check (Test-Path (Join-Path $app "mapi\$arch\NailMapi.dll")) "mapi\$arch\NailMapi.dll installed"
  Check (Test-Path (Join-Path $app 'mapi\x86\NailMapi.dll')) 'mapi\x86\NailMapi.dll installed'

  Check ((Hklm-Value $facts 'Installer') -eq 'msi') 'facts: Installer = msi'
  Check ((Hklm-Value $facts 'Scope') -eq 'per-machine') 'facts: Scope = per-machine'
  Check ((Hklm-Value $facts 'InstallDir').TrimEnd('\') -ieq $app) "facts: InstallDir = $app"
  Check ((Hklm-Value $facts 'DisableUpdates') -eq 1) 'facts: DisableUpdates = 1 (the default: no in-app updates)'
  Check ((Hklm-Value $facts 'Mapi') -eq 1) 'facts: Mapi = 1 (MAPI=1)'
  $firstVersion = Hklm-Value $facts 'Version'
  Check ([bool]$firstVersion) "facts: Version = $firstVersion"

  Check ((Hklm-Value 'Software\RegisteredApplications' 'Nail') -eq 'Software\Nail\Capabilities') 'listed in RegisteredApplications (machine)'
  foreach ($scheme in 'mailto', 'webcal', 'webcals') {
    Check ([bool](Hklm-Value 'Software\Nail\Capabilities\URLAssociations' $scheme)) "URL association $scheme"
  }
  foreach ($ext in '.eml', '.ics', '.vcf') {
    $progid = Hklm-Value 'Software\Nail\Capabilities\FileAssociations' $ext
    Check ([bool]$progid) "file association $ext"
    Check ((Hklm-Value "Software\Classes\$progid\shell\open\command" '') -like "*$app\Nail.exe`" -- `"%1`"") "$progid opens with the installed Nail.exe"
  }
  Check ((Hklm-Value 'Software\Clients\Mail\Nail' 'DLLPath') -ieq (Join-Path $app "mapi\$arch\NailMapi.dll")) 'Simple MAPI client (64-bit view)'
  Check ((Hklm-Value 'Software\Clients\Mail\Nail' 'DLLPath' 'Registry32') -ieq (Join-Path $app 'mapi\x86\NailMapi.dll')) 'Simple MAPI client (32-bit view, x86 DLL)'
  Check (Machine-Path-Has $bin) 'bin\ is on the machine PATH (ADDTOPATH=1)'
  Check (-not (Hklm-Value 'Software\Microsoft\Windows\CurrentVersion\Run' 'Nail')) 'no Run entry (AUTOSTART not set)'

  # --- launch --------------------------------------------------------------
  Write-Host 'launch'
  # Data and settings in the temporary directory, but no `portable` marker:
  # the app must recognise itself as the MSI installation (InstallDir).
  $stderr = Join-Path $work 'nail-app.stderr.log'
  $stdout = Join-Path $work 'nail-app.stdout.log'
  $env:NAIL_LOG = 'info'
  $proc = Start-Process -FilePath (Join-Path $app 'Nail.exe') `
    -ArgumentList @('--data-dir', "`"$app\data`"", '--config-dir', "`"$app\config`"", '--', 'mailto:smoke@example.org') `
    -RedirectStandardError $stderr -RedirectStandardOutput $stdout -PassThru
  $endpoint = Join-Path $app 'data\mcp-endpoint.json'
  $deadline = (Get-Date).AddSeconds($StartTimeoutSeconds)
  while (-not (Test-Path $endpoint) -and -not $proc.HasExited -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
  Check (Test-Path $endpoint) 'the core started (the data dir has mcp-endpoint.json)'
  Start-Sleep -Seconds 3
  Check (-not $proc.HasExited) 'Nail.exe is still running'
  # DISABLEUPDATES=1 (the default): the shell's installation facts say so,
  # and the UI then shows "Your organization installs updates" without
  # any update button (desktop smoke "an MSI install hides the updater").
  $log = ((Get-Content $stdout, $stderr -Raw -ErrorAction SilentlyContinue) -join "`n") -replace '\x1b\[[0-9;]*m', ''
  Check ($log -match 'installation kind="msi"') 'the app sees the MSI installation (kind msi)'
  Check ($log -match 'updates_disabled=Some\(MsiManaged\)') 'in-app updates disabled (msi-managed)'
  Stop-Process -Id $proc.Id -Force
  $proc.WaitForExit(10000) | Out-Null
  $proc = $null

  # --- upgrade -------------------------------------------------------------
  if ($UpgradeMsi) {
    Write-Host "upgrade ($UpgradeMsi)"
    $UpgradeMsi = (Resolve-Path $UpgradeMsi).Path
    $code = Msiexec @('/i', "`"$UpgradeMsi`"", 'AUTOSTART=1') (Join-Path $work 'upgrade.log')
    Check ($code -eq 0 -or $code -eq 3010) "msiexec /i (upgrade) exit code 0 (was $code)"
    $installed = $UpgradeMsi
    $newVersion = Hklm-Value $facts 'Version'
    Check ($newVersion -ne $firstVersion) "facts: Version = $newVersion (was $firstVersion)"
    Check ((Hklm-Value $facts 'InstallDir').TrimEnd('\') -ieq $app) 'same installation directory'
    Check ((Hklm-Value 'Software\Microsoft\Windows\CurrentVersion\Run' 'Nail') -like "*$app\Nail.exe`" --autostart") 'Run entry (AUTOSTART=1 on the command line wins)'
    Check ((Hklm-Value $facts 'DisableUpdates') -eq 1) 'DisableUpdates kept (remembered)'
    Check ((Hklm-Value $facts 'Mapi') -eq 1) 'Mapi kept (remembered)'
    Check ((Hklm-Value 'Software\Clients\Mail\Nail' 'DLLPath') -ieq (Join-Path $app "mapi\$arch\NailMapi.dll")) 'Simple MAPI client still registered'
    Check (Machine-Path-Has $bin) 'bin\ still on the machine PATH (remembered)'
    Check (Test-Path (Join-Path $app 'data\nail.sqlite3')) 'data kept across the upgrade'
  }

  # --- uninstall -----------------------------------------------------------
  Write-Host 'uninstall'
  $code = Msiexec @('/x', "`"$installed`"") (Join-Path $work 'uninstall.log')
  Check ($code -eq 0 -or $code -eq 3010) "msiexec /x exit code 0 (was $code)"
  $installed = $null
  Check (-not (Test-Path (Join-Path $app 'Nail.exe'))) 'Nail.exe removed'
  Check (-not (Test-Path (Join-Path $app 'mapi'))) 'mapi\ removed'
  Check (-not (Hklm-Exists $facts)) 'installation facts removed'
  Check (-not (Hklm-Value 'Software\RegisteredApplications' 'Nail')) 'RegisteredApplications entry removed'
  Check (-not (Hklm-Exists 'Software\Nail\Capabilities')) 'Capabilities removed'
  Check (-not (Hklm-Exists 'Software\Classes\Nail.Url.mailto')) 'mailto ProgID removed'
  Check (-not (Hklm-Exists 'Software\Clients\Mail\Nail')) 'Simple MAPI client removed (64-bit view)'
  Check (-not (Hklm-Exists 'Software\Clients\Mail\Nail' 'Registry32')) 'Simple MAPI client removed (32-bit view)'
  Check (-not (Machine-Path-Has $bin)) 'bin\ removed from the machine PATH'
  Check (-not (Hklm-Value 'Software\Microsoft\Windows\CurrentVersion\Run' 'Nail')) 'Run entry removed'
  Check (Test-Path (Join-Path $app 'data\nail.sqlite3')) 'data kept'
  Write-Host 'msi smoke: passed'
}
finally {
  if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
  if ($installed) {
    Msiexec @('/x', "`"$installed`"") (Join-Path $work 'cleanup.log') | Out-Null
  }
  Get-ChildItem $work -Filter '*.log' -ErrorAction SilentlyContinue | ForEach-Object {
    if ($env:NAIL_MSI_SMOKE_LOGS) { Copy-Item $_.FullName -Destination $env:NAIL_MSI_SMOKE_LOGS -Force }
  }
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
