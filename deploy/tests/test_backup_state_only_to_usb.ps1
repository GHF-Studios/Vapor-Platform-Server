$ErrorActionPreference = 'Stop'
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "vapor-state-usb-$([guid]::NewGuid().ToString('N'))"
$usbRoot = Join-Path $testRoot 'usb'
$remoteRoot = Join-Path $testRoot 'remote'
$sourceRoot = Join-Path $testRoot 'source'
New-Item -ItemType Directory -Path $remoteRoot, $sourceRoot | Out-Null

function Assert-True([bool]$Value, [string]$Message) {
    if (-not $Value) { throw $Message }
}

try {
    $git = (Get-Command git.exe -CommandType Application | Select-Object -First 1).Source
    $python = (Get-Command python.exe -CommandType Application | Select-Object -First 1).Source
    $sshShim = Join-Path $testRoot 'ssh-shim.ps1'
    $scpShim = Join-Path $testRoot 'scp-shim.ps1'
    @'
param([Parameter(ValueFromRemainingArguments=$true)][string[]]$CallArgs)
$command = $CallArgs[-1]
if ($command -like 'cat */latest.json') { Get-Content -LiteralPath (Join-Path $env:VAPOR_USB_TEST_REMOTE_ROOT 'latest.json') -Raw; exit 0 }
if ($command -like 'cat */index.json') { Get-Content -LiteralPath (Join-Path $env:VAPOR_USB_TEST_REMOTE_ROOT 'index.json') -Raw; exit 0 }
if ($command -match '^python3 /opt/vapor-platform-server/deploy/scripts/record_owner_state_usb_sync\.py --base64 ([A-Za-z0-9+/=]+)$') {
    $bytes = [Convert]::FromBase64String($Matches[1])
    $json = [System.Text.Encoding]::UTF8.GetString($bytes)
    Set-Content -LiteralPath (Join-Path $env:VAPOR_USB_TEST_REMOTE_ROOT 'owner-usb-sync.json') -Value $json -NoNewline -Encoding utf8
    exit 0
}
exit 2
'@ | Set-Content -LiteralPath $sshShim -Encoding utf8
    @'
param([Parameter(ValueFromRemainingArguments=$true)][string[]]$CallArgs)
$remote = $CallArgs[-2]
$destination = $CallArgs[-1]
$name = Split-Path ($remote.Split(':', 2)[1]) -Leaf
Copy-Item -LiteralPath (Join-Path $env:VAPOR_USB_TEST_REMOTE_ROOT $name) -Destination $destination -Force
exit 0
'@ | Set-Content -LiteralPath $scpShim -Encoding utf8

    & $git -C $sourceRoot init --quiet --initial-branch=main
    & $git -C $sourceRoot config user.name 'Vapor USB fixture'
    & $git -C $sourceRoot config user.email 'fixture@example.invalid'
    $kitFiles = @(
        'recover-state-only-host.sh', 'state_only_recovery_marker.py', 'restore-state-only.sh', 'recovery_bundle_metadata.py',
        'state-bundle-validate.py', 'deployed_source_manifest.py', 'service_deploy_map.py',
        'validate-state-databases.py', 'README-state-only-recovery.txt'
    )
    $scriptFolder = Join-Path $sourceRoot 'deploy\scripts'
    New-Item -ItemType Directory -Path $scriptFolder | Out-Null
    foreach ($name in $kitFiles) { Set-Content -LiteralPath (Join-Path $scriptFolder $name) -Value "fixture $name" -Encoding utf8 }
    Set-Content -LiteralPath (Join-Path $scriptFolder 'state_only_recovery_marker.py') -Value 'fixture recovery kit from archived revision' -Encoding utf8
    & $git -C $sourceRoot add deploy/scripts
    & $git -C $sourceRoot commit --quiet -m 'Fixture archived application revision'
    $archiveSourceCommit = (& $git -C $sourceRoot rev-parse HEAD).Trim()
    Set-Content -LiteralPath (Join-Path $scriptFolder 'state_only_recovery_marker.py') -Value 'fixture recovery kit from current main' -Encoding utf8
    & $git -C $sourceRoot add deploy/scripts/state_only_recovery_marker.py
    & $git -C $sourceRoot commit --quiet -m 'Fixture current recovery-kit revision'
    $recoveryKitCommit = (& $git -C $sourceRoot rev-parse HEAD).Trim()
    $repositoryCache = Join-Path $testRoot 'Vapor-Platform-Server.git'
    & $git clone --quiet --bare $sourceRoot $repositoryCache

    $archiveName = 'vapor-platform-state-20261108T000000Z-0123456789ab.tar.gz'
    $archivePath = Join-Path $remoteRoot $archiveName
    $manifestPath = Join-Path $testRoot 'manifest.toml'
    @"
schema_version = 4
created_at_utc = "2026-11-08T00:00:00Z"
platform_source_commit = "$archiveSourceCommit"
"@ | Set-Content -LiteralPath $manifestPath -Encoding ascii
    $makeArchive = Join-Path $testRoot 'make-archive.py'
    @'
import pathlib, sys, tarfile
root = pathlib.Path(sys.argv[1])
manifest = pathlib.Path(sys.argv[2])
with tarfile.open(root / sys.argv[3], "w:gz") as archive:
    for name, content in [
        ("vapor-platform-recovery", None),
        ("vapor-platform-recovery/manifest.toml", manifest.read_bytes()),
    ]:
        info = tarfile.TarInfo(name)
        if content is None:
            info.type = tarfile.DIRTYPE
            info.mode = 0o700
            archive.addfile(info)
        else:
            info.size = len(content)
            archive.addfile(info, __import__("io").BytesIO(content))
'@ | Set-Content -LiteralPath $makeArchive -Encoding utf8
    & $python $makeArchive $remoteRoot $manifestPath $archiveName
    $archiveHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    $archiveSize = (Get-Item -LiteralPath $archivePath).Length
    [ordered]@{ schema_version = 1; archive_schema_version = 4; archive = $archiveName; sha256 = $archiveHash; size_bytes = $archiveSize } |
        ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $remoteRoot 'latest.json') -Encoding utf8
    [ordered]@{ schema_version = 1; archives = @([ordered]@{ archive = $archiveName; sha256 = $archiveHash; size_bytes = $archiveSize }) } |
        ConvertTo-Json -Depth 4 -Compress | Set-Content -LiteralPath (Join-Path $remoteRoot 'index.json') -Encoding utf8
    $sshKey = Join-Path $testRoot 'key'
    Set-Content -LiteralPath $sshKey -Value 'fixture' -Encoding ascii

    $env:VAPOR_USB_TEST_REMOTE_ROOT = $remoteRoot
    $env:VAPOR_USB_TEST_SSH_EXECUTABLE = $sshShim
    $env:VAPOR_USB_TEST_SCP_EXECUTABLE = $scpShim
    $sync = Join-Path $PSScriptRoot '..\scripts\backup-state-only-to-usb.ps1'
    & $sync -Server fixture -SshKey $sshKey -UsbPath $usbRoot -AllowFixedDrive -RecoveryRepositoryPath $repositoryCache | Out-Null

    $snapshot = Join-Path $usbRoot (Join-Path 'State-Snapshots' $archiveName)
    Assert-True (Test-Path -LiteralPath $snapshot -PathType Leaf) 'verified state archive was not copied'
    $kitRoot = Join-Path $usbRoot (Join-Path 'Recovery-Kit\state-only' $recoveryKitCommit)
    foreach ($name in ($kitFiles + 'SHA256SUMS')) {
        Assert-True (Test-Path -LiteralPath (Join-Path $kitRoot $name) -PathType Leaf) "recovery kit is missing $name"
    }
    $pointer = Get-Content -LiteralPath (Join-Path $usbRoot 'Recovery-Kit\state-only\latest.json') -Raw | ConvertFrom-Json
    Assert-True ($pointer.recovery_kit_commit -ceq $recoveryKitCommit) 'recovery-kit pointer has the wrong kit revision'
    Assert-True ($pointer.archive_source_commit -ceq $archiveSourceCommit) 'recovery-kit pointer has the wrong archived application revision'
    Assert-True ($pointer.archive_sha256 -ceq $archiveHash) 'recovery-kit pointer does not bind the snapshot checksum'
    Assert-True ($pointer.files.'recover-state-only-host.sh' -ceq (Get-FileHash -LiteralPath (Join-Path $kitRoot 'recover-state-only-host.sh') -Algorithm SHA256).Hash.ToLowerInvariant()) 'recovery-kit file digest is incorrect'
    $syncReport = Get-Content -LiteralPath (Join-Path $usbRoot 'State-Snapshots\last-sync.json') -Raw | ConvertFrom-Json
    Assert-True ($syncReport.platform_reported -eq $true) 'local sync report does not show the Platform status update'
    $serverReport = Get-Content -LiteralPath (Join-Path $remoteRoot 'owner-usb-sync.json') -Raw | ConvertFrom-Json
    Assert-True ($serverReport.source_commit -ceq $recoveryKitCommit) 'Platform report has the wrong source-pinned recovery-kit revision'
    Assert-True ($serverReport.latest_archive_sha256 -ceq $archiveHash) 'Platform report does not bind the latest archive checksum'

    & $sync -Server fixture -SshKey $sshKey -UsbPath $usbRoot -AllowFixedDrive -RecoveryRepositoryPath $repositoryCache | Out-Null
    $damagePath = Join-Path $kitRoot 'recover-state-only-host.sh'
    Set-Content -LiteralPath $damagePath -Value 'tampered' -Encoding utf8
    $rejected = $false
    try { & $sync -Server fixture -SshKey $sshKey -UsbPath $usbRoot -AllowFixedDrive -RecoveryRepositoryPath $repositoryCache | Out-Null }
    catch { $rejected = $_.Exception.Message -like '*checksum file does not match*' }
    Assert-True $rejected 'sync must reject a modified source-pinned recovery kit'
    Assert-True ((Get-Content -LiteralPath (Join-Path $kitRoot 'state_only_recovery_marker.py') -Raw).Trim() -ceq 'fixture recovery kit from current main') 'recovery kit was incorrectly taken from the archived application revision'
    Write-Output 'state-only USB recovery-kit tests passed'
} finally {
    Remove-Item Env:\VAPOR_USB_TEST_REMOTE_ROOT -ErrorAction SilentlyContinue
    Remove-Item Env:\VAPOR_USB_TEST_SSH_EXECUTABLE -ErrorAction SilentlyContinue
    Remove-Item Env:\VAPOR_USB_TEST_SCP_EXECUTABLE -ErrorAction SilentlyContinue
    $fullTestRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $fullTestRoot.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing test cleanup outside the temporary directory.'
    }
    Remove-Item -LiteralPath $fullTestRoot -Recurse -Force -ErrorAction SilentlyContinue
}
