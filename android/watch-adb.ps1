# Watches for the tablet's adb TCP port to open, then connects and prints the device facts.
# Used when the port has to be switched on from the tablet side (root terminal: setprop service.adb.tcp.port).
param(
    [string]$Serial = '192.168.0.56:5555',
    [int]$Minutes = 10
)

$adb = 'C:\Users\sz_jm\CodexWorkSpaces\LeTV\tools\sdk\platform-tools\adb.exe'
$hostIp, $port = $Serial.Split(':')
$deadline = (Get-Date).AddMinutes($Minutes)

Write-Host "watching $Serial for up to $Minutes min..."

while ((Get-Date) -lt $deadline) {
    $client = New-Object System.Net.Sockets.TcpClient
    try {
        $async = $client.BeginConnect($hostIp, [int]$port, $null, $null)
        if ($async.AsyncWaitHandle.WaitOne(800) -and $client.Connected) {
            $client.Close()
            Write-Host "port $port is open at $(Get-Date -Format HH:mm:ss), connecting..."
            & $adb connect $Serial
            Start-Sleep -Seconds 2
            $state = (& $adb devices) | Select-String ([regex]::Escape($Serial))
            Write-Host "adb state: $state"
            if ("$state" -match '\sdevice$') {
                Write-Host '== device =='
                foreach ($p in @('ro.product.model', 'ro.build.version.release', 'ro.build.version.sdk', 'ro.sf.lcd_density')) {
                    "{0,-26} {1}" -f $p, ((& $adb -s $Serial shell "getprop $p") -join '')
                }
                Write-Host ((& $adb -s $Serial shell 'wm size') -join '')
                Write-Host ((& $adb -s $Serial shell 'wm density') -join '')
                return
            }
            if ("$state" -match 'unauthorized') {
                Write-Host 'Device answered but is unauthorized; accept the prompt on the tablet.'
                return
            }
        } else {
            $client.Close()
        }
    } catch {
        try { $client.Close() } catch {}
    }
    Start-Sleep -Seconds 5
}
Write-Host 'gave up waiting: port never opened'
