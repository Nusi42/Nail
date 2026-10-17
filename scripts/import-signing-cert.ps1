<#
.SYNOPSIS
  Release helper (ADRs 0089, 0201): imports the Authenticode certificate
  from a .pfx given in the environment, if there is one, and prints the
  thumbprint to set as NAIL_SIGN_THUMBPRINT for the installer build.

.DESCRIPTION
  NAIL_SIGN_PFX_BASE64    base64 of a .pfx with the code-signing certificate
  NAIL_SIGN_PFX_PASSWORD  its password

  When NAIL_SIGN_PFX_BASE64 is empty nothing happens and the build stays
  unsigned. With it, the certificate goes into Cert:\CurrentUser\My, the
  temporary .pfx is deleted, and the script prints the thumbprint, which
  scripts/build-installer.mjs (scripts/lib/sign.mjs) reads from
  NAIL_SIGN_THUMBPRINT. An EV certificate on a hardware token or a cloud HSM
  is not a .pfx: set NAIL_SIGN_THUMBPRINT (and NAIL_SIGNTOOL if needed)
  directly on a machine that sees the token instead.
#>
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $env:NAIL_SIGN_PFX_BASE64) {
  Write-Host 'Authenticode: no certificate configured (NAIL_SIGN_PFX_BASE64) - building unsigned'
  exit 0
}
$pfx = Join-Path ([IO.Path]::GetTempPath()) ("nail-sign-" + [Guid]::NewGuid().ToString('N') + '.pfx')
try {
  [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:NAIL_SIGN_PFX_BASE64))
  $password = ConvertTo-SecureString -String ($env:NAIL_SIGN_PFX_PASSWORD ?? '') -AsPlainText -Force
  $cert = Import-PfxCertificate -FilePath $pfx -CertStoreLocation Cert:\CurrentUser\My -Password $password
  $codeSigning = $cert | Where-Object { $_.EnhancedKeyUsageList.ObjectId -contains '1.3.6.1.5.5.7.3.3' } | Select-Object -First 1
  if (-not $codeSigning) { throw 'the .pfx holds no code-signing certificate' }
  Write-Host "Authenticode: imported $($codeSigning.Subject) ($($codeSigning.Thumbprint)), valid until $($codeSigning.NotAfter)"
  Write-Host "Set NAIL_SIGN_THUMBPRINT=$($codeSigning.Thumbprint) for scripts/build-installer.mjs"
}
finally {
  Remove-Item -Force $pfx -ErrorAction SilentlyContinue
}
