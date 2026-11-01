<#
.SYNOPSIS
  Build Glint and zip a release package into dist\Glint-win-x64.zip.
  Users unzip it and run Install.cmd. The package keeps the same scripts as the repo,
  and install.ps1 notices the binaries next to it instead of in target\release.

.PARAMETER NoBuild  Package the existing target\release binaries.
#>
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'

$repo = Split-Path $PSScriptRoot -Parent
$stage = Join-Path $repo 'dist\Glint'
$zip = Join-Path $repo 'dist\Glint-win-x64.zip'

if (-not $NoBuild) {
    Push-Location $repo
    try { cargo build --release; if ($LASTEXITCODE) { throw 'cargo build failed' } } finally { Pop-Location }
}

if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $stage 'scripts') | Out-Null
Copy-Item (Join-Path $repo 'target\release\glint.exe'), (Join-Path $repo 'target\release\glintw.exe') $stage
foreach ($s in 'install.ps1', 'set-default.ps1', 'uninstall.ps1') { Copy-Item (Join-Path $PSScriptRoot $s) (Join-Path $stage 'scripts') }
Copy-Item (Join-Path $repo 'packaging\Install.cmd'), (Join-Path $repo 'packaging\Uninstall.cmd'), (Join-Path $repo 'packaging\README.txt') $stage
Copy-Item (Join-Path $repo 'LICENSE') (Join-Path $stage 'LICENSE.txt')

if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip
$size = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host "Packaged $zip ($size MB)" -ForegroundColor Green
