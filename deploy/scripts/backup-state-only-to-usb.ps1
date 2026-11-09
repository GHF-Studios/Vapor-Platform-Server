[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9.-]+$')]
    [string]$Server,

    [Parameter(Mandatory = $true)]
    [string]$SshKey,

    [Parameter(Mandatory = $true)]
    [string]$UsbPath,

    [ValidateRange(1MB, 120GB)]
    [long]$MaxStoredBytes = 100GB,

    [switch]$AllowFixedDrive,

    [string]$RecoveryRepositoryPath = (Join-Path $PSScriptRoot '..\..')
)

$ErrorActionPreference = 'Stop'
$remoteRoot = '/var/backups/vapor-server/state-only'
$target = "root@$Server"
$sshCommand = if ([string]::IsNullOrWhiteSpace($env:VAPOR_USB_TEST_SSH_EXECUTABLE)) {
    Get-Command ssh.exe -CommandType Application -ErrorAction Stop
} else {
    Get-Command -Name $env:VAPOR_USB_TEST_SSH_EXECUTABLE -CommandType Application, ExternalScript -ErrorAction Stop
}
$scpCommand = if ([string]::IsNullOrWhiteSpace($env:VAPOR_USB_TEST_SCP_EXECUTABLE)) {
    Get-Command scp.exe -CommandType Application -ErrorAction Stop
} else {
    Get-Command -Name $env:VAPOR_USB_TEST_SCP_EXECUTABLE -CommandType Application, ExternalScript -ErrorAction Stop
}
$ssh = $sshCommand.Source
$scp = $scpCommand.Source
$keyPath = (Resolve-Path -LiteralPath $SshKey).Path
$destination = [System.IO.Path]::GetFullPath($UsbPath)
if ($destination -notmatch '^[A-Za-z]:\\') { throw 'USB destination must be a local drive path.' }
function Assert-NoReparsePathComponents([string]$Path) {
    $full = [System.IO.Path]::GetFullPath($Path)
    $root = [System.IO.Path]::GetPathRoot($full)
    $current = $root
    foreach ($part in $full.Substring($root.Length).Split([char[]]@('\', '/'), [System.StringSplitOptions]::RemoveEmptyEntries)) {
        $current = Join-Path $current $part
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Path contains a reparse point: $current"
            }
        }
    }
}
$driveRoot = [System.IO.Path]::GetPathRoot($destination)
$drive = [System.IO.DriveInfo]::new($driveRoot)
if (-not $drive.IsReady) { throw 'USB destination drive is not ready.' }
if ($drive.DriveType -eq [System.IO.DriveType]::Fixed -and -not $AllowFixedDrive) {
    throw 'Destination is not reported as removable. Use -AllowFixedDrive only after confirming the USB drive.'
}
if ($drive.DriveType -notin @([System.IO.DriveType]::Removable, [System.IO.DriveType]::Fixed)) {
    throw 'USB destination must be on a local removable or fixed drive.'
}
Assert-NoReparsePathComponents $destination
New-Item -ItemType Directory -Path $destination -Force | Out-Null
Assert-NoReparsePathComponents $destination
$destination = (Resolve-Path -LiteralPath $destination).Path
$snapshotRoot = Join-Path $destination 'State-Snapshots'
$incoming = Join-Path $snapshotRoot '.incoming'
Assert-NoReparsePathComponents $snapshotRoot
Assert-NoReparsePathComponents $incoming
New-Item -ItemType Directory -Path $incoming -Force | Out-Null
Assert-NoReparsePathComponents $incoming

$sshOptions = @('-i', $keyPath, '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes', '-o', 'ConnectTimeout=10')
$latestText = & $ssh @sshOptions $target "cat $remoteRoot/latest.json"
if ($LASTEXITCODE -ne 0) { throw 'Could not read the state-only latest pointer from the VPS.' }
$indexText = & $ssh @sshOptions $target "cat $remoteRoot/index.json"
if ($LASTEXITCODE -ne 0) { throw 'Could not read the state-only archive index from the VPS.' }
try {
    $latest = ($latestText -join "`n") | ConvertFrom-Json
    $index = ($indexText -join "`n") | ConvertFrom-Json
} catch { throw 'The state-only archive index or latest pointer is invalid JSON.' }
if ($latest.schema_version -ne 1 -or $latest.archive_schema_version -notin @(3, 4) -or
    $latest.archive -notmatch '^vapor-platform-state-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{12}\.tar\.gz$' -or
    $latest.sha256 -notmatch '^[0-9a-f]{64}$' -or [long]$latest.size_bytes -le 0) {
    throw 'The latest state-only archive pointer has an unsupported schema.'
}
if ($index.schema_version -ne 1 -or @($index.archives).Count -lt 1) {
    throw 'The state-only archive index has an unsupported schema or no entries.'
}
$entries = @{}
foreach ($entry in $index.archives) {
    if ($entry.archive -notmatch '^vapor-platform-state-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{12}\.tar\.gz$' -or
        $entry.sha256 -notmatch '^[0-9a-f]{64}$' -or [long]$entry.size_bytes -le 0 -or
        $entries.ContainsKey([string]$entry.archive)) {
        throw 'The state-only index contains invalid or duplicate archive metadata.'
    }
    $entries[[string]$entry.archive] = $entry
}
$latestEntry = $entries[[string]$latest.archive]
if ($null -eq $latestEntry -or $latestEntry.sha256 -cne $latest.sha256 -or
    [long]$latestEntry.size_bytes -ne [long]$latest.size_bytes) {
    throw 'The latest pointer does not match the verified archive index.'
}

