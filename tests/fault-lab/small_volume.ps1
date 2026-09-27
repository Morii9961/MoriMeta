# SPDX-License-Identifier: GPL-3.0-or-later
# Small NTFS test volume for the real disk-full test (docs/PHASE1B_FAULT_MATRIX.md, G-1).
#
# Needs an elevated PowerShell (diskpart). Creates an expandable 64 MB VHDX under the ignored
# research/.work/fault-lab/, attaches it, formats it NTFS with the label MMFAULT and assigns the
# next free drive letter. Nothing else on the system is changed; -Remove detaches the disk and
# deletes the VHDX.
#
#   .\tests\fault-lab\small_volume.ps1            # create + attach; prints MM_E2E_SMALL_VOLUME
#   $env:MM_E2E_SMALL_VOLUME = 'X:\'              # the printed value, then (non-elevated is fine):
#   cargo test -p mm-cli --test e2e real_disk_full_on_small_volume -- --nocapture
#   .\tests\fault-lab\small_volume.ps1 -Remove    # detach and delete

param([switch]$Remove, [int]$SizeMB = 64)
$ErrorActionPreference = 'Stop'

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'diskpart needs an elevated PowerShell'
}

$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$dir = Join-Path $repo 'research\.work\fault-lab'
$vhd = Join-Path $dir 'small.vhdx'
$script = Join-Path $dir 'diskpart.txt'
New-Item -ItemType Directory -Force $dir | Out-Null

if ($Remove) {
    if (-not (Test-Path $vhd)) { Write-Output 'no test volume'; return }
    "select vdisk file=`"$vhd`"`r`ndetach vdisk" | Set-Content -Encoding ascii $script
    diskpart /s $script | Out-Null
    Remove-Item $vhd, $script
    Write-Output 'detached and deleted'
    return
}

if (Test-Path $vhd) { throw "$vhd exists; run with -Remove first" }
@(
    "create vdisk file=`"$vhd`" maximum=$SizeMB type=expandable"
    "select vdisk file=`"$vhd`""
    'attach vdisk'
    'convert mbr'
    'create partition primary'
    'format fs=ntfs quick label=MMFAULT'
    'assign'
) -join "`r`n" | Set-Content -Encoding ascii $script
diskpart /s $script | Out-Null
Remove-Item $script

$volume = Get-Volume -FileSystemLabel MMFAULT | Select-Object -First 1
if (-not $volume.DriveLetter) { throw 'volume attached but has no drive letter' }
Write-Output ("MM_E2E_SMALL_VOLUME={0}:\  ({1} bytes)" -f $volume.DriveLetter, $volume.Size)
