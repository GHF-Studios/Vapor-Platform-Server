$ErrorActionPreference = 'Stop'

$scriptPath = Join-Path $PSScriptRoot '..\scripts\backup-to-usb.ps1'
$scriptPath = (Resolve-Path -LiteralPath $scriptPath).Path
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("vapor-usb-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw "ASSERT: $Message" }
}

function New-RecoveryArchive {
    param([string]$Root, [string]$ArchiveName, [string]$Payload, [switch]$CorruptKit, [switch]$WithoutKit, [switch]$IncompleteKit, [switch]$LegacyKit, [switch]$V2Kit)

    $stage = Join-Path $Root ($ArchiveName + '.stage')
    $recovery = Join-Path $stage 'vapor-platform-recovery'
    $kit = Join-Path $recovery 'source\recovery-kit'
    New-Item -ItemType Directory -Path $kit -Force | Out-Null
    $commit = '0123456789abcdef0123456789abcdef01234567'
    $files = [ordered]@{
        'recover-fresh-host.sh' = "#!/usr/bin/env bash`necho recovery`n"
        'recovery_bundle_metadata.py' = "#!/usr/bin/env python3`nprint('metadata')`n"
        'README.txt' = "Recovery instructions`n"
    }
    if (-not $LegacyKit.IsPresent) {
        $files['state-bundle-validate.py'] = "#!/usr/bin/env python3`nprint('validate')`n"
        $files['deployed_source_manifest.py'] = "#!/usr/bin/env python3`nprint('manifest')`n"
    }
    if (-not $LegacyKit.IsPresent -and -not $V2Kit.IsPresent) {
        $files['service_deploy_map.py'] = "#!/usr/bin/env python3`nDEPLOY_TARGETS = {}`n"
    }
    [System.IO.File]::WriteAllText((Join-Path $recovery 'source\root-commit.txt'), "$commit`n", [System.Text.UTF8Encoding]::new($false))
    foreach ($name in $files.Keys) {
        [System.IO.File]::WriteAllText((Join-Path $kit $name), $files[$name], [System.Text.UTF8Encoding]::new($false))
    }
    $hashLines = foreach ($name in $files.Keys) {
        $digest = (Get-FileHash -LiteralPath (Join-Path $kit $name) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$digest  $name"
    }
    [System.IO.File]::WriteAllText((Join-Path $kit 'SHA256SUMS'), (($hashLines -join "`n") + "`n"), [System.Text.UTF8Encoding]::new($false))
    [System.IO.File]::WriteAllText((Join-Path $recovery 'payload.txt'), $Payload, [System.Text.UTF8Encoding]::new($false))
    if ($CorruptKit.IsPresent) {
        $sumPath = Join-Path $kit 'SHA256SUMS'
        $sumLines = [System.IO.File]::ReadAllLines($sumPath)
        $sumLines[0] = ('0' * 64) + '  ' + ($sumLines[0].Substring(66))
        [System.IO.File]::WriteAllLines($sumPath, $sumLines, [System.Text.UTF8Encoding]::new($false))
    }
    if ($WithoutKit.IsPresent) {
        Remove-Item -LiteralPath $kit -Recurse -Force
    } elseif ($IncompleteKit.IsPresent) {
        Remove-Item -LiteralPath (Join-Path $kit 'recovery_bundle_metadata.py') -Force
    }
    $archivePath = Join-Path $Root $ArchiveName
    $tar = (Get-Command tar.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    & $tar -czf $archivePath -C $stage 'vapor-platform-recovery'
    if ($LASTEXITCODE -ne 0) { throw "Could not create recovery fixture archive (tar exit $LASTEXITCODE)." }
    return [System.IO.File]::ReadAllBytes($archivePath)
}

function New-Fixture {
    param([string]$Name, [switch]$CorruptLatest, [switch]$CorruptKit, [switch]$WithoutRecoveryKit, [switch]$IncompleteKit, [switch]$TruncateLatestArchive, [switch]$LegacyKit, [switch]$V2Kit, [switch]$LargeArchives, [switch]$ThreeArchives)

    $root = Join-Path $scratch $Name
    $remote = Join-Path $root 'remote'
    $destination = Join-Path $root 'usb'
    New-Item -ItemType Directory -Path $remote, $destination | Out-Null

    $olderName = 'vapor-platform-recovery-20261106T120000Z-111111111111.tar.gz'
    $secondHistoricalName = 'vapor-platform-recovery-20261106T130000Z-333333333333.tar.gz'
    $latestName = 'vapor-platform-recovery-20261107T120000Z-222222222222.tar.gz'
    $olderPayload = "known-good-$Name"
    $secondHistoricalPayload = "second-historical-good-$Name"
    $latestPayload = "latest-good-$Name"
    if ($LargeArchives.IsPresent) {
        $olderPayload += [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(1MB))
        $secondHistoricalPayload += [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(1MB))
        $latestPayload += [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(1MB))
    }
    $olderBytes = New-RecoveryArchive -Root $remote -ArchiveName $olderName -Payload $olderPayload -WithoutKit:$WithoutRecoveryKit
    $secondHistoricalBytes = if ($ThreeArchives.IsPresent) { New-RecoveryArchive -Root $remote -ArchiveName $secondHistoricalName -Payload $secondHistoricalPayload -WithoutKit:$WithoutRecoveryKit } else { $null }
    $latestBytes = New-RecoveryArchive -Root $remote -ArchiveName $latestName -Payload $latestPayload -CorruptKit:$CorruptKit -WithoutKit:$WithoutRecoveryKit -IncompleteKit:$IncompleteKit -LegacyKit:$LegacyKit -V2Kit:$V2Kit
    if ($TruncateLatestArchive.IsPresent) {
        [Array]::Resize([ref]$latestBytes, [Math]::Max(1, $latestBytes.Length - 32))
    }
    $wireBytes = if ($CorruptLatest.IsPresent) { [System.Text.Encoding]::UTF8.GetBytes("corrupt-$Name") } else { $latestBytes }
    [System.IO.File]::WriteAllBytes((Join-Path $remote $olderName), $olderBytes)
    if ($ThreeArchives.IsPresent) {
        [System.IO.File]::WriteAllBytes((Join-Path $remote $secondHistoricalName), $secondHistoricalBytes)
    }
    [System.IO.File]::WriteAllBytes((Join-Path $remote $latestName), $wireBytes)
    $olderHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($olderBytes)).ToLowerInvariant()
    $secondHistoricalHash = if ($ThreeArchives.IsPresent) { [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($secondHistoricalBytes)).ToLowerInvariant() } else { $null }
    $latestHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($latestBytes)).ToLowerInvariant()
    $latestSize = $latestBytes.Length
    $archiveEntries = @([ordered]@{ archive = $olderName; sha256 = $olderHash; size_bytes = $olderBytes.Length })
    if ($ThreeArchives.IsPresent) {
        $archiveEntries += [ordered]@{ archive = $secondHistoricalName; sha256 = $secondHistoricalHash; size_bytes = $secondHistoricalBytes.Length }
    }
    $archiveEntries += [ordered]@{ archive = $latestName; sha256 = $latestHash; size_bytes = $latestSize }
    $index = [ordered]@{ schema_version = 1; archives = $archiveEntries }
    $pointer = [ordered]@{ schema_version = 1; archive = $latestName; sha256 = $latestHash; size_bytes = $latestSize }
    if (-not $WithoutRecoveryKit.IsPresent) { $pointer.recovery_kit_included = $true }
    $index | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $remote 'index.json') -Encoding utf8
    $pointer | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $remote 'latest.json') -Encoding utf8

    $sshPath = Join-Path $root 'fake-ssh.ps1'
    @'
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CommandArgs)
$remoteCommand = $CommandArgs[-1]
if ($remoteCommand -eq 'cat /var/backups/vapor-server/latest.json') {
    if ($env:VAPOR_USB_TEST_BLOCK -eq '1') {
        Set-Content -LiteralPath $env:VAPOR_USB_TEST_READY -Value 'ready'
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        while (-not (Test-Path -LiteralPath $env:VAPOR_USB_TEST_RELEASE)) {
            if ([DateTime]::UtcNow -gt $deadline) { throw 'test ssh gate timed out' }
            Start-Sleep -Milliseconds 50
        }
    }
    Get-Content -LiteralPath (Join-Path $env:VAPOR_USB_TEST_REMOTE 'latest.json') -Raw
    $global:LASTEXITCODE = 0
} elseif ($remoteCommand -eq 'cat /var/backups/vapor-server/index.json') {
    Get-Content -LiteralPath (Join-Path $env:VAPOR_USB_TEST_REMOTE 'index.json') -Raw
    $global:LASTEXITCODE = 0
} else {
    [Console]::Error.WriteLine("unexpected fake ssh command: $remoteCommand")
    $global:LASTEXITCODE = 2
}
'@ | Set-Content -LiteralPath $sshPath -Encoding utf8

    $scpPath = Join-Path $root 'fake-scp.ps1'
    @'
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CommandArgs)
$remoteFile = $CommandArgs[-2]
$destination = $CommandArgs[-1]
$archive = [System.IO.Path]::GetFileName($remoteFile)
$source = Join-Path $env:VAPOR_USB_TEST_REMOTE $archive
Copy-Item -LiteralPath $source -Destination $destination -Force
$global:LASTEXITCODE = 0
'@ | Set-Content -LiteralPath $scpPath -Encoding utf8

    return [pscustomobject]@{
        Root = $root
        Remote = $remote
        Destination = $destination
        SshPath = $sshPath
        ScpPath = $scpPath
        OlderName = $olderName
        SecondHistoricalName = $secondHistoricalName
        LatestName = $latestName
        OlderBytes = $olderBytes
        SecondHistoricalBytes = $secondHistoricalBytes
        LatestBytes = $latestBytes
        OlderHash = $olderHash
        LatestHash = $latestHash
    }
}

