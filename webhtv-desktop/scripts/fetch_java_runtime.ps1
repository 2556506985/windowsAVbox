param(
    [string]$Destination = (Join-Path $PSScriptRoot "..\src-tauri\resources\java-runtime")
)

$ErrorActionPreference = "Stop"

$version = "17.0.19+10"
$archiveName = "OpenJDK17U-jre_x64_windows_hotspot_17.0.19_10.zip"
$url = "https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.19%2B10/$archiveName"
$archiveHash = "79A598E1FBB4E16582D92C4EE22280A3C4D72FD52606E1E46B1223C0FE53B0DA"
$markerValue = "Temurin JRE $version`n$archiveHash"

$destinationPath = [System.IO.Path]::GetFullPath($Destination)
$marker = Join-Path $destinationPath ".webhtv-runtime"
$javaw = Join-Path $destinationPath "bin\javaw.exe"

if ((Test-Path -LiteralPath $javaw) -and (Test-Path -LiteralPath $marker)) {
    $installed = [System.IO.File]::ReadAllText($marker).Trim()
    if ($installed -eq $markerValue.Trim()) {
        "Java runtime already installed at $destinationPath"
        exit 0
    }
}

$archive = Join-Path $env:TEMP $archiveName
$extract = Join-Path $env:TEMP "webhtv-temurin-$($version.Replace('+', '-'))"

& curl.exe -L --fail --silent --show-error --retry 5 --retry-all-errors --continue-at - $url -o $archive
if ($LASTEXITCODE -ne 0) {
    throw "Unable to download $url"
}

$actualHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
if ($actualHash -ne $archiveHash) {
    throw "Java archive SHA-256 mismatch: $actualHash"
}

Remove-Item -LiteralPath $extract -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $extract | Out-Null
Expand-Archive -LiteralPath $archive -DestinationPath $extract -Force

$runtimeRoot = Get-ChildItem -LiteralPath $extract -Directory |
    Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName "bin\javaw.exe") } |
    Select-Object -First 1
if (-not $runtimeRoot) {
    throw "The Java archive did not contain bin\javaw.exe"
}

$parent = Split-Path -Parent $destinationPath
New-Item -ItemType Directory -Force -Path $parent | Out-Null
Remove-Item -LiteralPath $destinationPath -Recurse -Force -ErrorAction SilentlyContinue
Copy-Item -LiteralPath $runtimeRoot.FullName -Destination $destinationPath -Recurse -Force
[System.IO.File]::WriteAllText($marker, $markerValue, [System.Text.UTF8Encoding]::new($false))
$readme = Join-Path $destinationPath "README.md"
$readmeText = "Temurin JRE $version for WebHomeTV Desktop. Source: $url`nArchive SHA-256: $archiveHash`n"
[System.IO.File]::WriteAllText($readme, $readmeText, [System.Text.UTF8Encoding]::new($false))

Remove-Item -LiteralPath $extract -Recurse -Force
Remove-Item -LiteralPath $archive -Force

"Installed Temurin JRE $version to $destinationPath"
