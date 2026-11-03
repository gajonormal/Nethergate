param(
    [ValidateSet('Inspect','Enable','Disable','ApplyEnable','ApplyDisable')][string]$Mode='Inspect',
    [Parameter(Mandatory=$true)][string]$Plan
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$resultPath=Join-Path (Split-Path -Parent $Plan) 'result.json'
function Get-Signature($value) {
    $json=ConvertTo-Json -InputObject $value -Depth 8 -Compress
    $sha=[Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($json)))).Replace('-','').ToLowerInvariant() }
    finally { $sha.Dispose() }
}
function Read-PublicRules($id) {
    $pattern='^NetherGate-Internet-'+[regex]::Escape($id)+'(?:-[0-9]{1,5})?$'
    $found=@(Get-NetFirewallRule -Name 'NetherGate-Internet-*' -ErrorAction SilentlyContinue | Where-Object { $_.Name -match $pattern } | Sort-Object Name)
    $rules=@()
    foreach($rule in $found) {
        $address=$rule | Get-NetFirewallAddressFilter
        $port=$rule | Get-NetFirewallPortFilter
        $app=$rule | Get-NetFirewallApplicationFilter
        $iface=$rule | Get-NetFirewallInterfaceFilter
        $marker=$rule.Description -eq ('NetherGate managed public rule v1; server='+$id)
        $legacy=$id -eq 'existing' -and $rule.Name -eq 'NetherGate-Internet-existing-25565' -and $rule.Description -eq 'NetherGate public access for existing server.'
        $owned=($marker -or $legacy) -and $rule.Direction -eq 'Inbound' -and $rule.Action -eq 'Allow' -and $port.Protocol -eq 'TCP' -and (Split-Path -Leaf $app.Program) -in @('java.exe','javaw.exe')
        $rules += [ordered]@{ name=$rule.Name; owned=[bool]$owned; enabled=($rule.Enabled -eq 'True'); profile=[string]$rule.Profile; action=[string]$rule.Action; direction=[string]$rule.Direction; addresses=@($address.LocalAddress | Sort-Object); remoteAddresses=@($address.RemoteAddress | Sort-Object); ports=@($port.LocalPort | Sort-Object); protocol=$port.Protocol; program=$app.Program; interfaces=@($iface.InterfaceAlias | Sort-Object) }
    }
    return $rules
}
function Emit-Result($value) {
    $json=ConvertTo-Json -InputObject $value -Depth 8 -Compress
    if($Mode -like 'Apply*') { $json | Set-Content -LiteralPath $resultPath -Encoding UTF8 }
    else { Write-Output $json }
}
try {
    $p=Get-Content -LiteralPath $Plan -Raw | ConvertFrom-Json
    if($p.id -notmatch '^[a-zA-Z0-9_-]{1,64}$') { throw 'Invalid server identifier.' }
    $rules=@(Read-PublicRules $p.id)
    $active=@($rules | Where-Object { $_.enabled })
    $unknown=@($rules | Where-Object { -not $_.owned })
    $java=$null
    if($p.java) {
        $command=Get-Command -Name $p.java -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if($command -and (Split-Path -Leaf $command.Source) -in @('java.exe','javaw.exe')) { $java=(Resolve-Path -LiteralPath $command.Source).ProviderPath }
    }
    $desired=[ordered]@{ address=$p.address; port=$p.port; interface=$p.interface; program=$java; authenticated=[bool]$p.authenticated; whitelist=[bool]$p.whitelist }
    $aligned=$active.Count -eq 1 -and $active[0].owned -and $active[0].addresses.Count -eq 1 -and $active[0].addresses[0] -eq $p.address -and $active[0].ports.Count -eq 1 -and $active[0].ports[0] -eq [string]$p.port -and $active[0].program -eq $java -and $active[0].remoteAddresses.Count -eq 1 -and $active[0].remoteAddresses[0] -eq 'Any' -and $active[0].interfaces.Count -eq 1 -and $active[0].interfaces[0] -eq $p.interface -and $active[0].profile -eq 'Any'
    $state=if($unknown.Count) {'unrecognized'} elseif($active.Count) {'enabled'} elseif($rules.Count) {'disabled'} else {'missing'}
    $result=[ordered]@{ state=$state; activeCount=$active.Count; aligned=[bool]$aligned; rules=$rules; desired=$desired; rulesSignature=(Get-Signature $rules); planSignature=(Get-Signature $desired); routerState='not_queried'; applied=$false }
    if($Mode -eq 'Inspect') { Emit-Result $result; exit 0 }
    $enabling=$Mode -in @('Enable','ApplyEnable')
    if($p.expectedRules -ne $result.rulesSignature -or ($enabling -and $p.expectedPlan -ne $result.planSignature)) { throw 'The reviewed firewall state changed. Verify again before confirming.' }
    if($unknown.Count) { throw 'A matching rule is not recognized as a Nethergate permission. Review it with an administrator.' }
    if($enabling) {
        if($active.Count -and -not $aligned) { throw 'Close the previous public permission before changing its destination.' }
        if(-not $p.authenticated -or -not $p.whitelist) { throw 'Enable official authentication and whitelist before public access.' }
        if(-not $java -or -not $p.interface -or $p.port -lt 1024 -or $p.port -gt 65535) { throw 'Select an active LAN address and a valid Java runtime/port.' }
        $ip=[Net.IPAddress]::Parse($p.address); $b=$ip.GetAddressBytes()
        if($b.Length -ne 4 -or -not ($b[0] -eq 10 -or ($b[0] -eq 172 -and $b[1] -ge 16 -and $b[1] -le 31) -or ($b[0] -eq 192 -and $b[1] -eq 168))) { throw 'An assigned private IPv4 address is required.' }
        $null=Get-NetIPAddress -IPAddress $p.address -InterfaceAlias $p.interface -AddressFamily IPv4
        $config=Get-Content -LiteralPath (Join-Path $p.serverDirectory 'nethergate.json') -Raw | ConvertFrom-Json
        $props=Get-Content -LiteralPath (Join-Path $p.serverDirectory 'server.properties')
        foreach($required in @(('server-ip='+$p.address),'online-mode=true','white-list=true')) { if($props -notcontains $required) { throw ('Server configuration changed: '+$required) } }
        if($config.port -ne $p.port) { throw 'The configured port changed.' }
        $whitelist=@(Get-Content -LiteralPath (Join-Path $p.serverDirectory 'whitelist.json') -Raw | ConvertFrom-Json)
        if(-not $whitelist.Count) { throw 'Add at least one official player to the whitelist.' }
    }
    if(($enabling -and $aligned) -or (-not $enabling -and -not $active.Count)) { Emit-Result $result; exit 0 }
    if($Mode -in @('Enable','Disable')) {
        if($java) { $p.java=$java; $p | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $Plan -Encoding UTF8 }
        $applyMode=if($enabling) {'ApplyEnable'} else {'ApplyDisable'}
        $argsLine='-NoProfile -NonInteractive -File "'+$PSCommandPath+'" -Mode '+$applyMode+' -Plan "'+(Resolve-Path -LiteralPath $Plan).ProviderPath+'"'
        try { $child=Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -Verb RunAs -WindowStyle Hidden -ArgumentList $argsLine -Wait -PassThru }
        catch { throw 'Administrator approval was cancelled or unavailable. No change was confirmed.' }
        if(-not (Test-Path -LiteralPath $resultPath)) { throw 'No administrator result was received. Verify before retrying.' }
        Get-Content -LiteralPath $resultPath -Raw; exit $child.ExitCode
    }
    $principal=New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Administrator permission is required.' }
    ConvertTo-Json -InputObject $rules -Depth 8 | Set-Content -LiteralPath (Join-Path (Split-Path -Parent $Plan) 'before.json') -Encoding UTF8
    if($enabling) {
        $name='NetherGate-Internet-'+$p.id
        $parameters=@{Enabled='True'; Direction='Inbound'; Action='Allow'; Profile='Any'; Protocol='TCP'; LocalPort=$p.port; LocalAddress=$p.address; RemoteAddress='Any'; InterfaceAlias=$p.interface; Program=$java; ErrorAction='Stop'}
        if($rules | Where-Object { $_.name -eq $name }) { Set-NetFirewallRule -Name $name @parameters | Out-Null }
        else { New-NetFirewallRule -Name $name -DisplayName ('Nethergate Internet - '+$p.id) -Description ('NetherGate managed public rule v1; server='+$p.id) @parameters | Out-Null }
    } else {
        foreach($rule in $active) { Disable-NetFirewallRule -Name $rule.name -ErrorAction Stop }
    }
    $result.applied=$true
    Emit-Result $result
} catch {
    Emit-Result @{state='unknown';error=$_.Exception.Message;applied=$false}
    exit 1
}
