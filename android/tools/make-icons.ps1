# Generates the monochrome white UI icons used by TouchIPTV.
# They are drawn with GDI+ (transparent background, white shapes) and tinted at runtime
# via ImageView#setColorFilter, because Android 4.4 has no VectorDrawable.
Add-Type -AssemblyName System.Drawing

$outRoot = "C:\Users\sz_jm\CodexWorkSpaces\YogaTablet8\tv\android\app\src\main\res\drawable-xhdpi"
New-Item -ItemType Directory -Force -Path $outRoot | Out-Null

$S = 96
$W = 7.0
$white = [System.Drawing.Color]::White

function New-Canvas {
    $bmp = New-Object System.Drawing.Bitmap($S, $S)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)
    return @($bmp, $g)
}

function New-Pen([double]$width) {
    $p = New-Object System.Drawing.Pen($white, [float]$width)
    $p.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $p.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $p.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    return $p
}

function Save-Icon($bmp, $g, $name) {
    $path = Join-Path $outRoot "$name.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Host "icon $name"
}

function New-TrianglePath([double[]]$pts) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $p.AddPolygon(@(
        (New-Object System.Drawing.PointF([float]$pts[0], [float]$pts[1])),
        (New-Object System.Drawing.PointF([float]$pts[2], [float]$pts[3])),
        (New-Object System.Drawing.PointF([float]$pts[4], [float]$pts[5]))
    ))
    return $p
}

function New-RoundedPath([double]$x, [double]$y, [double]$w, [double]$h, [double]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $p.AddArc([float]$x, [float]$y, [float]$d, [float]$d, 180, 90)
    $p.AddArc([float]($x + $w - $d), [float]$y, [float]$d, [float]$d, 270, 90)
    $p.AddArc([float]($x + $w - $d), [float]($y + $h - $d), [float]$d, [float]$d, 0, 90)
    $p.AddArc([float]$x, [float]($y + $h - $d), [float]$d, [float]$d, 90, 90)
    $p.CloseFigure()
    return $p
}

# ---------------------------------------------------------------- play
$c = New-Canvas; $g = $c[1]
$tri = New-TrianglePath @(34, 24, 34, 72, 74, 48)
$br = New-Object System.Drawing.SolidBrush($white)
$inflate = New-Object System.Drawing.Drawing2D.Matrix
$tri.Widen((New-Pen 3))
$g.FillPath($br, $tri)
Save-Icon $c[0] $g "ic_play"

# ---------------------------------------------------------------- pause
$c = New-Canvas; $g = $c[1]; $br = New-Object System.Drawing.SolidBrush($white)
$g.FillPath($br, (New-RoundedPath 30 24 12 48 4))
$g.FillPath($br, (New-RoundedPath 54 24 12 48 4))
Save-Icon $c[0] $g "ic_pause"

# ---------------------------------------------------------------- prev / next
$c = New-Canvas; $g = $c[1]; $br = New-Object System.Drawing.SolidBrush($white)
$g.FillPath($br, (New-RoundedPath 24 26 9 44 3))
$g.FillPath($br, (New-TrianglePath @(76, 26, 76, 70, 38, 48)))
Save-Icon $c[0] $g "ic_prev"

$c = New-Canvas; $g = $c[1]; $br = New-Object System.Drawing.SolidBrush($white)
$g.FillPath($br, (New-RoundedPath 63 26 9 44 3))
$g.FillPath($br, (New-TrianglePath @(20, 26, 20, 70, 58, 48)))
Save-Icon $c[0] $g "ic_next"

# ---------------------------------------------------------------- star
function New-StarPath([bool]$filled) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $cx = 48.0; $cy = 50.0; $rOut = 30.0; $rIn = 13.0
    $pts = New-Object System.Collections.ArrayList
    for ($i = 0; $i -lt 10; $i++) {
        $ang = -[Math]::PI / 2 + $i * [Math]::PI / 5
        $r = if ($i % 2 -eq 0) { $rOut } else { $rIn }
        [void]$pts.Add((New-Object System.Drawing.PointF([float]($cx + $r * [Math]::Cos($ang)), [float]($cy + $r * [Math]::Sin($ang)))))
    }
    $p.AddPolygon($pts.ToArray())
    return $p
}

$c = New-Canvas; $g = $c[1]; $br = New-Object System.Drawing.SolidBrush($white)
$g.FillPath($br, (New-StarPath $true))
Save-Icon $c[0] $g "ic_star_filled"

