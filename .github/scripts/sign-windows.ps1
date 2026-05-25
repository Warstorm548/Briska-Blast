<#
.SYNOPSIS
  Authenticode-sign a Windows artifact IF code-signing secrets are present.

.DESCRIPTION
  Conditional signing for the launcher release workflow (P4 — SmartScreen / AV
  publisher trust). Reads the cert material from environment variables:

    WINDOWS_CERT_PFX_BASE64 — base64 of an OV/EV code-signing .pfx
    WINDOWS_CERT_PASSWORD   — the .pfx password

  If EITHER is missing/empty the script prints a warning and exits 0, so the
  release workflow still succeeds and publishes UNSIGNED artifacts (no cert ==
  unsigned but green). When both are present it signs with SHA-256 + an RFC3161
  timestamp and then verifies the signature, failing the job on any error.

.PARAMETER Target
  Path to the .exe to sign (launcher binary or the NSIS Setup.exe).
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$Target
)

$ErrorActionPreference = "Stop"

$pfxB64 = $env:WINDOWS_CERT_PFX_BASE64
$pfxPass = $env:WINDOWS_CERT_PASSWORD

if ([string]::IsNullOrEmpty($pfxB64) -or [string]::IsNullOrEmpty($pfxPass)) {
    Write-Warning "Code-signing secrets not set — skipping Authenticode signing of '$Target'. Artifact will be published UNSIGNED."
    exit 0
}

if (-not (Test-Path $Target)) {
    throw "sign target not found: $Target"
}

# Decode the PFX to a temp file outside the workspace so it never lands in an
# uploaded artifact.
$pfxPath = Join-Path $env:RUNNER_TEMP "codesign.pfx"
[IO.File]::WriteAllBytes($pfxPath, [Convert]::FromBase64String($pfxB64))

try {
    # signtool ships in the Windows SDK and is NOT on PATH by default on the
    # GitHub runner. Resolve the newest SDK x64 copy, falling back to PATH.
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" `
        -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
    if (-not $signtool) { $signtool = "signtool" }

    # /fd sha256 : file digest algorithm
    # /tr + /td  : RFC3161 timestamp so signatures stay valid after cert expiry
    & $signtool sign /fd sha256 /tr "http://timestamp.digicert.com" /td sha256 `
        /f $pfxPath /p $pfxPass $Target
    if ($LASTEXITCODE -ne 0) { throw "signtool sign failed for $Target (exit $LASTEXITCODE)" }

    # /pa : use the Authenticode policy — the acceptance check for P4.
    & $signtool verify /pa $Target
    if ($LASTEXITCODE -ne 0) { throw "signtool verify failed for $Target (exit $LASTEXITCODE)" }

    Write-Host "Signed and verified: $Target"
}
finally {
    Remove-Item $pfxPath -Force -ErrorAction SilentlyContinue
}
