[CmdletBinding()]
param(
    [string]$Source = "glosswork.png"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Resolve paths relative to this script, not the caller's working directory.
$root = Split-Path -Parent $PSScriptRoot

if (-not $root) {
    $root = (Get-Location).Path
}

$sourcePath = Join-Path $root $Source
$winAssets  = Join-Path $root "assets/windows"
$msixAssets = Join-Path $root "packaging/msix/Assets"

# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------

if (-not (Get-Command magick -ErrorAction SilentlyContinue)) {
    throw "ImageMagick is not installed or 'magick' is not available in PATH."
}

if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
    throw "Source image not found: $sourcePath"
}

if ([IO.Path]::GetExtension($sourcePath) -ne ".png") {
    throw "Source image must be a PNG: $sourcePath"
}

# Verify the source is square.
$dimensions = & magick identify -format "%w %h" -- $sourcePath

if ($LASTEXITCODE -ne 0) {
    throw "Failed to read source image: $sourcePath"
}

$width, $height = $dimensions -split " " | ForEach-Object { [int]$_ }

if ($width -ne $height) {
    throw "Source image must be square. Got ${width}x${height}."
}

if ($width -lt 600) {
    Write-Warning "Source image is only ${width}x${height}. A 1024x1024 or larger source is recommended."
}

# ---------------------------------------------------------------------------
# Directories
# ---------------------------------------------------------------------------

New-Item -ItemType Directory -Force -Path $winAssets  | Out-Null
New-Item -ItemType Directory -Force -Path $msixAssets | Out-Null

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

function Invoke-Magick {
    param(
        [Parameter(Mandatory)]
        [string[]]$Arguments
    )

    & magick @Arguments

    if ($LASTEXITCODE -ne 0) {
        throw "ImageMagick failed with exit code $LASTEXITCODE."
    }
}

function New-IconAsset {
    param(
        [Parameter(Mandatory)]
        [int]$Size,

        [Parameter(Mandatory)]
        [string]$Output
    )

    Invoke-Magick @(
        $sourcePath
        "-filter", "Lanczos"
        "-resize", "${Size}x${Size}"
        "-strip"
        $Output
    )
}

# ---------------------------------------------------------------------------
# Win32 ICO
# ---------------------------------------------------------------------------

$icoPath = Join-Path $winAssets "glosswork.ico"

Invoke-Magick @(
    $sourcePath
    "-define", "icon:auto-resize=256,64,48,32,24,16"
    $icoPath
)

# ---------------------------------------------------------------------------
# MSIX scale-qualified assets
# ---------------------------------------------------------------------------

$scaledAssets = @{
    "Square44x44Logo.scale-100.png"   = 44
    "Square44x44Logo.scale-200.png"   = 88
    "Square44x44Logo.scale-400.png"   = 176

    "Square150x150Logo.scale-100.png" = 150
    "Square150x150Logo.scale-200.png" = 300
    "Square150x150Logo.scale-400.png" = 600

    "StoreLogo.scale-100.png"         = 50
    "StoreLogo.scale-200.png"         = 100
    "StoreLogo.scale-400.png"         = 200
}

foreach ($asset in $scaledAssets.GetEnumerator()) {
    New-IconAsset `
        -Size $asset.Value `
        -Output (Join-Path $msixAssets $asset.Key)
}

# ---------------------------------------------------------------------------
# MSIX target-size assets
# ---------------------------------------------------------------------------

foreach ($size in 16, 24, 32, 48, 256) {
    New-IconAsset `
        -Size $size `
        -Output (Join-Path $msixAssets "Square44x44Logo.targetsize-$size.png")
}

Write-Host "Windows assets generated successfully:"
Write-Host "  ICO:  $icoPath"
Write-Host "  MSIX: $msixAssets"