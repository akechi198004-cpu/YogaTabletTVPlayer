# Builds the release APK with the toolchain that lives in C:\Users\sz_jm\CodexWorkSpaces\LeTV\tools
$ErrorActionPreference = 'Stop'
$OutputEncoding = New-Object System.Text.UTF8Encoding $false
Set-Location $PSScriptRoot

$tools = 'C:\Users\sz_jm\CodexWorkSpaces\LeTV\tools'
if (-not $env:JAVA_HOME -or -not (Test-Path "$env:JAVA_HOME\bin\java.exe")) {
    $env:JAVA_HOME = "$tools\jdk\jdk-17.0.20.1+1"
}
if (-not $env:GRADLE_USER_HOME) {
    $env:GRADLE_USER_HOME = "$tools\gradle-home"
}
if (-not (Test-Path local.properties)) {
    "sdk.dir=$($tools -replace '\\', '/')/sdk" | Out-File -Encoding ascii local.properties
}

& .\gradlew.bat assembleRelease --console=plain
if ($LASTEXITCODE -ne 0) { throw 'Gradle build failed' }

$apk = 'app\build\outputs\apk\release\app-release.apk'
$dist = Join-Path (Split-Path $PSScriptRoot -Parent) 'dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item $apk (Join-Path $dist 'TouchIPTV-1.0.0.apk') -Force
Write-Host "APK -> $(Join-Path $dist 'TouchIPTV-1.0.0.apk')"
