[CmdletBinding()]
param(
    [string] $GodotPath = $env:DUNGEON_BARRAGE_GODOT,
    [string] $ExportTemplatesPath = $env:DUNGEON_BARRAGE_GODOT_TEMPLATES
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-CommandVersion {
    param(
        [Parameter(Mandatory)]
        [string] $Command,

        [Parameter(Mandatory)]
        [string[]] $Arguments,

        [Parameter(Mandatory)]
        [string] $ExpectedPattern,

        [Parameter(Mandatory)]
        [string] $Label
    )

    if ($null -eq (Get-Command -Name $Command -ErrorAction SilentlyContinue)) {
        throw "$Label is not installed or is not on PATH: $Command"
    }

    $output = (& $Command @Arguments 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "$Label version command failed with exit code $LASTEXITCODE`: $output"
    }
    if ($output -notmatch $ExpectedPattern) {
        throw "$Label version mismatch. Expected /$ExpectedPattern/ but received: $output"
    }

    Write-Output "$Label`t$output"
}

Assert-CommandVersion `
    -Command 'dotnet' `
    -Arguments @('--version') `
    -ExpectedPattern '^10\.0\.302$' `
    -Label '.NET SDK'

Assert-CommandVersion `
    -Command 'rustc' `
    -Arguments @('--version') `
    -ExpectedPattern '^rustc 1\.94\.0\b' `
    -Label 'Rust compiler'

Assert-CommandVersion `
    -Command 'cargo' `
    -Arguments @('--version') `
    -ExpectedPattern '^cargo 1\.94\.0\b' `
    -Label 'Cargo'

$godotCommand = $GodotPath
if ([string]::IsNullOrWhiteSpace($godotCommand)) {
    $resolvedGodot = Get-Command -Name 'godot' -ErrorAction SilentlyContinue
    if ($null -eq $resolvedGodot) {
        throw 'Godot 4.7.1 .NET is missing. Install the .NET editor and set DUNGEON_BARRAGE_GODOT to its executable path.'
    }
    $godotCommand = $resolvedGodot.Source
} elseif (-not (Test-Path -LiteralPath $godotCommand -PathType Leaf)) {
    throw "DUNGEON_BARRAGE_GODOT does not name a file: $godotCommand"
}

$godotVersion = (& $godotCommand --version 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "Godot version command failed with exit code $LASTEXITCODE`: $godotVersion"
}
if ($godotVersion -notmatch '^4\.7\.1\b' -or $godotVersion -notmatch '(?i)(mono|\.net)') {
    throw "Godot version mismatch. Expected the 4.7.1 .NET editor but received: $godotVersion"
}

Write-Output "Godot .NET`t$godotVersion"

if ([string]::IsNullOrWhiteSpace($ExportTemplatesPath)) {
    $applicationData = [Environment]::GetFolderPath('ApplicationData')
    if ([string]::IsNullOrWhiteSpace($applicationData)) {
        throw 'Godot export-template location is unavailable. Set DUNGEON_BARRAGE_GODOT_TEMPLATES explicitly.'
    }
    $ExportTemplatesPath = Join-Path $applicationData 'Godot\export_templates\4.7.1.stable'
}

if (-not (Test-Path -LiteralPath $ExportTemplatesPath -PathType Container)) {
    throw "Godot 4.7.1 .NET export templates are missing: $ExportTemplatesPath"
}

$templateVersionFile = Join-Path $ExportTemplatesPath 'version.txt'
if (-not (Test-Path -LiteralPath $templateVersionFile -PathType Leaf)) {
    throw "Godot export templates have no version.txt: $ExportTemplatesPath"
}

$templateVersion = (Get-Content -LiteralPath $templateVersionFile -Raw -Encoding utf8).Trim()
if ($templateVersion -notmatch '^4\.7\.1\.stable\.mono$') {
    throw "Godot export-template version mismatch. Expected 4.7.1.stable.mono but received: $templateVersion"
}

$requiredTemplates = @(
    'windows_debug_x86_64.exe',
    'windows_release_x86_64.exe'
)
foreach ($templateName in $requiredTemplates) {
    $templateFile = Join-Path $ExportTemplatesPath $templateName
    if (-not (Test-Path -LiteralPath $templateFile -PathType Leaf)) {
        throw "Godot .NET export template is incomplete; missing: $templateFile"
    }
}

Write-Output "Godot templates`t$templateVersion`t$ExportTemplatesPath"
Write-Output 'Dungeon Barrage toolchain verification passed.'
