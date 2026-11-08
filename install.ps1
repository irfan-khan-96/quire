# Install quire on Windows:
#
#   irm https://github.com/irfan-khan-96/quire/releases/latest/download/install.ps1 | iex
#
# Environment variables:
#   QUIRE_VERSION         release tag to install, e.g. v0.1.0 (default: latest)
#   QUIRE_INSTALL_DIR     where to put quire.exe (default: ~\.quire\bin)
#   QUIRE_NO_MODIFY_PATH  set to 1 to leave your user PATH unchanged
#   QUIRE_DOWNLOAD_URL    base URL or local directory holding the release files
$ErrorActionPreference = 'Stop'

$repo = 'irfan-khan-96/quire'
$version = if ($env:QUIRE_VERSION) { $env:QUIRE_VERSION } else { 'latest' }
$installDir = if ($env:QUIRE_INSTALL_DIR) { $env:QUIRE_INSTALL_DIR } else { Join-Path $HOME '.quire\bin' }
# Windows on ARM runs the x86_64 build through emulation.
$target = 'x86_64-pc-windows-msvc'
$archive = "quire-$target.zip"

if ($env:QUIRE_DOWNLOAD_URL) {
    $base = $env:QUIRE_DOWNLOAD_URL
} elseif ($version -eq 'latest') {
    $base = "https://github.com/$repo/releases/latest/download"
} else {
    $base = "https://github.com/$repo/releases/download/$version"
}

function Get-ReleaseFile([string]$Name, [string]$Destination) {
    if ($base -match '^https?://') {
        Invoke-WebRequest -Uri "$base/$Name" -OutFile $Destination -UseBasicParsing
    } else {
        Copy-Item -Path (Join-Path $base $Name) -Destination $Destination
    }
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("quire-install-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "Downloading quire ($version, $target)"
    Get-ReleaseFile $archive (Join-Path $tmp $archive)
    Get-ReleaseFile "$archive.sha256" (Join-Path $tmp "$archive.sha256")

    $expected = ((Get-Content (Join-Path $tmp "$archive.sha256") -Raw).Trim() -split '\s+')[0].ToLower()
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $archive)).Hash.ToLower()
    if ($expected -ne $actual) {
        throw "checksum mismatch for $archive (expected $expected, got $actual)"
    }

    Expand-Archive -Path (Join-Path $tmp $archive) -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    Copy-Item -Path (Join-Path $tmp "quire-$target\quire.exe") -Destination (Join-Path $installDir 'quire.exe') -Force
    Write-Host "Installed quire to $(Join-Path $installDir 'quire.exe')"
} finally {
    Remove-Item -Recurse -Force $tmp
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$onPath = ($userPath -split ';') -contains $installDir
if (-not $onPath) {
    if ($env:QUIRE_NO_MODIFY_PATH -eq '1') {
        Write-Host "Add $installDir to your PATH to run quire."
    } else {
        $newPath = if ($userPath) { "$userPath;$installDir" } else { $installDir }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        Write-Host "Added $installDir to your user PATH. Open a new terminal to use it."
    }
}
Write-Host 'Then run: quire doctor'
