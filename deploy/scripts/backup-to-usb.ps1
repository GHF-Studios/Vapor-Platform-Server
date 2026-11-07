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

    [switch]$AllowFixedDrive
)

$ErrorActionPreference = 'Stop'
$remoteRoot = '/var/backups/vapor-server'
$remoteUser = 'root'
$target = "$remoteUser@$Server"
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
$tarCommand = Get-Command tar.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
$ssh = $sshCommand.Source
$scp = $scpCommand.Source
$tar = $tarCommand.Source
if ([string]::IsNullOrWhiteSpace($ssh) -or [string]::IsNullOrWhiteSpace($scp)) {
    throw 'SSH and SCP must resolve to executable commands or script paths.'
}
$keyPath = (Resolve-Path -LiteralPath $SshKey).Path

function Assert-NoReparsePathComponents {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Description
    )
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $root = [System.IO.Path]::GetPathRoot($fullPath)
    if ([string]::IsNullOrWhiteSpace($root)) {
        throw "$Description must resolve to a local drive path."
    }
    $current = $root
    $parts = $fullPath.Substring($root.Length).Split([char[]]@('\', '/'), [System.StringSplitOptions]::RemoveEmptyEntries)
    for ($index = 0; $index -lt $parts.Length; $index++) {
        $current = Join-Path $current $parts[$index]
        if (-not (Test-Path -LiteralPath $current)) {
            break
        }
        $item = Get-Item -LiteralPath $current -Force
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "$Description contains a reparse point: $current"
        }
        if ($index -lt ($parts.Length - 1) -and -not $item.PSIsContainer) {
            throw "$Description contains a non-directory path component: $current"
        }
    }
}

