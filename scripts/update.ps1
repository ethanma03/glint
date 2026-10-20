<#
.SYNOPSIS
  Pull the latest source, rebuild, reinstall, and reopen whatever image was showing.
  File associations survive because the install path never changes.
#>
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$cli = Join-Path $env:LOCALAPPDATA 'Programs\Glint\glint.exe'

Push-Location $repo
try {
    if (git remote) {
        Write-Host 'Pulling...' -ForegroundColor Cyan
        git pull --ff-only
        if ($LASTEXITCODE) { throw 'git pull failed' }
    }
    $last = $null
    if (Test-Path $cli) {
        try { $last = (& $cli state 2>$null | ConvertFrom-Json).file } catch { }
    }
    & (Join-Path $PSScriptRoot 'install.ps1') -NoSettings
    if ($last -and (Test-Path $last)) { & $cli $last }
    Write-Host "Glint updated: $(& $cli --version)" -ForegroundColor Green
} finally {
    Pop-Location
}
