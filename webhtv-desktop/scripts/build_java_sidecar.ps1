param(
    [string]$Maven = $(if ($env:WEBHTV_MAVEN) { $env:WEBHTV_MAVEN } else { "D:\MR\App\Maven\bin\mvn.cmd" }),
    [string]$Repository = $(if ($env:WEBHTV_MAVEN_REPOSITORY) { $env:WEBHTV_MAVEN_REPOSITORY } else { "D:\MR\Data\Maven_repository" }),
    [string]$JavaHome = $(if ($env:WEBHTV_JAVA_HOME) { $env:WEBHTV_JAVA_HOME } else { "D:\MR\App\IntelliJ IDEA 2025.1\jbr" })
)

$ErrorActionPreference = "Stop"
$project = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\java-sidecar"))
$destination = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\src-tauri\resources\java-spider-sidecar-0.1.0.jar"))

if (-not (Test-Path -LiteralPath $Maven)) {
    throw "Maven was not found at $Maven"
}
if (-not (Test-Path -LiteralPath $Repository)) {
    throw "Maven repository was not found at $Repository"
}
if (-not (Test-Path -LiteralPath $JavaHome)) {
    throw "Java home was not found at $JavaHome"
}

$env:JAVA_HOME = $JavaHome
& $Maven "-f" (Join-Path $project "pom.xml") "-Dmaven.repo.local=$Repository" "-DskipTests" "clean" "package"
if ($LASTEXITCODE -ne 0) {
    throw "Java sidecar Maven build failed"
}

$jar = Join-Path $project "target\java-spider-sidecar-0.1.0.jar"
if (-not (Test-Path -LiteralPath $jar)) {
    throw "Java sidecar Maven output was not found at $jar"
}
Copy-Item -LiteralPath $jar -Destination $destination -Force
"Installed Java sidecar resource at $destination"
