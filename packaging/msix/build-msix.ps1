# AetherShell MSIX Packaging Automation Script (PRD Section 16 & Gate G15)
[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [string]$OutputDir = "",
    [switch]$Sign = $true,
    [string]$CertificateThumbprint = ""
)

$ErrorActionPreference = "Stop"

Write-Host "=== Packaging AetherShell MSIX ===" -ForegroundColor Cyan

$WorkspaceRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = "$WorkspaceRoot\target\dist\msix"
}
$StageDir = "$PSScriptRoot\layout"

# 1. Prepare clean layout directory
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path "$StageDir\Assets" | Out-Null
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

# 2. Copy binaries
$BinDir = "$WorkspaceRoot\target\$Configuration"
if (-not (Test-Path "$BinDir\aethershell.exe")) {
    $BinDir = "$WorkspaceRoot\target\x86_64-pc-windows-msvc\$Configuration"
}

$RequiredBins = @("aethershell.exe", "aethershell-script-host.exe")
foreach ($bin in $RequiredBins) {
    $src = "$BinDir\$bin"
    if (-not (Test-Path $src)) {
        Write-Warning "Binary '$src' not found. Building release workspace binaries..."
        cargo build --workspace --bin aethershell --bin aethershell-script-host --release
        break
    }
}
foreach ($bin in $RequiredBins) {
    Copy-Item "$BinDir\$bin" "$StageDir\$bin"
}

# Copy WinUI 3 Settings Application & Self-Contained Runtime
$WinUiSettingsDir = "$WorkspaceRoot\apps\aethershell-settings\x64\Release"
if (Test-Path "$WinUiSettingsDir\aethershell-settings.exe") {
    Write-Host "Packaging native WinUI 3 Settings application and runtime..." -ForegroundColor Green
    Copy-Item "$WinUiSettingsDir\aethershell-settings.exe" "$StageDir\aethershell-settings.exe"
    Get-ChildItem -Path "$WinUiSettingsDir\*" -Include @("*.dll", "*.pri", "*.xbf") -File | ForEach-Object {
        Copy-Item $_.FullName "$StageDir\$($_.Name)"
    }
    if (Test-Path "$WinUiSettingsDir\Microsoft.UI.Xaml") {
        Copy-Item -Recurse "$WinUiSettingsDir\Microsoft.UI.Xaml" "$StageDir\Microsoft.UI.Xaml"
    }
} elseif (Test-Path "$BinDir\aethershell-settings.exe") {
    Write-Host "Packaging fallback Settings binary..." -ForegroundColor Yellow
    Copy-Item "$BinDir\aethershell-settings.exe" "$StageDir\aethershell-settings.exe"
}

# 3. Copy manifest, assets, schemas, licenses
Copy-Item "$PSScriptRoot\AppxManifest.xml" "$StageDir\AppxManifest.xml"
Copy-Item "$WorkspaceRoot\LICENSE" "$StageDir\LICENSE"
Copy-Item "$WorkspaceRoot\NOTICE.md" "$StageDir\NOTICE.md"
Copy-Item -Recurse "$WorkspaceRoot\assets\*" "$StageDir\Assets\"
Copy-Item -Recurse "$WorkspaceRoot\schemas" "$StageDir\schemas"

# 4. Copy authentic visual assets from packaging/msix/Assets
$SourceAssetsDir = "$PSScriptRoot\Assets"
if (Test-Path $SourceAssetsDir) {
    Copy-Item "$SourceAssetsDir\*.png" "$StageDir\Assets\"
} else {
    throw "Source assets directory '$SourceAssetsDir' not found."
}

# 5. Locate MakeAppx.exe and SignTool.exe from Windows SDK
$SdkPaths = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -ErrorAction SilentlyContinue | Where-Object { $_.Name -match "^10\." } | Sort-Object Name -Descending
$MakeAppx = $null
$SignTool = $null

foreach ($sdk in $SdkPaths) {
    $candidate = "$($sdk.FullName)\x64\makeappx.exe"
    if (Test-Path $candidate) {
        $MakeAppx = $candidate
        break
    }
}

foreach ($sdk in $SdkPaths) {
    $candidate = "$($sdk.FullName)\x64\signtool.exe"
    if (Test-Path $candidate) {
        $SignTool = $candidate
        break
    }
}

$PackageFile = "$OutputDir\ArchdukeViel.AetherShell_1.0.0.0_x64.msix"

if ($MakeAppx) {
    Write-Host "Creating MSIX package using $MakeAppx..." -ForegroundColor Cyan
    & $MakeAppx pack /d $StageDir /p $PackageFile /o
    if ($LASTEXITCODE -ne 0) {
        throw "makeappx.exe failed with exit code $LASTEXITCODE"
    }
    Write-Host "MSIX generated: $PackageFile" -ForegroundColor Green

    # 6. Signing workflow
    if ($Sign -and $SignTool) {
        Write-Host "Signing MSIX package using $SignTool..." -ForegroundColor Cyan
        $thumb = $CertificateThumbprint
        if ([string]::IsNullOrWhiteSpace($thumb)) {
            $cert = Get-ChildItem Cert:\CurrentUser\My -ErrorAction SilentlyContinue |
                Where-Object { $_.Subject -eq "CN=ArchdukeViel" } |
                Select-Object -First 1

            if (-not $cert) {
                Write-Host "Creating test code-signing certificate for CN=ArchdukeViel..." -ForegroundColor Yellow
                $cert = New-SelfSignedCertificate -Type Custom -Subject "CN=ArchdukeViel" `
                    -KeyUsage DigitalSignature -FriendlyName "AetherShell Test Cert" `
                    -CertStoreLocation "Cert:\CurrentUser\My" `
                    -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3")
            }
            $thumb = $cert.Thumbprint
        }

        & $SignTool sign /fd SHA256 /sha1 $thumb /v $PackageFile
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "signtool.exe failed with exit code $LASTEXITCODE; package created but unsigned."
        } else {
            Write-Host "Successfully signed MSIX: $PackageFile" -ForegroundColor Green
        }
    }
} else {
    Write-Warning "makeappx.exe not located in Windows SDK paths. Staged layout preserved in $StageDir for packaging."
}
