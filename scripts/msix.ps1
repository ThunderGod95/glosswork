#Requires -Version 5.1

[CmdletBinding()]
param(
    # Force regeneration of ICO/MSIX assets through scripts/icons.ps1.
    [switch]$RegenerateIcons,

    # Package an already-built release binary.
    [switch]$SkipBuild,

    # Install the finished package after signing/verifying it.
    [switch]$Install,

    # Optional. Useful if multiple matching signing certificates exist.
    [string]$CertificateThumbprint
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))

$cargoManifest   = Join-Path $root "Cargo.toml"
$manifestSource = Join-Path $root "packaging\msix\AppxManifest.xml"
$assetSource     = Join-Path $root "packaging\msix\Assets"
$iconsScript     = Join-Path $PSScriptRoot "icons.ps1"
$win32Icon       = Join-Path $root "assets\windows\glosswork.ico"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

function Assert-File
{
    param(
        [Parameter(Mandatory)]
        [string]$Path
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf))
    {
        throw "Required file not found: $Path"
    }
}

function Assert-Directory
{
    param(
        [Parameter(Mandatory)]
        [string]$Path
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Container))
    {
        throw "Required directory not found: $Path"
    }
}

function Invoke-Native
{
    param(
        [Parameter(Mandatory)]
        [string]$FilePath,

        [Parameter(Mandatory)]
        [string[]]$ArgumentList,

        [Parameter(Mandatory)]
        [string]$Description
    )

    Write-Host ""
    Write-Host "==> $Description"

    & $FilePath @ArgumentList

    if ($LASTEXITCODE -ne 0)
    {
        throw "$Description failed with exit code $LASTEXITCODE."
    }
}

function Get-WindowsSdkTools
{
    $programFilesX86 =
    [Environment]::GetFolderPath(
        [Environment+SpecialFolder]::ProgramFilesX86
    )

    $sdkRoot = Join-Path $programFilesX86 "Windows Kits\10\bin"

    if (-not (Test-Path -LiteralPath $sdkRoot -PathType Container))
    {
        throw "Windows SDK bin directory not found: $sdkRoot"
    }

    $versions = Get-ChildItem -LiteralPath $sdkRoot -Directory |
        Where-Object {
            $_.Name -match '^\d+\.\d+\.\d+\.\d+$'
        } |
        Sort-Object -Property {
            [Version]$_.Name
        } -Descending

    foreach ($version in $versions)
    {
        $toolDir = Join-Path $version.FullName "x64"

        $makeAppx = Join-Path $toolDir "MakeAppx.exe"
        $signTool = Join-Path $toolDir "SignTool.exe"

        if (
            (Test-Path -LiteralPath $makeAppx -PathType Leaf) -and
            (Test-Path -LiteralPath $signTool -PathType Leaf)
        )
        {
            return [pscustomobject]@{
                Version  = $version.Name
                MakeAppx = $makeAppx
                SignTool = $signTool
            }
        }
    }

    throw "Could not find MakeAppx.exe and SignTool.exe in the Windows SDK."
}

