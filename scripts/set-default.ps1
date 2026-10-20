<#
.SYNOPSIS
  Make Glint the default app for every image type it supports, using Windows'
  supported "Default associations configuration" policy (one UAC prompt).

.DESCRIPTION
  Windows 11 protects per-user defaults (UserChoice hash + UCPD driver), so they
  can't be written directly. Instead this writes an associations XML and points
  the machine policy at it; Windows applies it at the next sign-in, and re-applies
  it at every sign-in while the policy is set. Run with -Remove to undo.

.PARAMETER Remove  Delete the policy (defaults stay as they are until you change them).
#>
param([switch]$Remove)
$ErrorActionPreference = 'Stop'

$policyKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System'
$xmlPath = Join-Path $env:ProgramData 'Glint\default-associations.xml'

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    # Re-launch elevated; the UAC prompt is the one click this needs.
    $argList = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"")
    if ($Remove) { $argList += '-Remove' }
    $p = Start-Process pwsh -Verb RunAs -ArgumentList $argList -Wait -PassThru
    exit $p.ExitCode
}

if ($Remove) {
    Remove-ItemProperty $policyKey -Name 'DefaultAssociationsConfiguration' -ErrorAction SilentlyContinue
    Remove-Item $xmlPath -ErrorAction SilentlyContinue
    Write-Host 'Removed the Glint default-associations policy.'
    exit 0
}

$exts = @('png','apng','jpg','jpeg','jpe','jfif','gif','webp','bmp','dib','ico','cur','tga','dds','qoi',
          'tif','tiff','exr','hdr','pbm','pgm','ppm','pnm','pam','ff','svg','svgz','ase','aseprite',
          'avif','heic','heif','jxl','psd','psb','xcf','jp2','j2k','pcx','dng','cr2','cr3','nef','arw','orf','rw2','raf')
$lines = $exts | ForEach-Object { "  <Association Identifier=`".$_`" ProgId=`"Glint.Image`" ApplicationName=`"Glint`" />" }
$xml = "<?xml version=`"1.0`" encoding=`"UTF-8`"?>`n<DefaultAssociations>`n$($lines -join "`n")`n</DefaultAssociations>`n"

New-Item -ItemType Directory -Force (Split-Path $xmlPath) | Out-Null
Set-Content -Path $xmlPath -Value $xml -Encoding UTF8
if (-not (Test-Path $policyKey)) { New-Item -Path $policyKey -Force | Out-Null }
Set-ItemProperty $policyKey -Name 'DefaultAssociationsConfiguration' -Value $xmlPath
Write-Host "Glint will be the default for $($exts.Count) image types after you sign out and back in." -ForegroundColor Green
