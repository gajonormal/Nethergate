param(
    [ValidateSet('Inspect', 'Repair', 'Apply')][string]$Mode = 'Inspect',
    [Parameter(Mandatory = $true)][string]$Plan
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$resultPath = Join-Path (Split-Path -Parent $Plan) 'result.json'
try {
    $p = Get-Content -LiteralPath $Plan -Raw | ConvertFrom-Json
    if ($p.id -notmatch '^[a-zA-Z0-9_-]{1,64}$' -or $p.port -lt 1024 -or $p.port -gt 65535) { throw 'Invalid server or port.' }
    $ip = [System.Net.IPAddress]::Parse($p.address)
    $b = $ip.GetAddressBytes()
    if ($b.Length -ne 4 -or -not ($b[0] -eq 10 -or ($b[0] -eq 172 -and $b[1] -ge 16 -and $b[1] -le 31) -or ($b[0] -eq 192 -and $b[1] -eq 168))) { throw 'A private IPv4 address is required.' }
    $null = Get-NetIPAddress -IPAddress $p.address -InterfaceAlias $p.interface -AddressFamily IPv4 -ErrorAction Stop
    $java = (Get-Command -Name $p.java -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    if ((Split-Path -Leaf $java) -notin @('java.exe', 'javaw.exe')) { throw 'The configured runtime is not a Java executable.' }
    $java = (Resolve-Path -LiteralPath $java).ProviderPath
    $config = Get-Content -LiteralPath (Join-Path $p.serverDirectory 'nethergate.json') -Raw | ConvertFrom-Json
    $props = Get-Content -LiteralPath (Join-Path $p.serverDirectory 'server.properties')
    $bind = @($props | Where-Object { $_ -match '^\s*server-ip\s*=' } | Select-Object -Last 1)
    if ($config.port -ne $p.port -or -not $bind -or ($bind[0] -replace '^\s*server-ip\s*=\s*', '').Trim() -ne $p.address) { throw 'Server configuration changed. Refresh the connection panel.' }
    $auth = @($props | Where-Object { $_ -match '^\s*online-mode\s*=' } | Select-Object -Last 1)
    if ($auth -and ($auth[0] -replace '^\s*online-mode\s*=\s*', '').Trim() -ne 'true') { throw 'Official authentication is required.' }

    $ownedName = 'NetherGate-LAN-' + $p.id + '-' + $p.port
    $rule = Get-NetFirewallRule -Name $ownedName -ErrorAction SilentlyContinue
    if (@($rule).Count -gt 1) { throw 'Multiple rules match this name. Administrator review is required.' }
    if (-not $rule) {
        $legacy = Get-NetFirewallRule -Name ('NetherGate-Minecraft-TCP-' + $p.port) -ErrorAction SilentlyContinue
        if (@($legacy).Count -gt 1) { throw 'Multiple legacy rules match this name. Administrator review is required.' }
        if ($legacy) {
            $legacyApp = $legacy | Get-NetFirewallApplicationFilter
            $legacyPort = $legacy | Get-NetFirewallPortFilter
            if ($legacyApp.Program -eq $java -and $legacyPort.Protocol -eq 'TCP' -and $legacyPort.LocalPort -eq [string]$p.port -and $legacy.Direction -eq 'Inbound' -and $legacy.Action -eq 'Allow') { $rule = $legacy }
        }
    }
    $ruleName = if ($rule) { $rule.Name } else { $ownedName }
    $before = $null
    if ($rule) {
        $af = $rule | Get-NetFirewallAddressFilter
        $pf = $rule | Get-NetFirewallPortFilter
        $app = $rule | Get-NetFirewallApplicationFilter
        $iface = $rule | Get-NetFirewallInterfaceFilter
        $before = [ordered]@{ name=$rule.Name; enabled=[string]$rule.Enabled; action=[string]$rule.Action; direction=[string]$rule.Direction; profile=[string]$rule.Profile; localAddress=@($af.LocalAddress); remoteAddress=@($af.RemoteAddress); program=$app.Program; protocol=$pf.Protocol; port=@($pf.LocalPort); interface=@($iface.InterfaceAlias) }
    }
    $aligned = $before -and $before.enabled -eq 'True' -and $before.action -eq 'Allow' -and $before.direction -eq 'Inbound' -and $before.profile -eq 'Any' -and $before.program -eq $java -and $before.protocol -eq 'TCP' -and $before.port.Count -eq 1 -and $before.port[0] -eq [string]$p.port -and $before.localAddress.Count -eq 1 -and $before.localAddress[0] -eq $p.address -and $before.remoteAddress.Count -eq 1 -and $before.remoteAddress[0] -eq 'LocalSubnet' -and $before.interface.Count -eq 1 -and $before.interface[0] -eq $p.interface
    $state = if ($aligned) { 'aligned' } elseif (-not $before) { 'missing' } elseif ($before.localAddress -notcontains $p.address) { 'stale_ip' } else { 'different' }
    $result = [ordered]@{ state=$state; address=$p.address; port=$p.port; interface=$p.interface; program=$java; ruleName=$ruleName; previous=$before; applied=$false }
    if ($Mode -eq 'Inspect' -or $aligned) {
        $output = $result | ConvertTo-Json -Depth 6 -Compress
        if ($Mode -eq 'Apply') { $output | Set-Content -LiteralPath $resultPath -Encoding UTF8 } else { Write-Output $output }
        exit 0
    }
    if ($Mode -eq 'Repair') {
        # Freeze the user-resolved Java path before the elevated process starts.
        $p.java = $java
        $p | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Plan -Encoding UTF8
        $argsLine = '-NoProfile -NonInteractive -File "' + $PSCommandPath + '" -Mode Apply -Plan "' + (Resolve-Path -LiteralPath $Plan).ProviderPath + '"'
        try { $child = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -Verb RunAs -WindowStyle Hidden -ArgumentList $argsLine -Wait -PassThru }
        catch { throw 'Administrator approval was cancelled or unavailable. No repair was confirmed.' }
        if (-not (Test-Path -LiteralPath $resultPath)) { throw 'No administrator result was received. Inspect the rule again before retrying.' }
        Get-Content -LiteralPath $resultPath -Raw
        exit $child.ExitCode
    }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Administrator permission is required.' }
    $before | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path (Split-Path -Parent $Plan) 'before.json') -Encoding UTF8
    $parameters = @{ Enabled='True'; Direction='Inbound'; Action='Allow'; Profile='Any'; Protocol='TCP'; LocalPort=$p.port; LocalAddress=$p.address; RemoteAddress='LocalSubnet'; InterfaceAlias=$p.interface; Program=$java; ErrorAction='Stop' }
    if ($rule) { Set-NetFirewallRule -Name $ruleName @parameters | Out-Null }
    else { New-NetFirewallRule -Name $ruleName -DisplayName ('Nethergate LAN - ' + $p.port) -Description 'NetherGate managed LAN rule. Local subnet only.' @parameters | Out-Null }
    $result.state = 'applied'
    $result.applied = $true
    $result | ConvertTo-Json -Depth 6 -Compress | Set-Content -LiteralPath $resultPath -Encoding UTF8
} catch {
    $failure = @{ state='unknown'; error=$_.Exception.Message; applied=$false } | ConvertTo-Json -Compress
    if ($Mode -eq 'Apply') { $failure | Set-Content -LiteralPath $resultPath -Encoding UTF8 } else { Write-Output $failure }
    exit 1
}
