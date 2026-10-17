# Generates the app icon set from primitives, so the repository carries no
# opaque binary art and the mark stays in one reviewable place.
#
#   pwsh -File scripts/make-icons.ps1
#
# The mark: two accent chevrons on a rounded dark tile — panes moving forward.

param(
    [string]$Out = (Join-Path $PSScriptRoot "..\apps\desktop\src-tauri\icons")
)

Add-Type -AssemblyName System.Drawing

function New-HyperPaneBitmap([int]$size) {
    $bmp = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)

    # Rounded tile.
    $radius = [float]($size * 0.22)
    $d = $radius * 2
    $tile = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $tile.AddArc(0, 0, $d, $d, 180, 90)
    $tile.AddArc($size - $d, 0, $d, $d, 270, 90)
    $tile.AddArc($size - $d, $size - $d, $d, $d, 0, 90)
    $tile.AddArc(0, $size - $d, $d, $d, 90, 90)
    $tile.CloseFigure()

    $bg = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
        [System.Drawing.PointF]::new(0, 0),
        [System.Drawing.PointF]::new([float]$size, [float]$size),
        [System.Drawing.ColorTranslator]::FromHtml("#161A22"),
        [System.Drawing.ColorTranslator]::FromHtml("#0C0E13"))
    $g.FillPath($bg, $tile)
    $bg.Dispose()
    $tile.Dispose()

    # Chevrons.
    $pen = [System.Drawing.Pen]::new(
        [System.Drawing.ColorTranslator]::FromHtml("#56D4DD"),
        [float]($size * 0.075))
    $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round

    $arm = $size * 0.15
    $half = $size * 0.17
    $mid = $size / 2.0

    foreach ($x in @(($size * 0.29), ($size * 0.55))) {
        $points = [System.Drawing.PointF[]]@(
            [System.Drawing.PointF]::new([float]$x, [float]($mid - $half)),
            [System.Drawing.PointF]::new([float]($x + $arm), [float]$mid),
            [System.Drawing.PointF]::new([float]$x, [float]($mid + $half))
        )
        $g.DrawLines($pen, $points)
    }

    $pen.Dispose()
    $g.Dispose()
    return $bmp
}

New-Item -ItemType Directory -Force -Path $Out | Out-Null

$targets = [ordered]@{
    "32x32.png"      = 32
    "128x128.png"    = 128
    "128x128@2x.png" = 256
    "icon.png"       = 512
}

foreach ($name in $targets.Keys) {
    $bmp = New-HyperPaneBitmap $targets[$name]
    $bmp.Save((Join-Path $Out $name), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "wrote $name ($($targets[$name])px)"
}

# Windows .ico: modern Windows accepts a single PNG-compressed 256x256 entry.
$bmp = New-HyperPaneBitmap 256
$stream = [System.IO.MemoryStream]::new()
$bmp.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
$png = $stream.ToArray()
$stream.Dispose()

$icoPath = Join-Path $Out "icon.ico"
$file = [System.IO.File]::Create($icoPath)
$writer = [System.IO.BinaryWriter]::new($file)
$writer.Write([UInt16]0)            # reserved
$writer.Write([UInt16]1)            # type: icon
$writer.Write([UInt16]1)            # one image
$writer.Write([Byte]0)              # width 0 means 256
$writer.Write([Byte]0)              # height 0 means 256
$writer.Write([Byte]0)              # palette size
$writer.Write([Byte]0)              # reserved
$writer.Write([UInt16]1)            # colour planes
$writer.Write([UInt16]32)           # bits per pixel
$writer.Write([UInt32]$png.Length)
$writer.Write([UInt32]22)           # payload offset
$writer.Write($png)
$writer.Flush()
$writer.Dispose()
$file.Dispose()
Write-Host "wrote icon.ico (256px embedded PNG)"

Write-Host ""
Write-Host "macOS bundles also need icon.icns. Either build on macOS and run"
Write-Host "  pnpm tauri icon assets/logo.svg"
Write-Host "or add the .icns through that command on any platform with the Tauri CLI."
