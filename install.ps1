cargo install --path . --force

# mise users need refreshed shims
if (Get-Command mise -ErrorAction SilentlyContinue) {
    mise reshim
}

# Otherwise ensure Cargo bin is on the user PATH
$cargoBin = Join-Path $HOME ".cargo\bin"
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")

if (($userPath -split ';') -notcontains $cargoBin) {
    [Environment]::SetEnvironmentVariable(
        "Path",
        "$userPath;$cargoBin",
        "User"
    )
}

Write-Host "Glosswork installed successfully."