function Invoke-Sync {
    param($Fixture, [long]$MaxStoredBytes = 100GB)
    $env:VAPOR_USB_TEST_REMOTE = $Fixture.Remote
    $env:VAPOR_USB_TEST_SSH_EXECUTABLE = $Fixture.SshPath
    $env:VAPOR_USB_TEST_SCP_EXECUTABLE = $Fixture.ScpPath
    & $scriptPath -Server 'example.test' -SshKey $PSCommandPath -UsbPath $Fixture.Destination -MaxStoredBytes $MaxStoredBytes -AllowFixedDrive
}

try {
    $fixture = New-Fixture -Name 'destination-junction'
    $outside = Join-Path $fixture.Root 'redirected-destination'
    $redirect = Join-Path $fixture.Root 'usb-junction'
    New-Item -ItemType Directory -Path $outside | Out-Null
    New-Item -ItemType Junction -Path $redirect -Target $outside | Out-Null
    $fixture.Destination = $redirect
    $rejected = $false
    try { Invoke-Sync $fixture | Out-Null } catch { $rejected = $_.Exception.Message -like '*reparse point*' }
    Assert-True $rejected 'a USB destination routed through a junction must be rejected'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $outside $fixture.LatestName))) 'destination junction wrote a recovery archive outside the USB path'

    $fixture = New-Fixture -Name 'incoming-junction'
    $outside = Join-Path $fixture.Root 'redirected-incoming'
    $incoming = Join-Path $fixture.Destination '.incoming'
    New-Item -ItemType Directory -Path $outside | Out-Null
    $sentinelPartial = Join-Path $outside 'outside.partial'
    Set-Content -LiteralPath $sentinelPartial -Value 'must remain untouched'
    New-Item -ItemType Junction -Path $incoming -Target $outside | Out-Null
    $rejected = $false
    try { Invoke-Sync $fixture | Out-Null } catch { $rejected = $_.Exception.Message -like '*reparse point*' }
    Assert-True $rejected 'a reparse-point .incoming directory must be rejected'
    Assert-True (Test-Path -LiteralPath $sentinelPartial) 'incoming junction cleanup deleted a file outside the USB path'

    $fixture = New-Fixture -Name 'nested-incoming-junction'
    $outside = Join-Path $fixture.Root 'redirected-nested-incoming'
    $partialDirectory = Join-Path $fixture.Destination '.incoming\stale.partial'
    New-Item -ItemType Directory -Path $outside, $partialDirectory | Out-Null
    $sentinelPartial = Join-Path $outside 'outside.partial'
    Set-Content -LiteralPath $sentinelPartial -Value 'must remain untouched'
    New-Item -ItemType Junction -Path (Join-Path $partialDirectory 'outside') -Target $outside | Out-Null
    $rejected = $false
    try { Invoke-Sync $fixture | Out-Null } catch { $rejected = $_.Exception.Message -like '*reparse point*' }
    Assert-True $rejected 'a nested partial-directory reparse point must be rejected before cleanup'
    Assert-True (Test-Path -LiteralPath $sentinelPartial) 'nested incoming junction cleanup modified a file outside the USB path'

    $fixture = New-Fixture -Name 'successful-copy'
    $incoming = Join-Path $fixture.Destination '.incoming'
    New-Item -ItemType Directory -Path $incoming | Out-Null
    Set-Content -LiteralPath (Join-Path $incoming 'old.partial') -Value 'stale'
    Invoke-Sync $fixture | Out-Null
    $latestPath = Join-Path $fixture.Destination $fixture.LatestName
    Assert-True (Test-Path -LiteralPath $latestPath -PathType Leaf) 'latest archive was not installed'
    Assert-True ((Get-FileHash -LiteralPath $latestPath -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $fixture.LatestHash) 'latest archive hash differs'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $incoming 'old.partial'))) 'stale partial file was not cleaned'
    $report = Get-Content -LiteralPath (Join-Path $fixture.Destination 'last-sync.json') -Raw | ConvertFrom-Json
    Assert-True ($report.copied_this_run -eq 2) 'first run should copy both indexed archives'
    Assert-True ($report.retained_archive_payload_bytes -eq ($fixture.OlderBytes.Length + $fixture.LatestBytes.Length)) 'sync report archive-payload byte count is inaccurate'
    Assert-True ($null -eq $report.PSObject.Properties['retained_bytes']) 'ambiguous total-byte field remains in sync report'
    $kitCommit = '0123456789abcdef0123456789abcdef01234567'
    $kitRoot = Join-Path $fixture.Destination (Join-Path 'Recovery-Kit' $kitCommit)
    Assert-True (Test-Path -LiteralPath (Join-Path $kitRoot 'recover-fresh-host.sh') -PathType Leaf) 'fresh-host recovery script was not extracted'
    Assert-True (Test-Path -LiteralPath (Join-Path $kitRoot 'recovery_bundle_metadata.py') -PathType Leaf) 'matching metadata helper was not extracted'
    Assert-True (Test-Path -LiteralPath (Join-Path $kitRoot 'state-bundle-validate.py') -PathType Leaf) 'matching archive validator was not extracted'
    Assert-True (Test-Path -LiteralPath (Join-Path $kitRoot 'deployed_source_manifest.py') -PathType Leaf) 'matching source-manifest validator dependency was not extracted'
    Assert-True (Test-Path -LiteralPath (Join-Path $kitRoot 'service_deploy_map.py') -PathType Leaf) 'matching deploy-map import dependency was not extracted'
    Assert-True (Test-Path -LiteralPath (Join-Path $kitRoot 'README.txt') -PathType Leaf) 'recovery instructions were not extracted'
    Assert-True (Test-Path -LiteralPath (Join-Path $kitRoot 'SHA256SUMS') -PathType Leaf) 'recovery-kit checksum file was not extracted'
    $kitPointer = Get-Content -LiteralPath (Join-Path $fixture.Destination 'Recovery-Kit\latest.json') -Raw | ConvertFrom-Json
    Assert-True ($kitPointer.source_commit -ceq $kitCommit) 'recovery kit pointer does not match the archive source revision'
    Assert-True ($kitPointer.archive -ceq $fixture.LatestName) 'recovery kit pointer does not identify the latest archive'
    Assert-True ($kitPointer.files.'recover-fresh-host.sh' -eq (Get-FileHash -LiteralPath (Join-Path $kitRoot 'recover-fresh-host.sh') -Algorithm SHA256).Hash.ToLowerInvariant()) 'recovery kit pointer does not bind the launcher hash'

    $fixture = New-Fixture -Name 'legacy-kit' -LegacyKit
    Invoke-Sync $fixture | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $fixture.Destination 'last-sync.json') -Raw | ConvertFrom-Json
    $kitPointer = Get-Content -LiteralPath (Join-Path $fixture.Destination 'Recovery-Kit\latest.json') -Raw | ConvertFrom-Json
    Assert-True ($report.recovery_kit_available) 'a valid previous recovery kit was not retained as usable'
    Assert-True (-not $report.recovery_kit_preflight_validation) 'previous recovery kit falsely reports preflight validation support'
    Assert-True (-not $kitPointer.preflight_validation) 'previous recovery kit pointer falsely reports preflight validation support'

    $fixture = New-Fixture -Name 'v2-kit' -V2Kit
    Invoke-Sync $fixture | Out-Null
    $v2KitRoot = Join-Path $fixture.Destination (Join-Path 'Recovery-Kit' '0123456789abcdef0123456789abcdef01234567')
    Assert-True (Test-Path -LiteralPath (Join-Path $v2KitRoot 'deployed_source_manifest.py') -PathType Leaf) 'valid v2 recovery kit was not accepted'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $v2KitRoot 'service_deploy_map.py'))) 'v2 kit was falsely treated as v3'

    Invoke-Sync $fixture | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $fixture.Destination 'last-sync.json') -Raw | ConvertFrom-Json
    Assert-True ($report.copied_this_run -eq 0) 'second run should only verify existing archives'

    $fixture = New-Fixture -Name 'full-historical-audit' -ThreeArchives
    Invoke-Sync $fixture | Out-Null
    $damagedHistoricalPath = Join-Path $fixture.Destination $fixture.OlderName
    $sameLengthDamage = [Security.Cryptography.RandomNumberGenerator]::GetBytes($fixture.OlderBytes.Length)
    [System.IO.File]::WriteAllBytes($damagedHistoricalPath, $sameLengthDamage)
    [ordered]@{
        schema_version = 1
        next_archive = $fixture.SecondHistoricalName
        last_rehashed_archive = $fixture.OlderName
        last_rehashed_utc = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    } | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $fixture.Destination '.vapor-usb-audit.json') -Encoding utf8
    Invoke-Sync $fixture | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $fixture.Destination 'last-sync.json') -Raw | ConvertFrom-Json
    $repairedHash = (Get-FileHash -LiteralPath $damagedHistoricalPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-True ($report.copied_this_run -eq 1) 'normal sync must repair a damaged historical archive, even when its matching sidecar is intact'
    Assert-True ($repairedHash -ceq $fixture.OlderHash) 'normal sync did not restore the historical archive from its indexed VPS copy'
    Assert-True ($report.hash_checks_this_run -eq 4) 'normal sync must hash every archive and verify the repaired download'
    Assert-True ($report.content_verified_this_run -eq 3) 'normal sync did not content-verify every indexed archive'
    Assert-True ($report.unverified_archives -eq 0) 'normal sync left indexed archives unverified'
    Assert-True ($report.verification_policy -ceq 'all indexed archives rehashed on every sync; pruning requires full verification') 'sync report does not describe the full verification policy'
    Assert-True ($null -eq $report.PSObject.Properties['rotating_audit_archive']) 'obsolete rotating-audit report field remains'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.Destination '.vapor-usb-audit.json'))) 'obsolete rotating-audit state was not retired after successful sync'

    $legacyPointer = [ordered]@{
        schema_version = 1
        archive = $fixture.OlderName
        sha256 = $fixture.OlderHash
        size_bytes = $fixture.OlderBytes.Length
    }
    $legacyPointer | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $fixture.Remote 'latest.json') -Encoding utf8
    Invoke-Sync $fixture | Out-Null
    $kitPointer = Get-Content -LiteralPath (Join-Path $fixture.Destination 'Recovery-Kit\latest.json') -Raw | ConvertFrom-Json
    Assert-True (-not $kitPointer.available) 'legacy sync did not clear the stale recovery-kit ready state'
    Assert-True ($kitPointer.archive -ceq $fixture.OlderName) 'legacy recovery-kit status points at the prior source archive'
    $report = Get-Content -LiteralPath (Join-Path $fixture.Destination 'last-sync.json') -Raw | ConvertFrom-Json
    Assert-True (-not $report.recovery_kit_available) 'legacy transition still reports a usable recovery kit'

    $fixture = New-Fixture -Name 'corrupt-kit' -CorruptKit
    $rejected = $false
    try { Invoke-Sync $fixture | Out-Null } catch { $rejected = $_.Exception.Message -like '*embedded checksum*' }
    Assert-True $rejected 'a recovery kit with an invalid embedded checksum must be rejected'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.Destination 'Recovery-Kit\latest.json'))) 'invalid kit was published as current'

    $fixture = New-Fixture -Name 'partial-kit' -IncompleteKit
    $rejected = $false
    $kitFailure = ''
    try { Invoke-Sync $fixture | Out-Null } catch { $kitFailure = $_.Exception.Message; $rejected = $kitFailure -like '*declares a recovery kit*' }
    Assert-True $rejected "a partially present kit declared by the archive pointer must fail sync ($kitFailure)"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.Destination 'last-sync.json'))) 'partial kit produced a successful sync report'

    $fixture = New-Fixture -Name 'truncated-kit-archive' -TruncateLatestArchive
    $rejected = $false
    $kitFailure = ''
    try { Invoke-Sync $fixture | Out-Null } catch { $kitFailure = $_.Exception.Message; $rejected = $kitFailure -like '*declares a recovery kit*' }
    Assert-True $rejected "a truncated archive declared to contain a kit must fail sync ($kitFailure)"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.Destination 'last-sync.json'))) 'truncated archive produced a successful sync report'

    $fixture = New-Fixture -Name 'legacy-without-kit' -WithoutRecoveryKit
    Invoke-Sync $fixture | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $fixture.Destination 'last-sync.json') -Raw | ConvertFrom-Json
    Assert-True (-not $report.recovery_kit_available) 'legacy archive without an embedded kit was reported as kit-ready'
    $kitPointer = Get-Content -LiteralPath (Join-Path $fixture.Destination 'Recovery-Kit\latest.json') -Raw | ConvertFrom-Json
    Assert-True (-not $kitPointer.available) 'legacy archive without an embedded kit published a ready pointer'

    $fixture = New-Fixture -Name 'retention-prunes-oldest-verified' -LargeArchives
    $retentionBudget = [long][Math]::Max(1MB, $fixture.LatestBytes.Length + [Math]::Floor($fixture.OlderBytes.Length / 2))
    Assert-True ($retentionBudget -lt ($fixture.LatestBytes.Length + $fixture.OlderBytes.Length)) 'retention fixture budget must require pruning'
    Invoke-Sync $fixture -MaxStoredBytes $retentionBudget | Out-Null
    $latestPath = Join-Path $fixture.Destination $fixture.LatestName
    Assert-True (Test-Path -LiteralPath $latestPath -PathType Leaf) 'retention removed the current archive'
    Assert-True ((Get-FileHash -LiteralPath $latestPath -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $fixture.LatestHash) 'retained latest archive is not content-verified'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.Destination $fixture.OlderName))) 'retention did not prune the oldest verified archive'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.Destination ($fixture.OlderName + '.sha256')))) 'retention left a detached checksum for a pruned archive'

    $fixture = New-Fixture -Name 'retention-preserves-unindexed' -LargeArchives
    $manualName = 'vapor-platform-recovery-20261105T120000Z-333333333333.tar.gz'
    $manualPath = Join-Path $fixture.Destination $manualName
    $manualBytes = [Security.Cryptography.RandomNumberGenerator]::GetBytes(1MB)
    [System.IO.File]::WriteAllBytes($manualPath, $manualBytes)
    $retentionBudget = [long][Math]::Max(1MB, $fixture.LatestBytes.Length + 1)
    $manualHash = (Get-FileHash -LiteralPath $manualPath -Algorithm SHA256).Hash
    $rejected = $false
    try { Invoke-Sync $fixture -MaxStoredBytes $retentionBudget | Out-Null } catch { $rejected = $_.Exception.Message -like '*latest archive plus unverified USB files exceed the storage budget*' }
    Assert-True $rejected 'retention must refuse when the latest and unindexed archive exceed the budget'
    Assert-True (Test-Path -LiteralPath $manualPath -PathType Leaf) 'retention deleted an unindexed/manual archive'
    Assert-True ((Get-FileHash -LiteralPath $manualPath -Algorithm SHA256).Hash -ceq $manualHash) 'retention modified an unindexed/manual archive'
    Assert-True (Test-Path -LiteralPath (Join-Path $fixture.Destination $fixture.OlderName) -PathType Leaf) 'refused retention deleted an older verified archive'

    $fixture = New-Fixture -Name 'invalid-pointer'
    $badPointer = [ordered]@{ schema_version = 1; archive = $fixture.LatestName; sha256 = ('0' * 64); size_bytes = 1 }
    $badPointer | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $fixture.Remote 'latest.json') -Encoding utf8
    $rejected = $false
    try { Invoke-Sync $fixture | Out-Null } catch { $rejected = $_.Exception.Message -like '*does not match an indexed archive*' }
    Assert-True $rejected 'an index-mismatched latest pointer must be rejected'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.Destination $fixture.LatestName))) 'invalid pointer downloaded an archive'

    $fixture = New-Fixture -Name 'bad-download' -CorruptLatest
    [System.IO.File]::WriteAllBytes((Join-Path $fixture.Destination $fixture.OlderName), $fixture.OlderBytes)
    Set-Content -LiteralPath (Join-Path $fixture.Destination ($fixture.OlderName + '.sha256')) `
        -Value "$($fixture.OlderHash)  $($fixture.OlderName)" -Encoding ascii
    $rejected = $false
    try { Invoke-Sync $fixture | Out-Null } catch { $rejected = $_.Exception.Message -like '*failed size or hash verification*' }
    Assert-True $rejected 'a corrupt archive download must be rejected'
    Assert-True (Test-Path -LiteralPath (Join-Path $fixture.Destination $fixture.OlderName)) 'failed download removed the older good archive'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.Destination $fixture.LatestName))) 'corrupt download became the latest archive'

    $fixture = New-Fixture -Name 'overlapping-sync'
    $incoming = Join-Path $fixture.Destination '.incoming'
    New-Item -ItemType Directory -Path $incoming | Out-Null
    $ready = Join-Path $fixture.Root 'ssh-ready'
    $release = Join-Path $fixture.Root 'ssh-release'
    $pwsh = (Get-Command pwsh -ErrorAction Stop).Source
    $processInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $processInfo.FileName = $pwsh
    $processInfo.UseShellExecute = $false
    $processInfo.CreateNoWindow = $true
    $processInfo.RedirectStandardError = $true
    $processInfo.RedirectStandardOutput = $true
    foreach ($argument in @(
        '-NoLogo', '-NoProfile', '-File', $scriptPath,
        '-Server', 'example.test', '-SshKey', $PSCommandPath,
        '-UsbPath', $fixture.Destination, '-AllowFixedDrive'
    )) { $processInfo.ArgumentList.Add($argument) }
    $processInfo.Environment['VAPOR_USB_TEST_REMOTE'] = $fixture.Remote
    $processInfo.Environment['VAPOR_USB_TEST_SSH_EXECUTABLE'] = $fixture.SshPath
    $processInfo.Environment['VAPOR_USB_TEST_SCP_EXECUTABLE'] = $fixture.ScpPath
    $processInfo.Environment['VAPOR_USB_TEST_BLOCK'] = '1'
    $processInfo.Environment['VAPOR_USB_TEST_READY'] = $ready
    $processInfo.Environment['VAPOR_USB_TEST_RELEASE'] = $release
    $process = [System.Diagnostics.Process]::Start($processInfo)
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (-not (Test-Path -LiteralPath $ready) -and [DateTime]::UtcNow -lt $deadline) {
            Start-Sleep -Milliseconds 50
        }
        Assert-True (Test-Path -LiteralPath $ready) 'first sync did not reach the blocked SSH call'
        $sentinelPartial = Join-Path $incoming 'active-transfer.partial'
        Set-Content -LiteralPath $sentinelPartial -Value 'owned by active sync'
        $overlapRejected = $false
        try { Invoke-Sync $fixture | Out-Null } catch { $overlapRejected = $_.Exception.Message -like '*already running for this destination*' }
        Assert-True $overlapRejected 'a second sync was not rejected while the first owned the destination lock'
        Assert-True (Test-Path -LiteralPath $sentinelPartial) 'overlapping sync deleted an active partial transfer'
        Set-Content -LiteralPath $release -Value 'release'
        if (-not $process.WaitForExit(20000)) { throw 'blocked sync process failed to exit' }
        $firstOutput = $process.StandardOutput.ReadToEnd()
        $firstError = $process.StandardError.ReadToEnd()
        Assert-True ($process.ExitCode -eq 0) "first sync failed (exit $($process.ExitCode)): $firstError`n$firstOutput"
    } finally {
        Set-Content -LiteralPath $release -Value 'release'
        if (-not $process.WaitForExit(20000)) { $process.Kill(); throw 'blocked sync process failed to exit' }
        $process.Dispose()
        Remove-Item Env:\VAPOR_USB_TEST_BLOCK, Env:\VAPOR_USB_TEST_READY, Env:\VAPOR_USB_TEST_RELEASE `
            -ErrorAction SilentlyContinue
    }

    Write-Output 'USB recovery sync tests passed.'
} finally {
    Remove-Item Env:\VAPOR_USB_TEST_REMOTE -ErrorAction SilentlyContinue
    Remove-Item Env:\VAPOR_USB_TEST_SSH_EXECUTABLE, Env:\VAPOR_USB_TEST_SCP_EXECUTABLE -ErrorAction SilentlyContinue
    $resolvedScratch = [System.IO.Path]::GetFullPath($scratch)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $resolvedScratch.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove test scratch outside temp root: $resolvedScratch"
    }
    Remove-Item -LiteralPath $resolvedScratch -Recurse -Force
}