$c = New-Canvas; $g = $c[1]; $pen = New-Pen 6
$g.DrawPath($pen, (New-StarPath $true))
Save-Icon $c[0] $g "ic_star"

# ---------------------------------------------------------------- volume
function Draw-Speaker($g, $br) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $p.AddPolygon(@(
        (New-Object System.Drawing.PointF(22, 38)),
        (New-Object System.Drawing.PointF(36, 38)),
        (New-Object System.Drawing.PointF(50, 24)),
        (New-Object System.Drawing.PointF(50, 72)),
        (New-Object System.Drawing.PointF(36, 58)),
        (New-Object System.Drawing.PointF(22, 58))
    ))
    $g.FillPath($br, $p)
}

$c = New-Canvas; $g = $c[1]; $br = New-Object System.Drawing.SolidBrush($white); $pen = New-Pen 6
Draw-Speaker $g $br
$g.DrawArc($pen, 44, 28, 26, 40, -55, 110)
$g.DrawArc($pen, 44, 18, 44, 60, -50, 100)
Save-Icon $c[0] $g "ic_volume"

$c = New-Canvas; $g = $c[1]; $br = New-Object System.Drawing.SolidBrush($white); $pen = New-Pen 6
Draw-Speaker $g $br
$g.DrawLine($pen, 62, 40, 82, 60)
$g.DrawLine($pen, 82, 40, 62, 60)
Save-Icon $c[0] $g "ic_volume_off"

# ---------------------------------------------------------------- fullscreen in / out
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 7
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(20, 40)), (New-Object System.Drawing.PointF(20, 20)), (New-Object System.Drawing.PointF(40, 20))))
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(56, 20)), (New-Object System.Drawing.PointF(76, 20)), (New-Object System.Drawing.PointF(76, 40))))
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(76, 56)), (New-Object System.Drawing.PointF(76, 76)), (New-Object System.Drawing.PointF(56, 76))))
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(40, 76)), (New-Object System.Drawing.PointF(20, 76)), (New-Object System.Drawing.PointF(20, 56))))
Save-Icon $c[0] $g "ic_fullscreen"

$c = New-Canvas; $g = $c[1]; $pen = New-Pen 7
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(40, 20)), (New-Object System.Drawing.PointF(40, 40)), (New-Object System.Drawing.PointF(20, 40))))
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(56, 20)), (New-Object System.Drawing.PointF(56, 40)), (New-Object System.Drawing.PointF(76, 40))))
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(76, 56)), (New-Object System.Drawing.PointF(56, 56)), (New-Object System.Drawing.PointF(56, 76))))
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(20, 56)), (New-Object System.Drawing.PointF(40, 56)), (New-Object System.Drawing.PointF(40, 76))))
Save-Icon $c[0] $g "ic_fullscreen_exit"

# ---------------------------------------------------------------- refresh
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 7; $br = New-Object System.Drawing.SolidBrush($white)
$g.DrawArc($pen, 20, 20, 56, 56, 45, 290)
$arrow = New-TrianglePath @(66, 12, 82, 30, 58, 32)
$g.FillPath($br, $arrow)
Save-Icon $c[0] $g "ic_refresh"

# ---------------------------------------------------------------- bolt (speed test)
$c = New-Canvas; $g = $c[1]; $br = New-Object System.Drawing.SolidBrush($white)
$bolt = New-Object System.Drawing.Drawing2D.GraphicsPath
$bolt.AddPolygon(@(
    (New-Object System.Drawing.PointF(56, 10)),
    (New-Object System.Drawing.PointF(26, 54)),
    (New-Object System.Drawing.PointF(45, 54)),
    (New-Object System.Drawing.PointF(39, 86)),
    (New-Object System.Drawing.PointF(70, 40)),
    (New-Object System.Drawing.PointF(50, 40))
))
$g.FillPath($br, $bolt)
Save-Icon $c[0] $g "ic_bolt"

# ---------------------------------------------------------------- search
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 7
$g.DrawEllipse($pen, 18, 18, 44, 44)
$g.DrawLine($pen, 59, 59, 80, 80)
Save-Icon $c[0] $g "ic_search"

