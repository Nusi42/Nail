<#
.SYNOPSIS
  Checks the Authenticode signatures of a build (plan M4b WP112): every
  .exe and .msi in -Folder, plus the files inside the portable zip, must be
  signed and timestamped. CI runs it only when a certificate is configured
  (NAIL_SIGN_THUMBPRINT); unsigned development builds skip it.
#>
param([Parameter(Mandatory)][string]$Folder)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$signtool = if ($env:NAIL_SIGNTOOL) { $env:NAIL_SIGNTOOL } else {
  Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\signtool.exe" |
    Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (-not $signtool) { throw 'signtool.exe not found; set NAIL_SIGNTOOL' }

$files = @(Get-ChildItem $Folder -File | Where-Object { $_.Extension -in '.exe', '.msi' })
$zip = Get-ChildItem $Folder -Filter '*-portable.zip' | Select-Object -First 1
$unzipped = $null
if ($zip) {
  $unzipped = Join-Path ([IO.Path]::GetTempPath()) ("nail-verify-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
  Expand-Archive -Path $zip.FullName -DestinationPath $unzipped
  $files += @(Get-ChildItem $unzipped -Recurse -File | Where-Object { $_.Extension -in '.exe', '.dll' })
}
try {
  if (-not $files) { throw "nothing to verify in $Folder" }
  foreach ($file in $files) {
    & $signtool verify /pa /all /tw $file.FullName | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "not signed (or not timestamped): $($file.FullName)" }
  }
  Write-Host "Authenticode: $($files.Count) files signed and timestamped"
}
finally {
  if ($unzipped) { Remove-Item -Recurse -Force $unzipped -ErrorAction SilentlyContinue }
}
