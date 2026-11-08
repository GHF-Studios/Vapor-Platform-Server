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
$remoteRoot = '/var/backups/vapor-server/state-only'
$target = "root@$Server"
$ssh = (Get-Command ssh.exe -CommandType Application -ErrorAction Stop).Source
$scp = (Get-Command scp.exe -CommandType Application -ErrorAction Stop).Source
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
if ($latest.schema_version -ne 1 -or $latest.archive_schema_version -ne 3 -or
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
Write-Output "state-usb-sync: verified $(@($index.archives).Count) generations; copied $copied; $verifiedBytes bytes"
