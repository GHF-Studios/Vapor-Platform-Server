[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9.-]+$')]
    [string]$Server,

    [Parameter(Mandatory = $true)]
    [string]$SshKey,

    [Parameter(Mandatory = $true)]
    [string]$UsbPath,

    [ValidateRange(1GB, 120GB)]
    [long]$MaxStoredBytes = 100GB,

    [switch]$AllowFixedDrive
)

$ErrorActionPreference = 'Stop'
$remoteRoot = '/var/backups/vapor-server'
$remoteUser = 'root'
$target = "$remoteUser@$Server"
$ssh = Get-Command ssh.exe -ErrorAction Stop | Select-Object -ExpandProperty Source
$scp = Get-Command scp.exe -ErrorAction Stop | Select-Object -ExpandProperty Source
$keyPath = (Resolve-Path -LiteralPath $SshKey).Path

$driveRoot = [System.IO.Path]::GetPathRoot([System.IO.Path]::GetFullPath($UsbPath))
if ([string]::IsNullOrWhiteSpace($driveRoot)) {
    throw 'USB destination must resolve to a local drive path.'
}
$drive = [System.IO.DriveInfo]::new($driveRoot)
if (-not $drive.IsReady) {
    throw 'USB destination drive is not ready.'
}
if ($drive.DriveType -ne [System.IO.DriveType]::Removable -and -not $AllowFixedDrive) {
    throw 'Destination is not reported as a removable drive. Use -AllowFixedDrive only after confirming the USB drive letter.'
}
if (-not (Test-Path -LiteralPath $UsbPath -PathType Container)) {
    New-Item -ItemType Directory -Path $UsbPath -Force | Out-Null
}
$destination = (Resolve-Path -LiteralPath $UsbPath).Path

$incoming = Join-Path $destination '.incoming'
New-Item -ItemType Directory -Path $incoming -Force | Out-Null
Get-ChildItem -LiteralPath $incoming -File -Filter '*.partial' -ErrorAction SilentlyContinue |
    Remove-Item -Force
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
    retained_bytes = [long]((Get-ChildItem -LiteralPath $destination -File -Filter 'vapor-platform-recovery-*.tar.gz' |
        Measure-Object -Property Length -Sum).Sum)
    unverified_archives = $unverifiedArchives.Count
    latest_rehashed = $true
    verification_policy = 'latest and one rotating historical archive each sync; all indexed candidates rehashed before pruning'
}
$reportTemp = Join-Path $incoming 'last-sync.json.partial'
$reportPath = Join-Path $destination 'last-sync.json'
$report | ConvertTo-Json -Compress | Set-Content -LiteralPath $reportTemp -Encoding utf8
Move-Item -LiteralPath $reportTemp -Destination $reportPath -Force

Write-Output "USB recovery sync complete: copied $copied archive(s); content-verified $($verifiedArchives.Count) generation(s) this run."