# ---------------------------------------------------------------- close
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 8
$g.DrawLine($pen, 26, 26, 70, 70)
$g.DrawLine($pen, 70, 26, 26, 70)
Save-Icon $c[0] $g "ic_close"

# ---------------------------------------------------------------- settings (sliders)
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 6; $br = New-Object System.Drawing.SolidBrush($white)
foreach ($y in @(30, 48, 66)) { $g.DrawLine($pen, 18, $y, 78, $y) }
$g.FillEllipse($br, [float](30 - 8), [float](30 - 8), 16, 16)
$g.FillEllipse($br, [float](62 - 8), [float](48 - 8), 16, 16)
$g.FillEllipse($br, [float](40 - 8), [float](66 - 8), 16, 16)
Save-Icon $c[0] $g "ic_settings"

# ---------------------------------------------------------------- tv
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 6
$g.DrawPath($pen, (New-RoundedPath 14 22 68 46 7))
$g.DrawLine($pen, 40, 76, 56, 76)
$g.DrawLine($pen, 48, 68, 48, 76)
Save-Icon $c[0] $g "ic_tv"

# ---------------------------------------------------------------- list
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 6; $br = New-Object System.Drawing.SolidBrush($white)
foreach ($y in @(28, 48, 68)) {
    $g.FillEllipse($br, 20, [float]($y - 5), 10, 10)
    $g.DrawLine($pen, 40, $y, 78, $y)
}
Save-Icon $c[0] $g "ic_list"

# ---------------------------------------------------------------- check
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 8
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(22, 50)), (New-Object System.Drawing.PointF(41, 69)), (New-Object System.Drawing.PointF(76, 28))))
Save-Icon $c[0] $g "ic_check"

# ---------------------------------------------------------------- chevron down
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 8
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(28, 38)), (New-Object System.Drawing.PointF(48, 60)), (New-Object System.Drawing.PointF(68, 38))))
Save-Icon $c[0] $g "ic_chevron_down"

# ---------------------------------------------------------------- aspect fit / fill
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 6
$g.DrawPath($pen, (New-RoundedPath 14 24 68 48 6))
$g.DrawLine($pen, 32, 48, 64, 48)
$g.DrawLine($pen, 32, 48, 40, 40)
$g.DrawLine($pen, 32, 48, 40, 56)
$g.DrawLine($pen, 64, 48, 56, 40)
$g.DrawLine($pen, 64, 48, 56, 56)
Save-Icon $c[0] $g "ic_aspect"

# ---------------------------------------------------------------- info
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 6; $br = New-Object System.Drawing.SolidBrush($white)
$g.DrawEllipse($pen, 18, 18, 60, 60)
$g.FillEllipse($br, 44, 30, 8, 8)
$g.DrawLine($pen, 48, 46, 48, 68)
Save-Icon $c[0] $g "ic_info"

# ---------------------------------------------------------------- wifi / signal
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 7; $br = New-Object System.Drawing.SolidBrush($white)
$g.DrawArc($pen, 20, 26, 56, 56, 210, 120)
$g.DrawArc($pen, 32, 38, 32, 32, 210, 120)
$g.FillEllipse($br, 42, 60, 12, 12)
Save-Icon $c[0] $g "ic_signal"

# ---------------------------------------------------------------- remote / dpad
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 6; $br = New-Object System.Drawing.SolidBrush($white)
$g.DrawPath($pen, (New-RoundedPath 26 12 44 72 14))
$g.FillEllipse($br, 42, 24, 12, 12)
$g.FillEllipse($br, 34, 44, 9, 9)
$g.FillEllipse($br, 53, 44, 9, 9)
$g.FillEllipse($br, 34, 60, 9, 9)
$g.FillEllipse($br, 53, 60, 9, 9)
Save-Icon $c[0] $g "ic_remote"

# ---------------------------------------------------------------- download / import
$c = New-Canvas; $g = $c[1]; $pen = New-Pen 7; $br = New-Object System.Drawing.SolidBrush($white)
$g.DrawLine($pen, 48, 18, 48, 58)
$g.FillPath($br, (New-TrianglePath @(30, 44, 66, 44, 48, 66)))
$g.DrawLines($pen, @((New-Object System.Drawing.PointF(20, 66)), (New-Object System.Drawing.PointF(20, 80)), (New-Object System.Drawing.PointF(76, 80)), (New-Object System.Drawing.PointF(76, 66))))
Save-Icon $c[0] $g "ic_download"
