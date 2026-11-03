$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.IO.Compression
$root=Split-Path -Parent $PSScriptRoot
$directory=Join-Path $root ('hub-data/java-extraction-'+[Guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $directory
$engine=Join-Path $PSScriptRoot 'Extract-Java.ps1'
function Test-Archive($label,$names,$expected) {
    $archive=Join-Path $directory ($label+'.zip')
    $zip=[IO.Compression.ZipFile]::Open($archive,[IO.Compression.ZipArchiveMode]::Create)
    try { foreach($name in $names) { $entry=$zip.CreateEntry($name);$stream=$entry.Open();try {$stream.WriteByte(42)}finally {$stream.Dispose()} } } finally {$zip.Dispose()}
    $destination=Join-Path $directory $label
    $ErrorActionPreference='Continue'
    $null=& (Join-Path $PSHOME 'powershell.exe') -NoProfile -NonInteractive -File $engine -Archive $archive -Destination $destination 2>&1
    $accepted=$LASTEXITCODE -eq 0
    $ErrorActionPreference='Stop'
    if($accepted -ne $expected) { throw ('Unexpected extraction outcome: '+$label) }
    if(-not $expected -and (Test-Path -LiteralPath $destination)) { throw ('Unsafe archive wrote files: '+$label) }
}
Test-Archive good @('runtime/bin/java.exe','runtime/legal/LICENSE') $true
Test-Archive traversal @('../escaped.txt') $false
Test-Archive absolute @('/escaped.txt') $false
Test-Archive duplicate @('runtime/bin/java.exe','runtime/bin/JAVA.exe') $false
Test-Archive stream @('runtime/bin/java.exe:payload') $false
@{status='passed';cases=5;directory=$directory} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'hub-data/java-extraction-result.json') -Encoding UTF8
Write-Output '5 archive extraction checks passed.'
