$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$source = [System.Drawing.Image]::FromFile((Join-Path $PSScriptRoot 'scopework-source.png'))
$sizes = @(16, 24, 32, 48, 64, 128, 256)
$frames = @()
try {
    foreach ($size in $sizes) {
        $bitmap = [System.Drawing.Bitmap]::new($size, $size)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
        $graphics.DrawImage($source, 0, 0, $size, $size)
        $memory = [System.IO.MemoryStream]::new()
        $bitmap.Save($memory, [System.Drawing.Imaging.ImageFormat]::Png)
        $frames += ,$memory.ToArray()
        if ($size -eq 256) { $bitmap.Save((Join-Path $PSScriptRoot 'scopework.png'), [System.Drawing.Imaging.ImageFormat]::Png) }
        $memory.Dispose(); $graphics.Dispose(); $bitmap.Dispose()
    }
} finally { $source.Dispose() }
$output = [System.IO.File]::Create((Join-Path $PSScriptRoot 'scopework.ico'))
$writer = [System.IO.BinaryWriter]::new($output)
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $dimension = if ($sizes[$i] -eq 256) { 0 } else { $sizes[$i] }
        $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
        $writer.Write([byte]0); $writer.Write([byte]0)
        $writer.Write([uint16]1); $writer.Write([uint16]32)
        $writer.Write([uint32]$frames[$i].Length); $writer.Write([uint32]$offset)
        $offset += $frames[$i].Length
    }
    foreach ($frame in $frames) { $writer.Write([byte[]]$frame) }
} finally { $writer.Dispose(); $output.Dispose() }
