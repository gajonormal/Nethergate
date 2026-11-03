# Creates only a unique disposable rule. Never changes the user's real server or router.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$id='validation-'+[Guid]::NewGuid().ToString('N').Substring(0,12)
$operation=Join-Path $root ('hub-data/public-validation/'+$id)
$name='NetherGate-Internet-'+$id
$engine=Join-Path $PSScriptRoot 'PublicFirewall.ps1'
$powershell=Join-Path $PSHOME 'powershell.exe'
$result=[ordered]@{status='failed';removed=$false;id=$id}
$created=$false
function Invoke-Engine($mode) {
    $output=& $powershell -NoProfile -NonInteractive -File $engine -Mode $mode -Plan $plan
    $code=$LASTEXITCODE
    $report=$output | ConvertFrom-Json
    if($code -ne 0) { throw $report.error }
    return $report
}
function Save-Plan { $p | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $plan -Encoding UTF8 }
try {
    $null=New-Item -ItemType Directory -Path $operation
    $adapter=Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -eq '192.168.1.4' -and $_.InterfaceAlias -eq 'Ethernet' } | Select-Object -First 1
    if(-not $adapter) { throw 'The reviewed test adapter is no longer assigned.' }
    if(Get-NetFirewallRule -Name $name -ErrorAction SilentlyContinue) { throw 'Disposable rule already exists.' }
    @{ port=25591;java='java' } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $operation 'nethergate.json')
    @('server-ip=192.168.1.4','online-mode=true','white-list=true') | Set-Content -LiteralPath (Join-Path $operation 'server.properties')
    '[{"uuid":"00000000-0000-0000-0000-000000000001","name":"Validation"}]' | Set-Content -LiteralPath (Join-Path $operation 'whitelist.json')
    $plan=Join-Path $operation 'plan.json'
    $p=@{id=$id;serverDirectory=$operation;address='192.168.1.4';port=25591;interface='Ethernet';java='java';authenticated=$true;whitelist=$true}
    Save-Plan
    $before=Invoke-Engine Inspect
    if($before.state -ne 'missing' -or $before.activeCount -ne 0 -or $before.rules.Count -ne 0) { throw 'Unexpected initial state.' }
    $p.expectedRules=$before.rulesSignature; $p.expectedPlan=$before.planSignature; Save-Plan
    $created=$true
    $null=Invoke-Engine ApplyEnable
    $enabled=Invoke-Engine Inspect
    if($enabled.activeCount -ne 1 -or -not $enabled.aligned) { throw 'New permission was not confirmed.' }
    $result.enableVerified=$true
    # An outdated review must never close a changed rule.
    & $powershell -NoProfile -NonInteractive -File $engine -Mode ApplyDisable -Plan $plan | Out-Null
    if($LASTEXITCODE -eq 0) { throw 'An outdated snapshot was accepted.' }
    $result.staleReviewRejected=$true
    # Removal still works when IP, Java and authentication are no longer usable.
    $p.address='192.168.1.250';$p.java='missing-java-for-test.exe';$p.interface=$null;$p.authenticated=$false;$p.whitelist=$false
    $p.expectedRules=$enabled.rulesSignature; Save-Plan
    $null=Invoke-Engine ApplyDisable
    $disabled=Invoke-Engine Inspect
    if($disabled.activeCount -ne 0 -or $disabled.state -ne 'disabled') { throw 'Closing the obsolete permission failed.' }
    $result.closeWithStaleConfiguration=$true
    # Ownership mismatch must be reported and must refuse mutation.
    Set-NetFirewallRule -Name $name -Description 'Unrecognized validation rule'
    $unknown=Invoke-Engine Inspect
    if($unknown.state -ne 'unrecognized') { throw 'Ownership mismatch was not reported.' }
    $p.expectedRules=$unknown.rulesSignature;Save-Plan
    & $powershell -NoProfile -NonInteractive -File $engine -Mode ApplyDisable -Plan $plan | Out-Null
    if($LASTEXITCODE -eq 0) { throw 'An unrecognized rule was accepted.' }
    $result.ownershipMismatchRejected=$true
    $result.status='passed'
} catch { $result.error=$_.Exception.Message }
finally {
    if($created -and (Get-NetFirewallRule -Name $name -ErrorAction SilentlyContinue)) { Remove-NetFirewallRule -Name $name }
    $result.removed=-not [bool](Get-NetFirewallRule -Name $name -ErrorAction SilentlyContinue)
    $result | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'hub-data/public-validation/result.json') -Encoding UTF8
}
if($result.status -ne 'passed' -or -not $result.removed) { exit 1 }
