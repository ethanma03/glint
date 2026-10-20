<#
.SYNOPSIS  Remove Glint's install folder, PATH entry and per-user registrations.
#>
$ErrorActionPreference = 'Continue'
$dest = Join-Path $env:LOCALAPPDATA 'Programs\Glint'

if (Test-Path "$dest\glint.exe") { & "$dest\glint.exe" quit 2>$null | Out-Null }
Start-Sleep -Milliseconds 500
Get-Process glintw -ErrorAction SilentlyContinue | Stop-Process -Force

foreach ($k in 'Software\Classes\Glint.Image', 'Software\Classes\Applications\glintw.exe', 'Software\Glint') {
    Remove-Item "HKCU:\$k" -Recurse -Force -ErrorAction SilentlyContinue
}
Remove-ItemProperty 'HKCU:\Software\RegisteredApplications' -Name 'Glint' -ErrorAction SilentlyContinue
Get-ChildItem 'HKCU:\Software\Classes' -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -like '.*' } | ForEach-Object {
    Remove-ItemProperty "$($_.PSPath)\OpenWithProgids" -Name 'Glint.Image' -ErrorAction SilentlyContinue
}
Get-ChildItem 'HKCU:\Software\Classes\SystemFileAssociations' -ErrorAction SilentlyContinue | ForEach-Object {
    Remove-Item "$($_.PSPath)\shell\Glint.Compare" -Recurse -Force -ErrorAction SilentlyContinue
}

Remove-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Glint' -ErrorAction SilentlyContinue
$policy = (Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System' -ErrorAction SilentlyContinue).DefaultAssociationsConfiguration
if ($policy -like '*Glint*') { & (Join-Path $PSScriptRoot 'set-default.ps1') -Remove }

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
[Environment]::SetEnvironmentVariable('Path', (($userPath -split ';') | Where-Object { $_ -and $_ -ne $dest }) -join ';', 'User')
Remove-Item $dest -Recurse -Force -ErrorAction SilentlyContinue
Write-Host 'Glint uninstalled. (Settings and undo backups remain in %APPDATA%\glint and %LOCALAPPDATA%\Glint.)'