function Publish-RecoveryKitPointer {
    param(
        [string]$KitRoot,
        [string]$IncomingPath,
        [object]$Manifest
    )
    if (-not (Test-Path -LiteralPath $KitRoot)) {
        New-Item -ItemType Directory -Path $KitRoot | Out-Null
    }
    $rootItem = Get-Item -LiteralPath $KitRoot
    if (-not $rootItem.PSIsContainer -or
        ($rootItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'USB recovery-kit destination is not a safe directory.'
    }
    $pointerPath = Join-Path $KitRoot 'latest.json'
    if (Test-Path -LiteralPath $pointerPath) {
        $pointerItem = Get-Item -LiteralPath $pointerPath
        if ($pointerItem.PSIsContainer -or
            ($pointerItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
            $pointerItem.Length -gt 4096) {
            throw 'Existing USB recovery-kit pointer is not a safe file.'
        }
    }
    $pointerTemp = Join-Path $IncomingPath 'latest-kit.json.partial'
    [System.IO.File]::WriteAllText(
        $pointerTemp,
        ($Manifest | ConvertTo-Json -Depth 4 -Compress) + [Environment]::NewLine,
        [System.Text.UTF8Encoding]::new($false)
    )
    Move-Item -LiteralPath $pointerTemp -Destination $pointerPath -Force
}

$fullUsbPath = [System.IO.Path]::GetFullPath($UsbPath)
if ($fullUsbPath -notmatch '^[A-Za-z]:\\') {
    throw 'USB destination must resolve to a local drive path.'
}
$driveRoot = [System.IO.Path]::GetPathRoot($fullUsbPath)
$drive = [System.IO.DriveInfo]::new($driveRoot)
if (-not $drive.IsReady) {
    throw 'USB destination drive is not ready.'
}
if ($drive.DriveType -notin @([System.IO.DriveType]::Removable, [System.IO.DriveType]::Fixed)) {
    throw 'USB destination must be on a local removable or fixed drive.'
}
if ($drive.DriveType -eq [System.IO.DriveType]::Fixed -and -not $AllowFixedDrive) {
    throw 'Destination is not reported as a removable drive. Use -AllowFixedDrive only after confirming the USB drive letter.'
}
Assert-NoReparsePathComponents -Path $fullUsbPath -Description 'USB destination path'
if (-not (Test-Path -LiteralPath $fullUsbPath -PathType Container)) {
    New-Item -ItemType Directory -Path $fullUsbPath | Out-Null
}
Assert-NoReparsePathComponents -Path $fullUsbPath -Description 'USB destination path'
$destination = (Resolve-Path -LiteralPath $fullUsbPath).Path
$resolvedDestinationRoot = [System.IO.Path]::GetPathRoot($destination)
if (-not [string]::Equals($resolvedDestinationRoot, $driveRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Resolved USB destination escaped its selected drive.'
}

$canonicalDestination = $destination.TrimEnd([char[]]@('\', '/')).ToUpperInvariant()
$pathHash = [Security.Cryptography.SHA256]::Create()
try {
    $pathBytes = [System.Text.Encoding]::UTF8.GetBytes($canonicalDestination)
    $mutexHash = ([BitConverter]::ToString($pathHash.ComputeHash($pathBytes))).Replace('-', '').ToLowerInvariant()
} finally {
    $pathHash.Dispose()
}
$syncMutex = [System.Threading.Mutex]::new($false, "Local\VaporRecoveryUsbSync_$mutexHash")
$mutexAcquired = $false
try {
    $mutexAcquired = $syncMutex.WaitOne(0)
} catch [System.Threading.AbandonedMutexException] {
    # An interrupted sync released its OS mutex when PowerShell exited. Its
    # partial files are safe to remove now because this process owns the lock.
    $mutexAcquired = $true
}
if (-not $mutexAcquired) {
    $syncMutex.Dispose()
    throw "A recovery sync is already running for this destination: $destination"
}

try {
$incoming = Join-Path $destination '.incoming'
Assert-NoReparsePathComponents -Path $incoming -Description 'USB incoming directory'
if (-not (Test-Path -LiteralPath $incoming -PathType Container)) {
    New-Item -ItemType Directory -Path $incoming | Out-Null
}
Assert-NoReparsePathComponents -Path $incoming -Description 'USB incoming directory'
foreach ($incomingItem in Get-ChildItem -LiteralPath $incoming -Force -Recurse) {
    if (($incomingItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "USB incoming directory contains a reparse point: $($incomingItem.Name)"
    }
}
Get-ChildItem -LiteralPath $incoming -File -Filter '*.partial' -ErrorAction SilentlyContinue |
    Remove-Item -Force
Get-ChildItem -LiteralPath $incoming -Directory -Filter '*.partial' -ErrorAction SilentlyContinue |
    Remove-Item -Recurse -Force
$sshOptions = @('-i', $keyPath, '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes', '-o', 'ConnectTimeout=10')
$latestText = & $ssh @sshOptions $target "cat $remoteRoot/latest.json"
if ($LASTEXITCODE -ne 0) {
    throw "Could not read the VPS latest-generation pointer (ssh exit $LASTEXITCODE)."
}
try {
    $latestPointer = ($latestText -join "`n") | ConvertFrom-Json
} catch {
    throw 'The VPS latest-generation pointer is not valid JSON.'
}
$indexText = & $ssh @sshOptions $target "cat $remoteRoot/index.json"
if ($LASTEXITCODE -ne 0) {
    throw "Could not read the VPS recovery index (ssh exit $LASTEXITCODE)."
}

try {
    $index = ($indexText -join "`n") | ConvertFrom-Json
} catch {
    throw 'The VPS recovery index is not valid JSON.'
}
if ($index.schema_version -ne 1 -or $null -eq $index.archives) {
    throw 'The VPS recovery index has an unsupported schema.'
}
if (@($index.archives).Count -lt 1) {
    throw 'The VPS recovery index contains no completed backup generations.'
}
if ($latestPointer.schema_version -ne 1 -or
    $latestPointer.archive -notmatch '^vapor-platform-recovery-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{12}\.tar\.gz$' -or
    $latestPointer.sha256 -notmatch '^[0-9a-f]{64}$' -or
    [long]$latestPointer.size_bytes -le 0) {
    throw 'The VPS latest-generation pointer has an unsupported or invalid schema.'
}
$latestDeclaresRecoveryKit = $latestPointer.PSObject.Properties.Name -contains 'recovery_kit_included'
if ($latestDeclaresRecoveryKit -and $latestPointer.recovery_kit_included -isnot [bool]) {
    throw 'The VPS latest-generation pointer has an invalid recovery-kit declaration.'
}
$latestHasRecoveryKit = $latestDeclaresRecoveryKit -and $latestPointer.recovery_kit_included

$entriesByName = @{}
foreach ($entry in $index.archives) {
    if ($entry.archive -notmatch '^vapor-platform-recovery-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{12}\.tar\.gz$') {
        throw 'The VPS recovery index contains an unexpected archive name.'
    }
    if ($entry.sha256 -notmatch '^[0-9a-f]{64}$' -or [long]$entry.size_bytes -le 0) {
        throw 'The VPS recovery index contains invalid archive metadata.'
    }
    if ($entriesByName.ContainsKey($entry.archive)) {
        throw 'The VPS recovery index contains duplicate archive names.'
    }
    $entriesByName[$entry.archive] = $entry
}

$latestEntry = $entriesByName[[string]$latestPointer.archive]
if ($null -eq $latestEntry -or
    $latestEntry.sha256 -cne [string]$latestPointer.sha256 -or
    [long]$latestEntry.size_bytes -ne [long]$latestPointer.size_bytes) {
    throw 'The VPS latest-generation pointer does not match an indexed archive.'
}
$latestArchive = [string]$latestPointer.archive
$orderedEntries = @($index.archives | Sort-Object { $_.archive })
$historicalEntries = @($orderedEntries | Where-Object { $_.archive -cne $latestArchive })
$auditArchive = $null
$nextAuditArchive = $null
$auditStatePath = Join-Path $destination '.vapor-usb-audit.json'
if ($historicalEntries.Count -gt 0) {
    if ((Test-Path -LiteralPath $auditStatePath) -and
        -not (Test-Path -LiteralPath $auditStatePath -PathType Leaf)) {
        throw 'USB verification state is not a regular file.'
    }
    $auditIndex = 0
    if (Test-Path -LiteralPath $auditStatePath -PathType Leaf) {
        $auditStateItem = Get-Item -LiteralPath $auditStatePath
        if (($auditStateItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'USB verification state is a reparse point.'
        }
        if ($auditStateItem.Length -gt 4096) {
            throw 'USB verification state exceeds the expected size.'
        }
        try {
            $auditState = Get-Content -LiteralPath $auditStatePath -Raw | ConvertFrom-Json
            for ($i = 0; $i -lt $historicalEntries.Count; $i++) {
                if ($historicalEntries[$i].archive -ceq [string]$auditState.next_archive) {
                    $auditIndex = $i
                    break
                }
            }
        } catch {
            $auditIndex = 0
        }
    }
    $auditArchive = $historicalEntries[$auditIndex].archive
    $nextAuditArchive = $historicalEntries[($auditIndex + 1) % $historicalEntries.Count].archive
}

$copied = 0
$hashChecks = 0
$hashedValidNames = @{}
foreach ($entry in $index.archives) {
    $finalPath = Join-Path $destination $entry.archive
    if ((Test-Path -LiteralPath $finalPath) -and
        -not (Test-Path -LiteralPath $finalPath -PathType Leaf)) {
        throw "Existing USB archive path is not a regular file: $($entry.archive)"
    }
    if (Test-Path -LiteralPath $finalPath -PathType Leaf) {
        $sidecarPath = "$finalPath.sha256"
        $expectedSidecar = "$($entry.sha256)  $($entry.archive)"
        $existing = Get-Item -LiteralPath $finalPath
        if (($existing.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Existing USB archive is a reparse point; refusing to follow it: $($entry.archive)"
        }
        $sidecarMatches = $false
        if ((Test-Path -LiteralPath $sidecarPath) -and
            -not (Test-Path -LiteralPath $sidecarPath -PathType Leaf)) {
            throw "Existing USB checksum path is not a regular file: $($entry.archive)"
        }
        if (Test-Path -LiteralPath $sidecarPath -PathType Leaf) {
            $sidecarItem = Get-Item -LiteralPath $sidecarPath
            if (($sidecarItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Existing USB checksum is a reparse point; refusing to follow it: $($entry.archive)"
            }
            if ($sidecarItem.Length -le 256) {
                $sidecarMatches = ((Get-Content -LiteralPath $sidecarPath -Raw).Trim() -ceq $expectedSidecar)
            }
        }
        $mustRehash = $entry.archive -ceq $latestArchive -or $entry.archive -ceq $auditArchive
        if ($existing.Length -ne [long]$entry.size_bytes -or -not $sidecarMatches -or $mustRehash) {
            $hashChecks++
            $existingHash = (Get-FileHash -LiteralPath $finalPath -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($existing.Length -eq [long]$entry.size_bytes -and $existingHash -ceq $entry.sha256) {
                $hashedValidNames[$entry.archive] = $true
                $sidecarTemp = Join-Path $incoming ($entry.archive + '.sha256.partial')
                Set-Content -LiteralPath $sidecarTemp -Value $expectedSidecar -Encoding ascii
                Move-Item -LiteralPath $sidecarTemp -Destination $sidecarPath -Force
                continue
            }
        }
        else {
            continue
        }
    }

    $available = $drive.AvailableFreeSpace
    if ($available -lt ([long]$entry.size_bytes + 512MB)) {
        throw 'USB drive has insufficient free space; older verified copies were left untouched.'
    }

    $partialPath = Join-Path $incoming ($entry.archive + '.partial')
    Remove-Item -LiteralPath $partialPath -Force -ErrorAction SilentlyContinue
    $remoteFile = "$target`:$remoteRoot/$($entry.archive)"
    & $scp @sshOptions $remoteFile $partialPath
    if ($LASTEXITCODE -ne 0) {
        Remove-Item -LiteralPath $partialPath -Force -ErrorAction SilentlyContinue
        throw "Archive transfer failed for $($entry.archive) (scp exit $LASTEXITCODE)."
    }

    $downloaded = Get-Item -LiteralPath $partialPath
    $hashChecks++
    $downloadedHash = (Get-FileHash -LiteralPath $partialPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($downloaded.Length -ne [long]$entry.size_bytes -or $downloadedHash -ne $entry.sha256) {
        Remove-Item -LiteralPath $partialPath -Force
        throw "Downloaded archive failed size or hash verification: $($entry.archive)"
    }
    $hashedValidNames[$entry.archive] = $true

    Move-Item -LiteralPath $partialPath -Destination $finalPath -Force
    $sidecarTemp = Join-Path $incoming ($entry.archive + '.sha256.partial')
    $sidecarPath = "$finalPath.sha256"
    Set-Content -LiteralPath $sidecarTemp -Value "$($entry.sha256)  $($entry.archive)" -Encoding ascii
    Move-Item -LiteralPath $sidecarTemp -Destination $sidecarPath -Force
    $copied++
}

# Materialize the exact recovery entrypoint and its metadata reader from the
# newest verified bundle. Older generations created before this feature may
# not carry the kit; keep archive catch-up working and report that explicitly.
$recoveryKitAvailable = $false
$recoveryKitPreflightValidation = $false
$recoveryKitCommit = $null
$latestBundlePath = Join-Path $destination $latestArchive
$kitStage = Join-Path $incoming ($latestArchive + '.kit.partial')
if ($latestHasRecoveryKit) {
    if (Test-Path -LiteralPath $kitStage) {
        Remove-Item -LiteralPath $kitStage -Recurse -Force
    }
    New-Item -ItemType Directory -Path $kitStage | Out-Null
    $archiveMembers = @(& $tar -tzf $latestBundlePath 2>$null)
    if ($LASTEXITCODE -ne 0) {
        throw 'The latest generation declares a recovery kit, but the archive member list could not be read.'
    }
    $validatorMember = 'vapor-platform-recovery/source/recovery-kit/state-bundle-validate.py'
    $manifestValidatorMember = 'vapor-platform-recovery/source/recovery-kit/deployed_source_manifest.py'
    $hasValidator = $archiveMembers -ccontains $validatorMember
    $hasManifestValidator = $archiveMembers -ccontains $manifestValidatorMember
    if ($hasValidator -ne $hasManifestValidator) {
        throw 'The latest generation declares an incomplete preflight recovery kit.'
    }
    $recoveryKitPreflightValidation = $hasValidator -and $hasManifestValidator

    $kitMembers = @(
        'vapor-platform-recovery/source/root-commit.txt',
        'vapor-platform-recovery/source/recovery-kit/README.txt',
        'vapor-platform-recovery/source/recovery-kit/recover-fresh-host.sh',
        'vapor-platform-recovery/source/recovery-kit/recovery_bundle_metadata.py',
        'vapor-platform-recovery/source/recovery-kit/SHA256SUMS'
    )
    if ($recoveryKitPreflightValidation) {
        $kitMembers += @($validatorMember, $manifestValidatorMember)
    }
    & $tar -xzf $latestBundlePath -C $kitStage @kitMembers 2>$null
    if ($LASTEXITCODE -ne 0) {
        throw 'The latest generation declares a recovery kit, but its required files could not be extracted.'
    }
    $kitSource = Join-Path $kitStage 'vapor-platform-recovery\source'
    $sourceCommitPath = Join-Path $kitSource 'root-commit.txt'
    $kitSourceDir = Join-Path $kitSource 'recovery-kit'
    $expectedKitNames = @(
        'README.txt',
        'recover-fresh-host.sh',
        'recovery_bundle_metadata.py'
    )
    if ($recoveryKitPreflightValidation) {
        $expectedKitNames += @('state-bundle-validate.py', 'deployed_source_manifest.py')
    }
    $checksumPath = Join-Path $kitSourceDir 'SHA256SUMS'
    if ((Test-Path -LiteralPath $sourceCommitPath -PathType Leaf) -and
        (Test-Path -LiteralPath $checksumPath -PathType Leaf)) {
        $commitItem = Get-Item -LiteralPath $sourceCommitPath
        $checksumItem = Get-Item -LiteralPath $checksumPath
        if (($commitItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
            ($checksumItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
            $commitItem.Length -gt 128 -or $checksumItem.Length -gt 4096) {
            throw 'Latest recovery archive contains unsafe recovery-kit metadata.'
        }
        $recoveryKitCommit = (Get-Content -LiteralPath $sourceCommitPath -Raw).Trim()
        if ($recoveryKitCommit -notmatch '^[0-9a-f]{40}$') {
            throw 'Latest recovery archive has an invalid recovery-kit source revision.'
        }
        $checksumText = Get-Content -LiteralPath $checksumPath -Raw
        $expectedHashes = @{}
        foreach ($line in ($checksumText -split '\r?\n' | Where-Object { $_ -ne '' })) {
            if ($line -notmatch '^([0-9a-f]{64})  ([A-Za-z0-9_.-]+)$' -or
                $expectedHashes.ContainsKey($Matches[2])) {
                throw 'Latest recovery archive has a malformed recovery-kit checksum list.'
            }
            $expectedHashes[$Matches[2]] = $Matches[1]
        }
        if (@($expectedHashes.Keys | Sort-Object) -join ',' -cne (@($expectedKitNames | Sort-Object) -join ',')) {
            throw 'Latest recovery archive has an incomplete recovery-kit checksum list.'
        }
        $kitHashes = @{}
        foreach ($name in $expectedKitNames) {
            $filePath = Join-Path $kitSourceDir $name
            if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) {
                throw 'Latest recovery archive is missing a recovery-kit file.'
            }
            $file = Get-Item -LiteralPath $filePath
            if (($file.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or $file.Length -gt 1MB) {
                throw 'Latest recovery archive contains an unsafe or oversized recovery-kit file.'
            }
            $actualHash = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($actualHash -cne $expectedHashes[$name]) {
                throw 'Latest recovery-kit file failed its embedded checksum.'
            }
            $kitHashes[$name] = $actualHash
        }
        $kitHashes['SHA256SUMS'] = (Get-FileHash -LiteralPath $checksumPath -Algorithm SHA256).Hash.ToLowerInvariant()

        $kitRoot = Join-Path $destination 'Recovery-Kit'
        if (-not (Test-Path -LiteralPath $kitRoot)) {
            New-Item -ItemType Directory -Path $kitRoot | Out-Null
        }
        if (-not (Test-Path -LiteralPath $kitRoot -PathType Container) -or
            ((Get-Item -LiteralPath $kitRoot).Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'USB recovery-kit destination is not a safe directory.'
        }
        $finalKit = Join-Path $kitRoot $recoveryKitCommit
        if (Test-Path -LiteralPath $finalKit) {
            if (-not (Test-Path -LiteralPath $finalKit -PathType Container) -or
                ((Get-Item -LiteralPath $finalKit).Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw 'Existing source-pinned recovery kit path is not a safe directory.'
            }
            $kitEntries = @(Get-ChildItem -LiteralPath $finalKit -Force)
            $expectedEntryNames = @($expectedKitNames + 'SHA256SUMS' | Sort-Object)
            if (@($kitEntries.Name | Sort-Object) -join ',' -cne ($expectedEntryNames -join ',') -or
                @($kitEntries | Where-Object { $_.PSIsContainer -or ($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 }).Count -gt 0) {
                throw 'Existing source-pinned recovery kit has unexpected entries.'
            }
            foreach ($name in $expectedEntryNames) {
                $existingKitFile = Join-Path $finalKit $name
                if (-not (Test-Path -LiteralPath $existingKitFile -PathType Leaf) -or
                    (Get-FileHash -LiteralPath $existingKitFile -Algorithm SHA256).Hash.ToLowerInvariant() -cne $kitHashes[$name]) {
                    throw 'Existing source-pinned recovery kit differs from the verified archive.'
                }
            }
        } else {
            Move-Item -LiteralPath $kitSourceDir -Destination $finalKit
        }

        $kitManifest = [ordered]@{
            schema_version = 1
            available = $true
            preflight_validation = $recoveryKitPreflightValidation
            source_commit = $recoveryKitCommit
            archive = $latestArchive
            archive_sha256 = [string]$latestPointer.sha256
            files = $kitHashes
        }
        Publish-RecoveryKitPointer -KitRoot $kitRoot -IncomingPath $incoming -Manifest $kitManifest
        $recoveryKitAvailable = $true
    } else {
        throw 'The latest generation declares a recovery kit, but required kit metadata is missing.'
    }
    Remove-Item -LiteralPath $kitStage -Recurse -Force -ErrorAction SilentlyContinue
} else {
    $kitManifest = [ordered]@{
        schema_version = 1
        available = $false
        preflight_validation = $false
        source_commit = $null
        archive = $latestArchive
        archive_sha256 = [string]$latestPointer.sha256
        files = @{}
    }
    Publish-RecoveryKitPointer -KitRoot (Join-Path $destination 'Recovery-Kit') -IncomingPath $incoming -Manifest $kitManifest
}

if ($historicalEntries.Count -gt 0) {
    $auditStateTemp = Join-Path $incoming '.vapor-usb-audit.json.partial'
    [ordered]@{
        schema_version = 1
        next_archive = $nextAuditArchive
        last_rehashed_archive = $auditArchive
        last_rehashed_utc = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    } | ConvertTo-Json -Compress | Set-Content -LiteralPath $auditStateTemp -Encoding utf8
    Move-Item -LiteralPath $auditStateTemp -Destination $auditStatePath -Force
}

$allArchives = @(Get-ChildItem -LiteralPath $destination -File -Filter 'vapor-platform-recovery-*.tar.gz' |
    Sort-Object Name)
$indexedArchives = @()
foreach ($archiveFile in $allArchives) {
    if (($archiveFile.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        continue
    }
    $sidecarPath = "$($archiveFile.FullName).sha256"
    if (-not (Test-Path -LiteralPath $sidecarPath -PathType Leaf)) {
        continue
    }
    $sidecarItem = Get-Item -LiteralPath $sidecarPath
    if (($sidecarItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
        $sidecarItem.Length -gt 256) {
        continue
    }
    $sidecar = (Get-Content -LiteralPath $sidecarPath -Raw).Trim()
    if ($sidecar -notmatch '^([0-9a-f]{64})  (vapor-platform-recovery-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{12}\.tar\.gz)$') {
        continue
    }
    if ($Matches[2] -cne $archiveFile.Name) {
        continue
    }
    $sidecarDigest = $Matches[1]
    $expectedEntry = $entriesByName[$archiveFile.Name]
    if ($null -ne $expectedEntry -and
        ($archiveFile.Length -ne [long]$expectedEntry.size_bytes -or $sidecarDigest -cne $expectedEntry.sha256)) {
        continue
    }
    if ($archiveFile.Length -gt 0) {
        $indexedArchives += $archiveFile
    }
}
if (-not ($indexedArchives.Name -contains $latestArchive) -or
    -not $hashedValidNames.ContainsKey($latestArchive)) {
    throw 'The newest VPS generation is not present as a content-verified USB copy.'
}

$verifiedArchives = @($indexedArchives | Where-Object { $hashedValidNames.ContainsKey($_.Name) })
$allArchiveBytes = [long](($allArchives | Measure-Object -Property Length -Sum).Sum)
if ($allArchiveBytes -gt $MaxStoredBytes) {
    # Capacity pruning must make its decision from payload hashes, never from
    # a sidecar that could still match after an archive was damaged.
    foreach ($archiveFile in $indexedArchives) {
        if ($hashedValidNames.ContainsKey($archiveFile.Name)) {
            continue
        }
        if (-not $entriesByName.ContainsKey($archiveFile.Name)) {
            continue
        }
        $entry = $entriesByName[$archiveFile.Name]
        $hashChecks++
        $actualHash = (Get-FileHash -LiteralPath $archiveFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($archiveFile.Length -eq [long]$entry.size_bytes -and $actualHash -ceq $entry.sha256) {
            $hashedValidNames[$archiveFile.Name] = $true
        }
    }
    $verifiedArchives = @($indexedArchives | Where-Object { $hashedValidNames.ContainsKey($_.Name) })
    $unverifiedArchives = @($allArchives | Where-Object { $_.Name -notin $verifiedArchives.Name })
    $unverifiedBytes = [long](($unverifiedArchives | Measure-Object -Property Length -Sum).Sum)
    $totalBytes = [long](($verifiedArchives | Measure-Object -Property Length -Sum).Sum)
    $latestLength = [long](($verifiedArchives | Where-Object { $_.Name -ceq $latestArchive } |
        Measure-Object -Property Length -Sum).Sum)
    if (($latestLength + $unverifiedBytes) -gt $MaxStoredBytes) {
        throw 'The latest archive plus unverified USB files exceed the storage budget; no archive was removed.'
    }
    while (($totalBytes + $unverifiedBytes) -gt $MaxStoredBytes) {
        $oldest = @($verifiedArchives | Where-Object { $_.Name -cne $latestArchive } | Sort-Object Name | Select-Object -First 1)
        if ($oldest.Count -eq 0) {
            throw 'The latest archive and unverified files exceed the configured USB storage budget.'
        }
        $oldest = $oldest[0]
        Remove-Item -LiteralPath $oldest.FullName -Force
        Remove-Item -LiteralPath "$($oldest.FullName).sha256" -Force -ErrorAction SilentlyContinue
        $totalBytes -= $oldest.Length
        $verifiedArchives = @($verifiedArchives | Where-Object { $_.FullName -cne $oldest.FullName })
    }
}

$allArchives = @(Get-ChildItem -LiteralPath $destination -File -Filter 'vapor-platform-recovery-*.tar.gz' |
    Sort-Object Name)
$unverifiedArchives = @($allArchives | Where-Object { $_.Name -notin $verifiedArchives.Name })

$report = [ordered]@{
    schema_version = 1
    server = $Server
    last_success_utc = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    latest_archive = $latestArchive
    rotating_audit_archive = $auditArchive
    copied_this_run = $copied
    hash_checks_this_run = $hashChecks
    retained_archive_files = $allArchives.Count
    content_verified_this_run = $verifiedArchives.Count
    retained_archive_payload_bytes = [long]((Get-ChildItem -LiteralPath $destination -File -Filter 'vapor-platform-recovery-*.tar.gz' |
        Measure-Object -Property Length -Sum).Sum)
    unverified_archives = $unverifiedArchives.Count
    latest_rehashed = $true
    recovery_kit_available = $recoveryKitAvailable
    recovery_kit_preflight_validation = $recoveryKitPreflightValidation
    recovery_kit_source_commit = $recoveryKitCommit
    verification_policy = 'latest and one rotating historical archive each sync; all indexed candidates rehashed before pruning'
}
$reportTemp = Join-Path $incoming 'last-sync.json.partial'
$reportPath = Join-Path $destination 'last-sync.json'
$report | ConvertTo-Json -Compress | Set-Content -LiteralPath $reportTemp -Encoding utf8
Move-Item -LiteralPath $reportTemp -Destination $reportPath -Force

Write-Output "USB recovery sync complete: copied $copied archive(s); content-verified $($verifiedArchives.Count) generation(s) this run."
} finally {
    if ($mutexAcquired) {
        $syncMutex.ReleaseMutex()
    }
    $syncMutex.Dispose()
}
