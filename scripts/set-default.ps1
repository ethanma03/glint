<#
.SYNOPSIS
  Make Glint the default app for every image type it supports, using Windows'
  supported "Default associations configuration" policy (one UAC prompt).

.DESCRIPTION
  Windows 11 protects per-user defaults (UserChoice hash + UCPD driver), so they
  can't be written directly. Instead this writes an associations XML and points
  the machine policy at it; Windows applies it at the next sign-in, and re-applies
  it at every sign-in while the policy is set.

  There is only one such policy per machine. If another file is already configured
  (e.g. Flick's videos or Leaf's documents), its entries are merged into Glint's file,
  so every app stays the default for its types.

.PARAMETER Remove  Drop Glint's entries (other apps' merged entries stay in effect).
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
    $p = Start-Process (Get-Process -Id $PID).Path -Verb RunAs -ArgumentList $argList -Wait -PassThru
    exit $p.ExitCode
}

$exts = @('png','apng','jpg','jpeg','jpe','jfif','gif','webp','bmp','dib','ico','cur','tga','dds','qoi',
          'tif','tiff','exr','hdr','pbm','pgm','ppm','pnm','pam','ff','svg','svgz','ase','aseprite',
          'avif','heic','heif','jxl','psd','psb','xcf','jp2','j2k','pcx','dng','cr2','cr3','nef','arw','orf','rw2','raf')

# Entries from whatever file the policy points at now (another app's, or ours from last time).
$current = (Get-ItemProperty $policyKey -ErrorAction SilentlyContinue).DefaultAssociationsConfiguration
$others = @()
if ($current -and (Test-Path $current)) {
    try {
        $doc = [xml](Get-Content -Raw $current)
        $others = @($doc.DefaultAssociations.Association | Where-Object { $_ -and $_.ProgId -notlike 'Glint.*' })
    } catch { Write-Warning "Couldn't read the existing associations file $current; replacing it." }
}
$mine = $exts | ForEach-Object { @{ Identifier = ".$_"; ProgId = 'Glint.Image'; ApplicationName = 'Glint' } }
$keep = $others | Where-Object { $Remove -or ($exts -notcontains $_.Identifier.TrimStart('.')) } |
    ForEach-Object { @{ Identifier = $_.Identifier; ProgId = $_.ProgId; ApplicationName = $_.ApplicationName } }
$all = @($keep) + $(if ($Remove) { @() } else { @($mine) })

if ($all.Count -eq 0) {
    Remove-ItemProperty $policyKey -Name 'DefaultAssociationsConfiguration' -ErrorAction SilentlyContinue
    Remove-Item $xmlPath -ErrorAction SilentlyContinue
    Write-Host 'Removed the default-associations policy.'
    exit 0
}

$esc = { param($s) [Security.SecurityElement]::Escape([string]$s) }
$lines = $all | ForEach-Object {
    "  <Association Identifier=`"$(& $esc $_.Identifier)`" ProgId=`"$(& $esc $_.ProgId)`" ApplicationName=`"$(& $esc $_.ApplicationName)`" />"
}
$xml = "<?xml version=`"1.0`" encoding=`"UTF-8`"?>`n<DefaultAssociations>`n$($lines -join "`n")`n</DefaultAssociations>`n"
New-Item -ItemType Directory -Force (Split-Path $xmlPath) | Out-Null
Set-Content -Path $xmlPath -Value $xml -Encoding UTF8
if (-not (Test-Path $policyKey)) { New-Item -Path $policyKey -Force | Out-Null }
Set-ItemProperty $policyKey -Name 'DefaultAssociationsConfiguration' -Value $xmlPath

if ($Remove) {
    Write-Host "Removed Glint's defaults; kept $($keep.Count) entries from other apps." -ForegroundColor Green
} else {
    $extra = if ($keep.Count) { " (kept $($keep.Count) entries from $current)" } else { '' }
    Write-Host "Glint will be the default for $($exts.Count) image types after you sign out and back in$extra." -ForegroundColor Green
}
