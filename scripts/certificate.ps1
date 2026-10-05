#Requires -Version 5.1

[CmdletBinding()]
param(
    # Create a new certificate even if a valid one already exists.
    [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$manifestPath = Join-Path $root "packaging\msix\AppxManifest.xml"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

function Test-IsAdministrator
{
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)

    return $principal.IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator
    )
}

function Restart-Elevated
{
    $shell = (Get-Process -Id $PID).Path

    $arguments = @(
        "-NoProfile"
        "-ExecutionPolicy"
        "Bypass"
        "-File"
        "`"$PSCommandPath`""
    )

    if ($Force)
    {
        $arguments += "-Force"
    }

    Write-Host "Administrator privileges are required. Requesting elevation..."

    $process = Start-Process `
        -FilePath $shell `
        -ArgumentList $arguments `
        -Verb RunAs `
        -Wait `
        -PassThru

    exit $process.ExitCode
}

function Test-CodeSigningCertificate
{
    param(
        [Parameter(Mandatory)]
        [System.Security.Cryptography.X509Certificates.X509Certificate2]
        $Certificate
    )

    foreach ($extension in $Certificate.Extensions)
    {
        if ($extension.Oid.Value -ne "2.5.29.37")
        {
            continue
        }

        foreach ($oid in $extension.EnhancedKeyUsages)
        {
            if ($oid.Value -eq "1.3.6.1.5.5.7.3.3")
            {
                return $true
            }
        }
    }

    return $false
}

function Test-CertificateTrusted
{
    param(
        [Parameter(Mandatory)]
        [System.Security.Cryptography.X509Certificates.X509Certificate2]
        $Certificate
    )

    $trusted = Get-ChildItem "Cert:\LocalMachine\TrustedPeople" |
        Where-Object {
            $_.Thumbprint -eq $Certificate.Thumbprint
        } |
        Select-Object -First 1

    return $null -ne $trusted
}

function Add-ToTrustedPeople
{
    param(
        [Parameter(Mandatory)]
        [System.Security.Cryptography.X509Certificates.X509Certificate2]
        $Certificate
    )

    if (Test-CertificateTrusted $Certificate)
    {
        return
    }

    Write-Host "==> Trusting certificate"

    # Copy only the public certificate into TrustedPeople.
    $bytes = $Certificate.Export(
        [Security.Cryptography.X509Certificates.X509ContentType]::Cert
    )

    $publicCertificate =
    [Security.Cryptography.X509Certificates.X509Certificate2]::new($bytes)

    $store = [Security.Cryptography.X509Certificates.X509Store]::new(
        [Security.Cryptography.X509Certificates.StoreName]::TrustedPeople,
        [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
    )

    try
    {
        $store.Open(
            [Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite
        )

        $store.Add($publicCertificate)
    } finally
    {
        $store.Close()
        $publicCertificate.Dispose()
    }
}

# ---------------------------------------------------------------------------
# Platform / elevation
# ---------------------------------------------------------------------------

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT)
{
    throw "This script can only run on Windows."
}

if (-not (Test-IsAdministrator))
{
    Restart-Elevated
}

# ---------------------------------------------------------------------------
# Read publisher from AppxManifest.xml
# ---------------------------------------------------------------------------

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf))
{
    throw "AppxManifest.xml not found: $manifestPath"
}

[xml]$manifest = Get-Content -LiteralPath $manifestPath -Raw

$identity = $manifest.SelectSingleNode(
    "/*[local-name()='Package']/*[local-name()='Identity']"
)

if ($null -eq $identity)
{
    throw "AppxManifest.xml does not contain a Package/Identity element."
}

$publisher = $identity.GetAttribute("Publisher")

if ([string]::IsNullOrWhiteSpace($publisher))
{
    throw "Identity.Publisher is missing from AppxManifest.xml."
}

Write-Host ""
Write-Host "MSIX certificate provisioning"
Write-Host "  Publisher: $publisher"

# ---------------------------------------------------------------------------
# Look for an existing usable certificate
# ---------------------------------------------------------------------------

$now = Get-Date

$existing = @(
    Get-ChildItem "Cert:\CurrentUser\My" |
        Where-Object {
            $_.Subject -eq $publisher -and
            $_.HasPrivateKey -and
            $_.NotBefore -le $now -and
            $_.NotAfter -gt $now -and
            (Test-CodeSigningCertificate $_)
        } |
        Sort-Object NotBefore -Descending
)

if (-not $Force -and $existing.Count -gt 0)
{
    $certificate = $existing[0]

    Write-Host ""
    Write-Host "==> Existing signing certificate found"
} else
{
    # -----------------------------------------------------------------------
    # Create certificate
    # -----------------------------------------------------------------------

    Write-Host ""
    Write-Host "==> Creating signing certificate"

    $certificate = New-SelfSignedCertificate `
        -Type Custom `
        -Subject $publisher `
        -FriendlyName "Glosswork MSIX" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -KeyAlgorithm RSA `
        -KeyLength 3072 `
        -HashAlgorithm SHA256 `
        -KeyUsage DigitalSignature `
        -KeyExportPolicy Exportable `
        -TextExtension @(
        "2.5.29.37={text}1.3.6.1.5.5.7.3.3",
        "2.5.29.19={text}"
    )

    if ($null -eq $certificate)
    {
        throw "New-SelfSignedCertificate did not return a certificate."
    }
}

# ---------------------------------------------------------------------------
# Trust exact signing certificate
# ---------------------------------------------------------------------------

Add-ToTrustedPeople $certificate

# ---------------------------------------------------------------------------
# Verify
# ---------------------------------------------------------------------------

$privateCopy = Get-Item (
    "Cert:\CurrentUser\My\{0}" -f $certificate.Thumbprint
)

if (-not $privateCopy.HasPrivateKey)
{
    throw "Certificate does not have an accessible private key."
}

if (-not (Test-CodeSigningCertificate $privateCopy))
{
    throw "Certificate does not have the Code Signing EKU."
}

if (-not (Test-CertificateTrusted $privateCopy))
{
    throw "Certificate was not successfully added to LocalMachine\TrustedPeople."
}

# ---------------------------------------------------------------------------
# Result
# ---------------------------------------------------------------------------

Write-Host ""
Write-Host "Certificate ready."
Write-Host "  Subject:     $($privateCopy.Subject)"
Write-Host "  Thumbprint:  $($privateCopy.Thumbprint)"
Write-Host "  Valid from:  $($privateCopy.NotBefore)"
Write-Host "  Valid until: $($privateCopy.NotAfter)"
Write-Host ""
Write-Host "Private key:"
Write-Host "  Cert:\CurrentUser\My\$($privateCopy.Thumbprint)"
Write-Host ""
Write-Host "Trusted certificate:"
Write-Host "  Cert:\LocalMachine\TrustedPeople\$($privateCopy.Thumbprint)"
