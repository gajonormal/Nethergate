param([Parameter(Mandatory=$true)][string]$Archive,[Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$root=[IO.Path]::GetFullPath($Destination).TrimEnd('\')+'\'
if(Test-Path -LiteralPath $Destination) { throw 'Extraction destination already exists.' }
$zip=[IO.Compression.ZipFile]::OpenRead($Archive)
try {
    if($zip.Entries.Count -gt 20000) { throw 'Too many ZIP entries.' }
    $seen=New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
    [long]$total=0
    # Validate the complete archive before creating any extracted file.
    foreach($entry in $zip.Entries) {
        $name=$entry.FullName.Replace('\','/')
        if($name.StartsWith('/') -or $name.Contains(':') -or ($name.Split('/') -contains '..') -or ($name.Split('/') -contains '.')) { throw 'Unsafe ZIP path.' }
        $target=[IO.Path]::GetFullPath((Join-Path $root $name))
        if(-not $target.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) -or -not $seen.Add($target)) { throw 'Duplicate or escaping ZIP entry.' }
        if((($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) { throw 'ZIP symbolic links are not supported.' }
        $total+=$entry.Length
        if($total -gt 1073741824 -or $entry.Length -gt 536870912) { throw 'ZIP exceeds extraction limits.' }
    }
    $null=[IO.Directory]::CreateDirectory($root)
    foreach($entry in $zip.Entries) {
        $target=[IO.Path]::GetFullPath((Join-Path $root $entry.FullName))
        if($entry.FullName.EndsWith('/') -or $entry.FullName.EndsWith('\')) { $null=[IO.Directory]::CreateDirectory($target); continue }
        $null=[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target))
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry,$target,$false)
    }
} finally { $zip.Dispose() }