$expectedTotalBytes = 0L
foreach ($entry in $index.archives) { $expectedTotalBytes += [long]$entry.size_bytes }
if ($expectedTotalBytes -gt $MaxStoredBytes) { throw 'Indexed state snapshots exceed the configured USB storage limit.' }

$verifiedBytes = 0L
$copied = 0
foreach ($entry in $index.archives) {
    $name = [string]$entry.archive
    $finalPath = Join-Path $snapshotRoot $name
    $sidecarPath = "$finalPath.sha256"
    Assert-NoReparsePathComponents $finalPath
    Assert-NoReparsePathComponents $sidecarPath
    $validExisting = $false
    if ((Test-Path -LiteralPath $finalPath -PathType Leaf) -and (Test-Path -LiteralPath $sidecarPath -PathType Leaf)) {
        $file = Get-Item -LiteralPath $finalPath
        if ($file.Length -eq [long]$entry.size_bytes) {
            $hash = (Get-FileHash -LiteralPath $finalPath -Algorithm SHA256).Hash.ToLowerInvariant()
            $sidecar = (Get-Content -LiteralPath $sidecarPath -Raw).Trim()
            $validExisting = $hash -ceq [string]$entry.sha256 -and
                $sidecar -ceq "$hash  $name"
        }
    }
    if (-not $validExisting) {
        $partial = Join-Path $incoming "$name.partial"
        Remove-Item -LiteralPath $partial -Force -ErrorAction SilentlyContinue
        & $scp @sshOptions "$target`:$remoteRoot/$name" $partial
        if ($LASTEXITCODE -ne 0) { throw "Could not copy state archive $name from the VPS." }
        $downloaded = Get-Item -LiteralPath $partial
        $hash = (Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($downloaded.Length -ne [long]$entry.size_bytes -or $hash -cne [string]$entry.sha256) {
            Remove-Item -LiteralPath $partial -Force -ErrorAction SilentlyContinue
            throw "State archive verification failed for $name."
        }
        Move-Item -LiteralPath $partial -Destination $finalPath -Force
        Set-Content -LiteralPath $sidecarPath -Value "$hash  $name" -Encoding ascii
        $copied++
    }
    $verifiedBytes += [long]$entry.size_bytes
}
if ($verifiedBytes -ne $expectedTotalBytes) { throw 'Verified state snapshot accounting did not match the index.' }

$indexPath = Join-Path $snapshotRoot 'index.json'
$latestPath = Join-Path $snapshotRoot 'latest.json'
$indexTemp = Join-Path $incoming 'index.json.partial'
$latestTemp = Join-Path $incoming 'latest.json.partial'
foreach ($metadataPath in @($indexPath, $latestPath, $indexTemp, $latestTemp)) {
    Assert-NoReparsePathComponents $metadataPath
}
[System.IO.File]::WriteAllText($indexTemp, ($index | ConvertTo-Json -Depth 8 -Compress) + "`n", [System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText($latestTemp, ($latest | ConvertTo-Json -Depth 4 -Compress) + "`n", [System.Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $indexTemp -Destination $indexPath -Force
Move-Item -LiteralPath $latestTemp -Destination $latestPath -Force

# Keep the recovery launcher and its validators beside the state snapshots.
# The archived Platform revision rebuilds the application; the recovery kit
# must come from the current recovery-tool source, which can be newer.
$latestArchivePath = Join-Path $snapshotRoot ([string]$latest.archive)
$metadataHelper = Join-Path $PSScriptRoot 'recovery_bundle_metadata.py'
$python = (Get-Command python.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$archiveSourceCommit = & $python $metadataHelper --source-commit $latestArchivePath
if ($LASTEXITCODE -ne 0 -or $archiveSourceCommit -notmatch '^[0-9a-f]{40}$') {
    throw 'The latest state snapshot does not contain a valid Platform source revision.'
}
$kitFiles = @(
    'recover-state-only-host.sh',
    'state_only_recovery_marker.py',
    'restore-state-only.sh',
    'recovery_bundle_metadata.py',
    'state-bundle-validate.py',
    'deployed_source_manifest.py',
    'service_deploy_map.py',
    'validate-state-databases.py',
    'README-state-only-recovery.txt'
)
$archivePaths = @($kitFiles | ForEach-Object { "deploy/scripts/$_" })
$archiveAttributesPath = '.gitattributes'
$repositoryRoot = (Resolve-Path -LiteralPath $RecoveryRepositoryPath).Path
$git = (Get-Command git.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$isBare = (& $git -C $repositoryRoot rev-parse --is-bare-repository).Trim()
if ($LASTEXITCODE -ne 0 -or $isBare -notin @('true', 'false')) {
    throw 'The recovery-tool repository is not a valid Git repository.'
}
if ($isBare -cne 'true') {
    $dirty = & $git -C $repositoryRoot status --porcelain --untracked-files=all
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the recovery-tool checkout.' }
    if (-not [string]::IsNullOrWhiteSpace(($dirty -join "`n"))) {
        throw 'The recovery-tool checkout has local changes; use a clean checkout or bare repository cache.'
    }
}
# Always refresh canonical main. A clean local checkout can still contain
# unpushed commits, which a fresh recovery host cannot fetch from GitHub.
& $git -C $repositoryRoot fetch --no-tags origin main
if ($LASTEXITCODE -ne 0) { throw 'Could not update recovery tools from Platform main.' }
$sourceRef = 'FETCH_HEAD'
& $git -C $repositoryRoot merge-base --is-ancestor $archiveSourceCommit $sourceRef 2>$null
$ancestorExitCode = $LASTEXITCODE
# A failed ancestry predicate is an expected validation result for an
# unrecoverable archive, not a failed process. Clear the native exit code
# before throwing so callers that handle this validation error can continue
# and still finish successfully.
$global:LASTEXITCODE = 0
if ($ancestorExitCode -ne 0) {
    throw 'The snapshot source revision is not reachable from Platform origin/main; refusing to create an unrecoverable USB generation.'
}
$recoveryKitCommit = (& $git -C $repositoryRoot log -1 --format=%H $sourceRef -- @archivePaths $archiveAttributesPath).Trim()
if ($recoveryKitCommit -notmatch '^[0-9a-f]{40}$') {
    throw 'Could not identify the latest commit containing the recovery-tool files.'
}
& $git -C $repositoryRoot cat-file -e "$recoveryKitCommit^{commit}" 2>$null
if ($LASTEXITCODE -ne 0) {
    throw 'The selected recovery-tool revision is unavailable in the repository.'
}

$kitRoot = Join-Path $destination 'Recovery-Kit\state-only'
$kitFinal = Join-Path $kitRoot $recoveryKitCommit
$kitStage = Join-Path $incoming "kit-$recoveryKitCommit"
$kitZip = Join-Path $incoming "kit-$recoveryKitCommit.zip"
Assert-NoReparsePathComponents $kitRoot
New-Item -ItemType Directory -Path $kitRoot -Force | Out-Null
Assert-NoReparsePathComponents $kitRoot
Assert-NoReparsePathComponents $kitFinal
Assert-NoReparsePathComponents $kitStage
Assert-NoReparsePathComponents $kitZip
$kitHashes = [ordered]@{}
if (Test-Path -LiteralPath $kitFinal) {
    $expectedKitEntries = @($kitFiles + 'SHA256SUMS' | Sort-Object)
    $existingKitEntries = @(Get-ChildItem -LiteralPath $kitFinal -Force | Select-Object -ExpandProperty Name | Sort-Object)
    if (@($existingKitEntries) -join ',' -cne ($expectedKitEntries -join ',')) {
        throw 'Existing recovery-kit revision has unexpected files; refusing to replace it.'
    }
    foreach ($name in $kitFiles) {
        $path = Join-Path $kitFinal $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw 'Existing recovery-kit revision is incomplete.' }
        $kitHashes[$name] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $expectedChecksums = foreach ($name in ($kitFiles | Sort-Object)) { "$($kitHashes[$name])  $name" }
    $actualChecksums = (Get-Content -LiteralPath (Join-Path $kitFinal 'SHA256SUMS') -Raw).TrimEnd("`r", "`n") -split "`r?`n"
    if (@($actualChecksums) -join "`n" -cne (@($expectedChecksums) -join "`n")) {
        throw 'Existing recovery-kit checksum file does not match its contents.'
    }
} else {
    Remove-Item -LiteralPath $kitStage -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $kitZip -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $kitStage | Out-Null
    & $git -C $repositoryRoot archive --format=zip --output=$kitZip $recoveryKitCommit @archivePaths
    if ($LASTEXITCODE -ne 0) { throw 'Could not export the selected recovery kit.' }
    Expand-Archive -LiteralPath $kitZip -DestinationPath $kitStage
    $sourceScripts = Join-Path $kitStage 'deploy\scripts'
    foreach ($name in $kitFiles) {
        $source = Join-Path $sourceScripts $name
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
            throw "The selected recovery kit is missing $name."
        }
        Copy-Item -LiteralPath $source -Destination (Join-Path $kitStage $name)
    }
    Remove-Item -LiteralPath (Join-Path $kitStage 'deploy') -Recurse -Force
    $checksumLines = foreach ($name in ($kitFiles | Sort-Object)) {
        $path = Join-Path $kitStage $name
        $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        $kitHashes[$name] = $hash
        "$hash  $name"
    }
    [System.IO.File]::WriteAllText(
        (Join-Path $kitStage 'SHA256SUMS'),
        ($checksumLines -join "`n") + "`n",
        [System.Text.Encoding]::ASCII
    )
    Move-Item -LiteralPath $kitStage -Destination $kitFinal
    Remove-Item -LiteralPath $kitZip -Force
}

$kitPointerRoot = Join-Path $destination 'Recovery-Kit\state-only'
if (-not (Test-Path -LiteralPath $kitPointerRoot -PathType Container)) {
    New-Item -ItemType Directory -Path $kitPointerRoot -Force | Out-Null
}
Assert-NoReparsePathComponents $kitPointerRoot
$kitPointerTemp = Join-Path $incoming 'latest-state-kit.json.partial'
$kitPointer = [ordered]@{
    schema_version = 2
    available = $true
    recovery_kit_commit = $recoveryKitCommit
    archive_source_commit = $archiveSourceCommit
    archive = [string]$latest.archive
    archive_sha256 = [string]$latest.sha256
    files = $kitHashes
}
[System.IO.File]::WriteAllText(
    $kitPointerTemp,
    ($kitPointer | ConvertTo-Json -Depth 4 -Compress) + "`n",
    [System.Text.UTF8Encoding]::new($false)
)
Move-Item -LiteralPath $kitPointerTemp -Destination (Join-Path $kitPointerRoot 'latest.json') -Force
$lastSuccessUnix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$ownerReport = [ordered]@{
    schema_version = 1
    last_success_unix = $lastSuccessUnix
    latest_archive = [string]$latest.archive
    latest_archive_sha256 = [string]$latest.sha256
    verified_generation_count = [int]@($index.archives).Count
    retained_payload_bytes = [long]$verifiedBytes
    source_commit = [string]$recoveryKitCommit
    latest_rehashed = $true
    recovery_kit_available = $true
}
$reportJson = $ownerReport | ConvertTo-Json -Depth 4 -Compress
$reportEncoded = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($reportJson))
$platformReported = $false
try {
    $remoteReportCommand = "python3 /opt/vapor-platform-server/deploy/scripts/record_owner_state_usb_sync.py --base64 $reportEncoded"
    & $ssh @sshOptions $target $remoteReportCommand | Out-Null
    $platformReported = $LASTEXITCODE -eq 0
} catch {
    $platformReported = $false
}
$localReport = [ordered]@{
    schema_version = 1
    last_success_unix = $lastSuccessUnix
    latest_archive = [string]$latest.archive
    latest_archive_sha256 = [string]$latest.sha256
    verified_generation_count = [int]@($index.archives).Count
    retained_payload_bytes = [long]$verifiedBytes
    recovery_kit_commit = [string]$recoveryKitCommit
    archive_source_commit = [string]$archiveSourceCommit
    recovery_kit_available = $true
    platform_reported = $platformReported
}
$localReportPath = Join-Path $snapshotRoot 'last-sync.json'
$localReportTemp = Join-Path $incoming 'last-sync.json.partial'
Assert-NoReparsePathComponents $localReportPath
Assert-NoReparsePathComponents $localReportTemp
[System.IO.File]::WriteAllText($localReportTemp, ($localReport | ConvertTo-Json -Depth 4 -Compress) + "`n", [System.Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $localReportTemp -Destination $localReportPath -Force
if (-not $platformReported) {
    Write-Warning 'USB snapshot and recovery kit verified locally, but the server status report could not be updated.'
}
Write-Output "state-usb-sync: verified $(@($index.archives).Count) generations; copied $copied; $verifiedBytes bytes; recovery kit $recoveryKitCommit for archive source $archiveSourceCommit; platform report $(if ($platformReported) { 'updated' } else { 'unavailable' })"
