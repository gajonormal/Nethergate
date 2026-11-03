$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskSource = Join-Path $taskRoot 'ui/assets/nethergate-mark.png'
$taskBitmap = [Drawing.Bitmap]::new($taskSource)
try {
    # Convert each original pixel into a solid vector cell: no filtering or new artwork.
    $taskCells = [Text.StringBuilder]::new()
    $taskMinX = $taskBitmap.Width
    $taskMinY = $taskBitmap.Height
    $taskMaxX = -1
    $taskMaxY = -1
    for ($taskY = 0; $taskY -lt $taskBitmap.Height; $taskY++) {
        for ($taskX = 0; $taskX -lt $taskBitmap.Width; $taskX++) {
            $taskPixel = $taskBitmap.GetPixel($taskX, $taskY)
            if ($taskPixel.A -eq 0) { continue }
            $taskMinX = [Math]::Min($taskMinX, $taskX)
            $taskMinY = [Math]::Min($taskMinY, $taskY)
            $taskMaxX = [Math]::Max($taskMaxX, $taskX)
            $taskMaxY = [Math]::Max($taskMaxY, $taskY)
            $taskColor = '#{0:x2}{1:x2}{2:x2}' -f $taskPixel.R, $taskPixel.G, $taskPixel.B
            $taskOpacity = ($taskPixel.A / 255).ToString('0.########', [Globalization.CultureInfo]::InvariantCulture)
            [void]$taskCells.Append("<rect x=`"$taskX`" y=`"$taskY`" width=`"1`" height=`"1`" fill=`"$taskColor`" opacity=`"$taskOpacity`"/>")
        }
    }
    if ($taskMaxX -lt 0) { throw 'O símbolo está vazio' }
    # Fit the visible artwork, not the transparent 16px canvas, in the icon square.
    $taskEdge = [Math]::Max($taskMaxX - $taskMinX + 1, $taskMaxY - $taskMinY + 1)
    $taskViewX = (($taskMinX + $taskMaxX + 1 - $taskEdge) / 2).ToString('0.########', [Globalization.CultureInfo]::InvariantCulture)
    $taskViewY = (($taskMinY + $taskMaxY + 1 - $taskEdge) / 2).ToString('0.########', [Globalization.CultureInfo]::InvariantCulture)
    $taskSvg = "<svg xmlns=`"http://www.w3.org/2000/svg`" width=`"512`" height=`"512`" viewBox=`"$taskViewX $taskViewY $taskEdge $taskEdge`" shape-rendering=`"crispEdges`"><title>Nethergate</title>$taskCells</svg>"
    $taskSvgPath = Join-Path $taskRoot 'src-tauri/icons/portal.svg'
    [IO.File]::WriteAllText($taskSvgPath, $taskSvg)
} finally { $taskBitmap.Dispose() }
$taskStaging = Join-Path $taskRoot 'src-tauri/target/brand-icons'
& (Join-Path $taskRoot 'node_modules/.bin/tauri.cmd') icon $taskSvgPath --output $taskStaging
if ($LASTEXITCODE -ne 0) { throw 'Falha ao gerar os ícones Tauri' }
Get-ChildItem -LiteralPath $taskStaging -File | Where-Object { $_.Extension -in '.png', '.ico', '.icns' } | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $taskRoot "src-tauri/icons/$($_.Name)")
}
# Keep the existing 64px asset current too.
$taskExtra = Join-Path $taskRoot 'src-tauri/target/brand-icons-64'
& (Join-Path $taskRoot 'node_modules/.bin/tauri.cmd') icon $taskSvgPath --output $taskExtra --png 64
if ($LASTEXITCODE -ne 0) { throw 'Falha ao gerar o ícone de 64px' }
Copy-Item -LiteralPath (Join-Path $taskExtra '64x64.png') -Destination (Join-Path $taskRoot 'src-tauri/icons/64x64.png')
Copy-Item -LiteralPath (Join-Path $taskRoot 'src-tauri/icons/32x32.png') -Destination (Join-Path $taskRoot 'public/favicon.png')
Copy-Item -LiteralPath (Join-Path $taskRoot 'src-tauri/icons/icon.ico') -Destination (Join-Path $taskRoot 'public/favicon.ico')
