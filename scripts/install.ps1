<#
.SYNOPSIS
  Build Glint, install it to %LOCALAPPDATA%\Programs\Glint, put `glint` on PATH and
  register it for image files (per-user, no admin needed).

.PARAMETER NoBuild       Use the existing target\release binaries.
.PARAMETER NoSettings    Don't run set-default.ps1 (the default-app policy, one UAC prompt).
.PARAMETER NoBackground  Don't start Glint at login / keep it in the tray.
#>
param([switch]$NoBuild, [switch]$NoSettings, [switch]$NoBackground)
$ErrorActionPreference = 'Stop'

$repo = Split-Path $PSScriptRoot -Parent
$dest = Join-Path $env:LOCALAPPDATA 'Programs\Glint'
$gui = Join-Path $dest 'glintw.exe'
$cli = Join-Path $dest 'glint.exe'

# A release package keeps the binaries next to the scripts folder; a source checkout builds them into target\release.
$packaged = Test-Path (Join-Path $repo 'glintw.exe')
$bin = if ($packaged) { $repo } else { Join-Path $repo 'target\release' }

# Every extension Glint opens. Keep in sync with decode::EXTENSIONS.
$exts = @('png','apng','jpg','jpeg','jpe','jfif','gif','webp','bmp','dib','ico','cur','tga','dds','qoi',
          'tif','tiff','exr','hdr','pbm','pgm','ppm','pnm','pam','ff','svg','svgz','ase','aseprite',
          'avif','heic','heif','jxl','psd','psb','xcf','jp2','j2k','pcx','dng','cr2','cr3','nef','arw','orf','rw2','raf')

if (-not $NoBuild -and -not $packaged) {
    Write-Host 'Building release...' -ForegroundColor Cyan
    Push-Location $repo
    try { cargo build --release; if ($LASTEXITCODE) { throw 'cargo build failed' } } finally { Pop-Location }
}

# Close a running viewer so its exe can be replaced.
if (Test-Path $cli) { & $cli quit 2>$null | Out-Null }
$deadline = (Get-Date).AddSeconds(5)
while ((Get-Process glintw -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
Get-Process glintw -ErrorAction SilentlyContinue | Stop-Process -Force

New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item (Join-Path $bin 'glint.exe') $cli -Force
Copy-Item (Join-Path $bin 'glintw.exe') $gui -Force
Write-Host "Installed to $dest" -ForegroundColor Green

# Keep the uninstall scripts with the app, and clear the downloaded-file mark so Windows doesn't warn at every launch.
$scriptsDest = Join-Path $dest 'scripts'
New-Item -ItemType Directory -Force $scriptsDest | Out-Null
Copy-Item (Join-Path $PSScriptRoot 'uninstall.ps1'), (Join-Path $PSScriptRoot 'set-default.ps1') $scriptsDest -Force
Get-ChildItem $dest -Recurse -File | Unblock-File

# PATH (user scope).
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $dest) {
    [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ";$dest"), 'User')
    Write-Host "Added $dest to your PATH (open a new terminal to use 'glint')."
}

function Set-Key([string]$path, [hashtable]$values) {
    $full = "HKCU:\$path"
    if (-not (Test-Path $full)) { New-Item -Path $full -Force | Out-Null }
    foreach ($k in $values.Keys) {
        $name = if ($k -eq '(default)') { '(default)' } else { $k }
        Set-ItemProperty -Path $full -Name $name -Value $values[$k]
    }
}

$open = "`"$gui`" `"%1`""
$icon = "`"$gui`",0"

# ProgID used by every image type.
Set-Key 'Software\Classes\Glint.Image' @{ '(default)' = 'Image (Glint)'; 'FriendlyTypeName' = 'Image' }
Set-Key 'Software\Classes\Glint.Image\DefaultIcon' @{ '(default)' = $icon }
Set-Key 'Software\Classes\Glint.Image\shell\open' @{ 'FriendlyAppName' = 'Glint' }
Set-Key 'Software\Classes\Glint.Image\shell\open\command' @{ '(default)' = $open }

# Application registration (shows up in "Open with" and Default Apps).
Set-Key 'Software\Classes\Applications\glintw.exe' @{ 'FriendlyAppName' = 'Glint' }
Set-Key 'Software\Classes\Applications\glintw.exe\DefaultIcon' @{ '(default)' = $icon }
Set-Key 'Software\Classes\Applications\glintw.exe\shell\open\command' @{ '(default)' = $open }
Set-Key 'Software\Glint\Capabilities' @{
    'ApplicationName' = 'Glint'
    'ApplicationDescription' = 'Fast, elegant image viewer for sprites and screenshots'
    'ApplicationIcon' = $icon
}
Set-Key 'Software\RegisteredApplications' @{ 'Glint' = 'Software\Glint\Capabilities' }

foreach ($e in $exts) {
    Set-Key "Software\Classes\.$e\OpenWithProgids" @{ 'Glint.Image' = '' }
    Set-Key 'Software\Glint\Capabilities\FileAssociations' @{ ".$e" = 'Glint.Image' }
    Set-Key 'Software\Classes\Applications\glintw.exe\SupportedTypes' @{ ".$e" = '' }
    # Explorer context menu: select two images -> "Compare with Glint".
    Set-Key "Software\Classes\SystemFileAssociations\.$e\shell\Glint.Compare" @{
        '(default)' = 'Compare with Glint'; 'Icon' = $icon; 'MultiSelectModel' = 'Document'
    }
    Set-Key "Software\Classes\SystemFileAssociations\.$e\shell\Glint.Compare\command" @{
        '(default)' = "`"$gui`" --compare-add `"%1`""
    }
}

# Tell Explorer that associations changed.
Add-Type -Namespace Glint -Name Shell -MemberDefinition @'
[DllImport("shell32.dll")] public static extern void SHChangeNotify(int eventId, uint flags, System.IntPtr a, System.IntPtr b);
'@ -ErrorAction SilentlyContinue
[Glint.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

Write-Host "Registered Glint for $($exts.Count) image types." -ForegroundColor Green

# Background mode: start hidden in the tray at login so opening images is instant.
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
if ($NoBackground) {
    Remove-ItemProperty $run -Name 'Glint' -ErrorAction SilentlyContinue
} else {
    Set-ItemProperty $run -Name 'Glint' -Value "`"$gui`" --background"
    Start-Process $gui -ArgumentList '--background'
    Write-Host 'Glint is running in the tray and will start at login.'
}

$policy = (Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System' -ErrorAction SilentlyContinue).DefaultAssociationsConfiguration
if (-not $NoSettings -and -not $policy) {
    & (Join-Path $PSScriptRoot 'set-default.ps1')
}
