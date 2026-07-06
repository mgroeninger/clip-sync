# Bump the workspace semver in root Cargo.toml ([workspace.package].version).
# All crates inherit via version.workspace = true.
#
# Usage (from repo root):
#   .\scripts\bump-version.ps1                    # print current version
#   .\scripts\bump-version.ps1 -Bump patch        # 0.1.0 -> 0.1.1
#   .\scripts\bump-version.ps1 -Bump minor        # 0.1.0 -> 0.2.0
#   .\scripts\bump-version.ps1 -Bump major        # 0.1.0 -> 1.0.0
#   .\scripts\bump-version.ps1 -Version 0.2.0     # set explicitly
#   .\scripts\bump-version.ps1 -Bump patch -DryRun

[CmdletBinding(DefaultParameterSetName = 'Show')]
param(
    [Parameter(ParameterSetName = 'Bump')]
    [ValidateSet('patch', 'minor', 'major')]
    [string]$Bump,

    [Parameter(ParameterSetName = 'Set')]
    [string]$Version,

    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path $PSScriptRoot -Parent
$CargoToml = Join-Path $RepoRoot 'Cargo.toml'

function Get-WorkspaceVersion {
    param([string]$Path)

    $text = Get-Content -Raw -Path $Path
    $match = [regex]::Match(
        $text,
        '(?ms)\[workspace\.package\].*?^version\s*=\s*"([^"]+)"'
    )
    if (-not $match.Success) {
        throw "Could not find [workspace.package] version in $Path"
    }
    return $match.Groups[1].Value
}

function Test-SemverTriple {
    param([string]$Value)

    return $Value -match '^\d+\.\d+\.\d+$'
}

function Get-BumpedVersion {
    param(
        [string]$Current,
        [string]$Kind
    )

    if (-not (Test-SemverTriple $Current)) {
        throw "Current workspace version is not MAJOR.MINOR.PATCH: $Current"
    }

    $parts = $Current.Split('.')
    $major = [int]$parts[0]
    $minor = [int]$parts[1]
    $patch = [int]$parts[2]

    switch ($Kind) {
        'patch' { return "$major.$minor.$($patch + 1)" }
        'minor' { return "$major.$($minor + 1).0" }
        'major' { return "$($major + 1).0.0" }
        default { throw "Unknown bump kind: $Kind" }
    }
}

function Set-WorkspaceVersion {
    param(
        [string]$Path,
        [string]$NewVersion
    )

    $text = Get-Content -Raw -Path $Path
    $updated = [regex]::Replace(
        $text,
        '(?ms)(\[workspace\.package\].*?^version\s*=\s*")[^"]+(")',
        "`${1}$NewVersion`${2}",
        1
    )
    if ($updated -eq $text) {
        throw "Failed to update version in $Path"
    }
    Set-Content -Path $Path -Value $updated -NoNewline
}

function Get-ResolvedCrateVersions {
    param([string]$Root)

    $json = cargo metadata --format-version 1 --no-deps 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed: $json"
    }
    $meta = $json | ConvertFrom-Json
    return $meta.packages |
        Where-Object { $_.manifest_path -like "*\crates\*" } |
        Sort-Object name |
        ForEach-Object { [PSCustomObject]@{ Name = $_.name; Version = $_.version } }
}

$current = Get-WorkspaceVersion -Path $CargoToml

if ($PSCmdlet.ParameterSetName -eq 'Show') {
    Write-Host "Workspace version: $current" -ForegroundColor Cyan
    Write-Host ''
    Write-Host 'Bump with -Bump patch|minor|major or set with -Version MAJOR.MINOR.PATCH'
    exit 0
}

if ($PSCmdlet.ParameterSetName -eq 'Set') {
    if (-not (Test-SemverTriple $Version)) {
        throw "Invalid -Version '$Version' (expected MAJOR.MINOR.PATCH)"
    }
    $next = $Version
}
else {
    $next = Get-BumpedVersion -Current $current -Kind $Bump
}

if ($next -eq $current) {
    Write-Host "Version unchanged: $current" -ForegroundColor Yellow
    exit 0
}

Write-Host "Workspace version: $current -> $next" -ForegroundColor Cyan

if ($DryRun) {
    Write-Host 'Dry run - Cargo.toml not modified' -ForegroundColor DarkGray
    exit 0
}

Set-WorkspaceVersion -Path $CargoToml -NewVersion $next

Write-Host ''
Write-Host 'Resolved crate versions:' -ForegroundColor DarkGray
Push-Location $RepoRoot
try {
    Get-ResolvedCrateVersions -Root $RepoRoot | ForEach-Object {
        Write-Host ("  {0,-28} {1}" -f $_.Name, $_.Version)
    }
}
finally {
    Pop-Location
}

Write-Host ''
Write-Host "Updated $CargoToml" -ForegroundColor Green
Write-Host "Suggested commit: release: v$next"