function Test-CertificateTrusted
{
    param(
        [Parameter(Mandatory)]
        [System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate
    )

    $thumbprint = $Certificate.Thumbprint

    $stores = @(
        "Cert:\CurrentUser\TrustedPeople",
        "Cert:\LocalMachine\TrustedPeople",
        "Cert:\CurrentUser\Root",
        "Cert:\LocalMachine\Root"
    )

    foreach ($store in $stores)
    {
        if (-not (Test-Path $store))
        {
            continue
        }

        $match = Get-ChildItem $store -ErrorAction SilentlyContinue |
            Where-Object {
                $_.Thumbprint -eq $thumbprint
            } |
            Select-Object -First 1

        if ($null -ne $match)
        {
            return $true
        }
    }

    return $false
}

function Get-SigningCertificate
{
    param(
        [Parameter(Mandatory)]
        [string]$Publisher,

        [string]$Thumbprint
    )

    $normalizedThumbprint = $null

    if ($Thumbprint)
    {
        $normalizedThumbprint =
        ($Thumbprint -replace '\s', '').ToUpperInvariant()
    }

    $stores = @(
        [pscustomobject]@{
            Scope = "CurrentUser"
            Path  = "Cert:\CurrentUser\My"
        },
        [pscustomobject]@{
            Scope = "LocalMachine"
            Path  = "Cert:\LocalMachine\My"
        }
    )

    $now = Get-Date
    $candidates = @()

    foreach ($store in $stores)
    {
        if (-not (Test-Path $store.Path))
        {
            continue
        }

        foreach ($certificate in Get-ChildItem $store.Path)
        {
            if (-not $certificate.HasPrivateKey)
            {
                continue
            }

            if (
                $certificate.NotBefore -gt $now -or
                $certificate.NotAfter -le $now
            )
            {
                continue
            }

            if ($normalizedThumbprint)
            {
                $candidateThumbprint =
                ($certificate.Thumbprint -replace '\s', '').ToUpperInvariant()

                if ($candidateThumbprint -ne $normalizedThumbprint)
                {
                    continue
                }
            } elseif ($certificate.Subject -ne $Publisher)
            {
                continue
            }

            $candidates += [pscustomobject]@{
                Certificate = $certificate
                Scope       = $store.Scope
                Trusted     = Test-CertificateTrusted $certificate
            }
        }
    }

    if ($candidates.Count -eq 0)
    {
        if ($normalizedThumbprint)
        {
            throw "Signing certificate '$normalizedThumbprint' was not found with a private key."
        }

        throw "No valid signing certificate found for publisher '$Publisher'."
    }

    # Prefer a trusted certificate. This matters if several CN=ThunderGod95
    # development certificates have been created over time.
    $selected = $candidates |
        Sort-Object -Property `
        @{
            Expression = {
                if ($_.Trusted)
                { 1 
                } else
                { 0 
                }
            }
            Descending = $true
        },
        @{
            Expression = {
                $_.Certificate.NotBefore
            }
            Descending = $true
        } |
        Select-Object -First 1

    if ($selected.Certificate.Subject -ne $Publisher)
    {
        throw @"
Certificate subject does not match the MSIX publisher.

Manifest:    $Publisher
Certificate: $($selected.Certificate.Subject)
"@
    }

    return $selected
}

# ---------------------------------------------------------------------------
# Prerequisites
# ---------------------------------------------------------------------------

Assert-File $cargoManifest
Assert-File $manifestSource

$cargoCommand = Get-Command cargo -ErrorAction Stop
$rustcCommand = Get-Command rustc -ErrorAction Stop

$cargo = $cargoCommand.Source
$rustc = $rustcCommand.Source

# ---------------------------------------------------------------------------
# Cargo metadata
# ---------------------------------------------------------------------------

Write-Host "==> Reading Cargo metadata"

$metadataOutput = & $cargo metadata --format-version 1 --no-deps

if ($LASTEXITCODE -ne 0)
{
    throw "cargo metadata failed with exit code $LASTEXITCODE."
}

$metadata = ($metadataOutput -join "`n") | ConvertFrom-Json

$expectedManifest = [IO.Path]::GetFullPath($cargoManifest)

$package = $metadata.packages |
    Where-Object {
        [IO.Path]::GetFullPath([string]$_.manifest_path) -eq $expectedManifest
    } |
    Select-Object -First 1

if ($null -eq $package)
{
    throw "Could not locate the root package in cargo metadata."
}

$cargoVersion = [string]$package.version
$targetDir    = [IO.Path]::GetFullPath([string]$metadata.target_directory)

# Cargo SemVer -> MSIX's required four-number version.
$coreVersion = ($cargoVersion -split '[-+]')[0]
$parts = $coreVersion.Split(".")

if ($parts.Count -ne 3)
{
    throw "Unsupported Cargo version for MSIX conversion: $cargoVersion"
}

$numericParts = @()

foreach ($part in $parts)
{
    $value = 0

    if (-not [int]::TryParse($part, [ref]$value))
    {
        throw "Invalid numeric Cargo version component: '$part'"
    }

    if ($value -lt 0 -or $value -gt 65535)
    {
        throw "MSIX version component is outside 0..65535: $value"
    }

    $numericParts += $value
}

$msixVersion = "{0}.{1}.{2}.0" -f `
    $numericParts[0],
$numericParts[1],
$numericParts[2]

# ---------------------------------------------------------------------------
# Manifest metadata
# ---------------------------------------------------------------------------

[xml]$manifestXml = Get-Content -LiteralPath $manifestSource -Raw

$identity = $manifestXml.SelectSingleNode(
    "/*[local-name()='Package']/*[local-name()='Identity']"
)

$application = $manifestXml.SelectSingleNode(
    "/*[local-name()='Package']/*[local-name()='Applications']/*[local-name()='Application']"
)

if ($null -eq $identity)
{
    throw "Manifest does not contain a Package/Identity element."
}

if ($null -eq $application)
{
    throw "Manifest does not contain an Application element."
}

$packageName  = $identity.GetAttribute("Name")
$publisher    = $identity.GetAttribute("Publisher")
$architecture = $identity.GetAttribute("ProcessorArchitecture")
$executable   = $application.GetAttribute("Executable")

if (-not $packageName)
{
    throw "Manifest Identity.Name is empty."
}

if (-not $publisher)
{
    throw "Manifest Identity.Publisher is empty."
}

if (-not $architecture)
{
    throw "Manifest Identity.ProcessorArchitecture is empty."
}

if (-not $executable)
{
    throw "Manifest Application.Executable is empty."
}

$binaryName = [IO.Path]::GetFileNameWithoutExtension($executable)

# ---------------------------------------------------------------------------
# Verify Rust host architecture matches the manifest
# ---------------------------------------------------------------------------

$rustcInfo = & $rustc -vV

if ($LASTEXITCODE -ne 0)
{
    throw "rustc -vV failed."
}

$hostLine = $rustcInfo |
    Where-Object {
        $_ -like "host:*"
    } |
    Select-Object -First 1

if (-not $hostLine)
{
    throw "Could not determine the Rust host target."
}

$rustHost = ($hostLine -split ':', 2)[1].Trim()

$rustArchitecture = switch -Regex ($rustHost)
{
    '^x86_64-'
    { "x64"; break 
    }
    '^i686-'
    { "x86"; break 
    }
    '^aarch64-'
    { "arm64"; break 
    }
    default
    { $null 
    }
}

if (-not $rustArchitecture)
{
    throw "Unsupported Rust host architecture: $rustHost"
}

if ($rustArchitecture -ne $architecture)
{
    throw @"
Rust host architecture does not match AppxManifest.xml.

Rust:     $rustArchitecture ($rustHost)
Manifest: $architecture
"@
}

# ---------------------------------------------------------------------------
# Icons/assets
# ---------------------------------------------------------------------------

$requiredAssets = @(
    $win32Icon,
    (Join-Path $assetSource "Square44x44Logo.scale-100.png"),
    (Join-Path $assetSource "Square150x150Logo.scale-100.png"),
    (Join-Path $assetSource "StoreLogo.scale-100.png")
)

$missingAssets = @(
    $requiredAssets |
        Where-Object {
            -not (Test-Path -LiteralPath $_ -PathType Leaf)
        }
)

if ($RegenerateIcons -or $missingAssets.Count -gt 0)
{
    Assert-File $iconsScript

    Write-Host ""
    Write-Host "==> Generating Windows icon assets"

    & $iconsScript

    foreach ($asset in $requiredAssets)
    {
        Assert-File $asset
    }
}

Assert-Directory $assetSource
Assert-File $win32Icon

# ---------------------------------------------------------------------------
# Build Rust executable
# ---------------------------------------------------------------------------

if (-not $SkipBuild)
{
    Invoke-Native `
        -FilePath $cargo `
        -ArgumentList @(
        "build",
        "--release",
        "--bin",
        $binaryName
    ) `
        -Description "Building $binaryName.exe"
}

$binaryPath = Join-Path $targetDir "release\$binaryName.exe"

Assert-File $binaryPath

# ---------------------------------------------------------------------------
# Stage MSIX
# ---------------------------------------------------------------------------

$stageDir    = Join-Path $targetDir "msix"
$stageAssets = Join-Path $stageDir "Assets"

Write-Host ""
Write-Host "==> Staging MSIX package"

if (Test-Path -LiteralPath $stageDir)
{
    Remove-Item -LiteralPath $stageDir -Recurse -Force
}

New-Item -ItemType Directory -Path $stageAssets -Force | Out-Null

$stagedExecutable = Join-Path $stageDir $executable
$stagedExeParent  = Split-Path -Parent $stagedExecutable

if (-not (Test-Path -LiteralPath $stagedExeParent))
{
    New-Item -ItemType Directory -Path $stagedExeParent -Force | Out-Null
}

Copy-Item `
    -LiteralPath $binaryPath `
    -Destination $stagedExecutable `
    -Force

$stagedManifest = Join-Path $stageDir "AppxManifest.xml"

Copy-Item `
    -LiteralPath $manifestSource `
    -Destination $stagedManifest `
    -Force

Copy-Item `
    -Path (Join-Path $assetSource "*") `
    -Destination $stageAssets `
    -Recurse `
    -Force

# MakeAppx validates the unqualified files referenced by the manifest.
# Keep the scale-qualified originals and create base fallbacks from scale-100.
$baseAssets = @{
    "Square44x44Logo.png"   = "Square44x44Logo.scale-100.png"
    "Square150x150Logo.png" = "Square150x150Logo.scale-100.png"
    "StoreLogo.png"         = "StoreLogo.scale-100.png"
}

foreach ($entry in $baseAssets.GetEnumerator())
{
    $source = Join-Path $stageAssets $entry.Value
    $destination = Join-Path $stageAssets $entry.Key

    Assert-File $source

    Copy-Item `
        -LiteralPath $source `
        -Destination $destination `
        -Force
}

# ---------------------------------------------------------------------------
# Synchronize staged manifest version with Cargo.toml
# ---------------------------------------------------------------------------

[xml]$stagedManifestXml =
Get-Content -LiteralPath $stagedManifest -Raw

$stagedIdentity = $stagedManifestXml.SelectSingleNode(
    "/*[local-name()='Package']/*[local-name()='Identity']"
)

if ($null -eq $stagedIdentity)
{
    throw "Staged manifest is missing Identity."
}

$stagedIdentity.SetAttribute("Version", $msixVersion)

$stagedManifestXml.Save($stagedManifest)

# ---------------------------------------------------------------------------
# Validate manifest asset references before invoking MakeAppx
# ---------------------------------------------------------------------------

[xml]$validationManifest =
Get-Content -LiteralPath $stagedManifest -Raw

$propertiesLogo = $validationManifest.SelectSingleNode(
    "/*[local-name()='Package']/*[local-name()='Properties']/*[local-name()='Logo']"
)

$visualElements = $validationManifest.SelectSingleNode(
    "/*[local-name()='Package']/*[local-name()='Applications']/*[local-name()='Application']/*[local-name()='VisualElements']"
)

$manifestAssets = @()

if ($null -ne $propertiesLogo)
{
    $manifestAssets += $propertiesLogo.InnerText
}

if ($null -ne $visualElements)
{
    $manifestAssets += $visualElements.GetAttribute("Square44x44Logo")
    $manifestAssets += $visualElements.GetAttribute("Square150x150Logo")
}

foreach ($asset in $manifestAssets)
{
    if (-not $asset)
    {
        continue
    }

    Assert-File (Join-Path $stageDir $asset)
}

# ---------------------------------------------------------------------------
# Locate Windows SDK
# ---------------------------------------------------------------------------

$sdk = Get-WindowsSdkTools

Write-Host ""
Write-Host "Windows SDK: $($sdk.Version)"

# ---------------------------------------------------------------------------
# Package
# ---------------------------------------------------------------------------

$safeVersion = $cargoVersion -replace '[^0-9A-Za-z._-]', '_'

$outputPath = Join-Path $targetDir (
    "{0}-{1}-{2}.msix" -f `
        $packageName,
    $safeVersion,
    $architecture
)

if (Test-Path -LiteralPath $outputPath)
{
    Remove-Item -LiteralPath $outputPath -Force
}

Invoke-Native `
    -FilePath $sdk.MakeAppx `
    -ArgumentList @(
    "pack",
    "/d", $stageDir,
    "/p", $outputPath
) `
    -Description "Creating MSIX package"

Assert-File $outputPath

# ---------------------------------------------------------------------------
# Signing certificate
# ---------------------------------------------------------------------------

$signing = Get-SigningCertificate `
    -Publisher $publisher `
    -Thumbprint $CertificateThumbprint

$certificate = $signing.Certificate

Write-Host ""
Write-Host "Signing certificate:"
Write-Host "  Subject:    $($certificate.Subject)"
Write-Host "  Thumbprint: $($certificate.Thumbprint)"
Write-Host "  Store:      $($signing.Scope)"

if (-not $signing.Trusted)
{
    throw @"
The signing certificate exists but is not trusted.

Thumbprint:
$($certificate.Thumbprint)

Trust this exact certificate before building/installing the package.
"@
}

# ---------------------------------------------------------------------------
# Sign
# ---------------------------------------------------------------------------

$signArguments = @(
    "sign",
    "/fd", "SHA256",
    "/sha1", $certificate.Thumbprint,
    "/s", "My"
)

if ($signing.Scope -eq "LocalMachine")
{
    $signArguments += "/sm"
}

$signArguments += $outputPath

Invoke-Native `
    -FilePath $sdk.SignTool `
    -ArgumentList $signArguments `
    -Description "Signing MSIX package"

# ---------------------------------------------------------------------------
# Verify signature
# ---------------------------------------------------------------------------

Invoke-Native `
    -FilePath $sdk.SignTool `
    -ArgumentList @(
    "verify",
    "/pa",
    "/v",
    $outputPath
) `
    -Description "Verifying MSIX signature"

# ---------------------------------------------------------------------------
# Optional installation
# ---------------------------------------------------------------------------

if ($Install)
{
    Write-Host ""
    Write-Host "==> Installing MSIX"

    Add-AppxPackage -Path $outputPath

    Write-Host "Installed successfully."
}

# ---------------------------------------------------------------------------
# Result
# ---------------------------------------------------------------------------

Write-Host ""
Write-Host "MSIX build successful."
Write-Host "  Cargo version: $cargoVersion"
Write-Host "  MSIX version:  $msixVersion"
Write-Host "  Architecture:  $architecture"
Write-Host "  Package:       $outputPath"
