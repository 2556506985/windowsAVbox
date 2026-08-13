param(
    [string]$Destination = (Join-Path $PSScriptRoot "..\src-tauri\resources\mpv")
)

$ErrorActionPreference = "Stop"

$version = "mpv-dev-x86_64-20251012-git-ad59ff1"
$url = "https://sourceforge.net/projects/mpv-player-windows/files/libmpv/$version.7z/download"
$archiveHash = "3B8B81A6BD0712DD8598B02AB57AE78A6BC9CCE9B91E348B5E26B34F8635A44D"
$dllHash = "73CF918A538A7A1B1D2B53736AB43270770B766E71DB39EAD272B3843A327245"

if (-not (Get-Command 7z.exe -ErrorAction SilentlyContinue)) {
    throw "7z.exe is required to extract the official libmpv archive."
}

$destinationPath = [System.IO.Path]::GetFullPath($Destination)
New-Item -ItemType Directory -Force -Path $destinationPath | Out-Null
$archive = Join-Path $env:TEMP "$version.7z"
$extract = Join-Path $env:TEMP "$version-extract"

& curl.exe -L --fail --silent --show-error --retry 5 --retry-all-errors --continue-at - $url -o $archive
if ($LASTEXITCODE -ne 0) {
    throw "Unable to download $url"
}

$actualArchiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
if ($actualArchiveHash -ne $archiveHash) {
    throw "Archive SHA-256 mismatch: $actualArchiveHash"
}

Remove-Item -LiteralPath $extract -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $extract | Out-Null
& 7z.exe x $archive "-o$extract" -y | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "Unable to extract $archive"
}

$dll = Join-Path $extract "libmpv-2.dll"
if (-not (Test-Path -LiteralPath $dll)) {
    throw "The archive did not contain libmpv-2.dll"
}

$actualDllHash = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash
if ($actualDllHash -ne $dllHash) {
    throw "libmpv SHA-256 mismatch: $actualDllHash"
}

Copy-Item -LiteralPath $dll -Destination (Join-Path $destinationPath "libmpv-2.dll") -Force
Remove-Item -LiteralPath $extract -Recurse -Force
Remove-Item -LiteralPath $archive -Force

"Installed libmpv-2.dll to $destinationPath"
