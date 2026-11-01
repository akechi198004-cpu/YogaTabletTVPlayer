# One-shot device check / install / screenshot for the Yoga Tablet 8.
#
#   .\install-and-check.ps1                 # read-only: props + installed packages
#   .\install-and-check.ps1 -Install        # install the release APK, launch it, screenshot
#
# Requires an already authorised adb target. On Android 4.4 the network port has to be switched
# on once over USB:  adb tcpip 5555
param(
    [string]$Serial = '192.168.0.56:5555',
    [switch]$Install,
    [string]$Apk = ''
)

$ErrorActionPreference = 'Stop'
$OutputEncoding = New-Object System.Text.UTF8Encoding $false

$adb = 'C:\Users\sz_jm\CodexWorkSpaces\LeTV\tools\sdk\platform-tools\adb.exe'
$pkg = 'com.gathin.touchiptv'
if (-not $Apk) { $Apk = Join-Path (Split-Path $PSScriptRoot -Parent) 'dist\TouchIPTV-1.0.0.apk' }

& $adb connect $Serial | Out-Null
$devices = & $adb devices
if ($devices -notmatch ([regex]::Escape($Serial) + '\s+device')) {
    $state = ($devices | Select-String ([regex]::Escape($Serial)) | Select-Object -First 1)
    throw "adb target $Serial is not ready ($state). If it says 'unauthorized', accept the RSA prompt on the tablet; if it is missing, run 'adb tcpip 5555' over USB."
}

function Sh([string]$cmd) { & $adb -s $Serial shell $cmd }

Write-Host '== device =='
foreach ($p in @('ro.product.model', 'ro.product.manufacturer', 'ro.build.version.release',
        'ro.build.version.sdk', 'ro.build.display.id', 'ro.sf.lcd_density')) {
    "{0,-28} {1}" -f $p, ((Sh "getprop $p") -join '')
}
$size = (Sh 'wm size') -join ''
$density = (Sh 'wm density') -join ''
Write-Host $size
Write-Host $density

Write-Host '== decoders (h264) =='
Sh 'dumpsys media.player' | Select-String -Pattern 'h264|avc' | Select-Object -First 12

Write-Host '== is it installed? =='
$installed = (Sh "pm path $pkg") -join ''
if ($installed) { Write-Host "installed: $installed" } else { Write-Host 'not installed' }

if (-not $Install) {
    Write-Host ''
    Write-Host 'Read-only check done. Re-run with -Install to install + launch + screenshot.'
    return
}

Write-Host "== install $Apk =="
& $adb -s $Serial install -r $Apk
if ($LASTEXITCODE -ne 0) { throw 'adb install failed' }

& $adb -s $Serial logcat -c
Sh "am start -n $pkg/.MainActivity"
Start-Sleep -Seconds 6

$shots = Join-Path (Split-Path $PSScriptRoot -Parent) 'dist\screens'
New-Item -ItemType Directory -Force -Path $shots | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$remote = '/sdcard/touchiptv.png'
Sh "screencap -p $remote"
& $adb -s $Serial pull $remote (Join-Path $shots "shot-$stamp.png") | Out-Null
Sh "rm $remote"

Write-Host '== logcat (errors) =='
& $adb -s $Serial logcat -d -v time | Select-String -Pattern 'AndroidRuntime|FATAL|TouchIPTV|ExoPlayer|MediaCodec' |
    Select-Object -Last 60

Write-Host "screenshot -> $(Join-Path $shots "shot-$stamp.png")"
