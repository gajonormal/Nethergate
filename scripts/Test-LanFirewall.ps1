# Disposable Windows firewall integration test. Requires administrator permission.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$fixture = Join-Path $root 'hub-data/players-ui-test-733b7613/server'
$operation = Join-Path $root ('hub-data/firewall-test-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
$ruleName = 'NetherGate-LAN-fixture-25591'
$powershell = Join-Path $PSHOME 'powershell.exe'
$engine = Join-Path $PSScriptRoot 'LanFirewall.ps1'
$result = [ordered]@{ status='failed'; removed=$false }
$created = $false
$original = $null
try {
    if (Get-NetFirewallRule -Name $ruleName -ErrorAction SilentlyContinue) { throw 'The disposable rule already exists; test refused.' }
    if (Get-NetTCPConnection -LocalPort 25591 -State Listen -ErrorAction SilentlyContinue) { throw 'The fixture must be stopped.' }
    $adapter = Get-NetIPAddress -IPAddress '192.168.1.4' -InterfaceAlias 'Ethernet' -AddressFamily IPv4
    $null = New-Item -ItemType Directory -Path $operation
    $properties = Join-Path $fixture 'server.properties'
    $original = [IO.File]::ReadAllBytes($properties)
    $text = [IO.File]::ReadAllText($properties)
    $text = [regex]::Replace($text, '(?m)^server-ip=.*$', 'server-ip=192.168.1.4')
    [IO.File]::WriteAllText($properties, $text)
    $plan = Join-Path $operation 'plan.json'
    @{ id='fixture'; address='192.168.1.4'; port=25591; interface='Ethernet'; java='java'; serverDirectory=$fixture } | ConvertTo-Json | Set-Content -LiteralPath $plan -Encoding UTF8
    $created = $true
    & $powershell -NoProfile -NonInteractive -File $engine -Mode Apply -Plan $plan
    if ($LASTEXITCODE -ne 0) { throw 'Creating test rule failed.' }
    $aligned = (& $powershell -NoProfile -NonInteractive -File $engine -Mode Inspect -Plan $plan) | ConvertFrom-Json
    if ($aligned.state -ne 'aligned') { throw 'New test rule was not confirmed.' }
    Set-NetFirewallRule -Name $ruleName -LocalAddress '192.168.1.250'
    $stale = (& $powershell -NoProfile -NonInteractive -File $engine -Mode Inspect -Plan $plan) | ConvertFrom-Json
    if ($stale.state -ne 'stale_ip') { throw 'Stale IP was not detected.' }
    & $powershell -NoProfile -NonInteractive -File $engine -Mode Apply -Plan $plan
    if ($LASTEXITCODE -ne 0) { throw 'Repairing the test rule failed.' }
    $repaired = (& $powershell -NoProfile -NonInteractive -File $engine -Mode Inspect -Plan $plan) | ConvertFrom-Json
    if ($repaired.state -ne 'aligned') { throw 'Repaired test rule was not confirmed.' }
    $result.status = 'passed'
    $result.staleDetected = $true
    $result.repairVerified = $true
} catch {
    $result.error = $_.Exception.Message
} finally {
    if ($created) {
        $rule = Get-NetFirewallRule -Name $ruleName -ErrorAction SilentlyContinue
        if ($rule) { Remove-NetFirewallRule -Name $ruleName -ErrorAction Stop }
        $result.removed = -not [bool](Get-NetFirewallRule -Name $ruleName -ErrorAction SilentlyContinue)
    }
    if ($null -ne $original) { [IO.File]::WriteAllBytes((Join-Path $fixture 'server.properties'), $original) }
    $result | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'hub-data/firewall-integration-result.json') -Encoding UTF8
}
if ($result.status -ne 'passed' -or -not $result.removed) { exit 1 }
