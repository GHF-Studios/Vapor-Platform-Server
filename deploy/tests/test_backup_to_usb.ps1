$ErrorActionPreference = 'Stop'

$scriptPath = Join-Path $PSScriptRoot '..\scripts\backup-to-usb.ps1'
$scriptPath = (Resolve-Path -LiteralPath $scriptPath).Path
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("vapor-usb-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw "ASSERT: $Message" }
}

function New-Fixture {
    param([string]$Name, [switch]$CorruptLatest)

    $root = Join-Path $scratch $Name
    $remote = Join-Path $root 'remote'
    $destination = Join-Path $root 'usb'
    New-Item -ItemType Directory -Path $remote, $destination | Out-Null

    $olderName = 'vapor-platform-recovery-20261106T120000Z-111111111111.tar.gz'
    $latestName = 'vapor-platform-recovery-20261107T120000Z-222222222222.tar.gz'
    $olderBytes = [System.Text.Encoding]::UTF8.GetBytes("known-good-$Name")
    $latestBytes = [System.Text.Encoding]::UTF8.GetBytes("latest-good-$Name")
    $wireBytes = if ($CorruptLatest.IsPresent) { [System.Text.Encoding]::UTF8.GetBytes("corrupt-$Name") } else { $latestBytes }
    [System.IO.File]::WriteAllBytes((Join-Path $remote $olderName), $olderBytes)
    [System.IO.File]::WriteAllBytes((Join-Path $remote $latestName), $wireBytes)
    $olderHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($olderBytes)).ToLowerInvariant()
    $latestHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($latestBytes)).ToLowerInvariant()
    $latestSize = $latestBytes.Length
    $index = [ordered]@{ schema_version = 1; archives = @(
        [ordered]@{ archive = $olderName; sha256 = $olderHash; size_bytes = $olderBytes.Length },
        [ordered]@{ archive = $latestName; sha256 = $latestHash; size_bytes = $latestSize }
    ) }
    $pointer = [ordered]@{ schema_version = 1; archive = $latestName; sha256 = $latestHash; size_bytes = $latestSize }
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
        LatestName = $latestName
        OlderBytes = $olderBytes
        LatestBytes = $latestBytes
        OlderHash = $olderHash
        LatestHash = $latestHash
    }
}

function Invoke-Sync {
    param($Fixture)
    $env:VAPOR_USB_TEST_REMOTE = $Fixture.Remote
    $env:VAPOR_USB_TEST_SSH_EXECUTABLE = $Fixture.SshPath
    $env:VAPOR_USB_TEST_SCP_EXECUTABLE = $Fixture.ScpPath
    & $scriptPath -Server 'example.test' -SshKey $PSCommandPath -UsbPath $Fixture.Destination -AllowFixedDrive
}

try {
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

    Invoke-Sync $fixture | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $fixture.Destination 'last-sync.json') -Raw | ConvertFrom-Json
    Assert-True ($report.copied_this_run -eq 0) 'second run should only verify existing archives'

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

    Write-Output 'USB recovery sync tests passed (4 scenarios).'
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
