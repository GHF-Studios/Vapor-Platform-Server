[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9.-]+$')]
    [string]$Server,

    [Parameter(Mandatory = $true)]
    [string]$SshKey,

    [Parameter(Mandatory = $true)]
    [string]$UsbPath,

    [string]$RecoveryRepositoryPath = (Join-Path $env:USERPROFILE 'Vapor\cache\repositories\Vapor-Platform-Server.git'),

    [string]$TaskName = 'Vapor USB Recovery Sync',

    [switch]$AllowFixedDrive
)

$ErrorActionPreference = 'Stop'
$keyPath = (Resolve-Path -LiteralPath $SshKey).Path
if (-not (Test-Path -LiteralPath $RecoveryRepositoryPath)) {
    $repositoryParent = Split-Path -Parent ([System.IO.Path]::GetFullPath($RecoveryRepositoryPath))
    New-Item -ItemType Directory -Path $repositoryParent -Force | Out-Null
    $bootstrapGit = Get-Command git.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
    & $bootstrapGit.Source clone --bare --single-branch --branch main `
        'https://github.com/GHF-Studios/Vapor-Platform-Server.git' $RecoveryRepositoryPath
    if ($LASTEXITCODE -ne 0) {
        throw 'Could not bootstrap the Platform recovery-source cache from its public GitHub repository.'
    }
}
$sourceRepositoryPath = (Resolve-Path -LiteralPath $RecoveryRepositoryPath).Path
$gitCommand = Get-Command git.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
& $gitCommand.Source -C $sourceRepositoryPath rev-parse --git-dir 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw 'RecoveryRepositoryPath must be an existing Git checkout or bare repository.'
}
$sourceBackupScript = Join-Path $PSScriptRoot 'backup-state-only-to-usb.ps1'
$sourceMetadataHelper = Join-Path $PSScriptRoot 'recovery_bundle_metadata.py'
$triggerHelper = Join-Path $PSScriptRoot 'usb_backup_task.ps1'
if (-not (Test-Path -LiteralPath $sourceBackupScript -PathType Leaf)) {
    throw 'The state-only USB sync script is missing beside the task installer.'
}
if (-not (Test-Path -LiteralPath $sourceMetadataHelper -PathType Leaf)) {
    throw 'The recovery metadata helper is missing beside the task installer.'
}
if (-not (Test-Path -LiteralPath $triggerHelper -PathType Leaf)) {
    throw 'The USB task trigger helper is missing beside the task installer.'
}
. $triggerHelper
if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    throw 'Run this installer as the Windows user who will own the scheduled task.'
}
$localScriptDirectory = Join-Path $env:LOCALAPPDATA 'Vapor\Recovery'
New-Item -ItemType Directory -Path $localScriptDirectory -Force | Out-Null
$backupScript = Join-Path $localScriptDirectory 'backup-state-only-to-usb.ps1'
Copy-Item -LiteralPath $sourceBackupScript -Destination $backupScript -Force
Copy-Item -LiteralPath $sourceMetadataHelper -Destination (Join-Path $localScriptDirectory 'recovery_bundle_metadata.py') -Force
$powershellExe = Get-VaporUsbTaskPowerShellPath
function ConvertTo-WindowsCommandLineArgument {
    param([Parameter(Mandatory = $true)][string]$Value)

    $builder = [System.Text.StringBuilder]::new()
    [void]$builder.Append('"')
    $backslashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq '\') {
            $backslashes++
            continue
        }
        if ($character -eq '"') {
            [void]$builder.Append(('\' * (2 * $backslashes + 1)))
            [void]$builder.Append('"')
            $backslashes = 0
            continue
        }
        if ($backslashes -gt 0) {
            [void]$builder.Append(('\' * $backslashes))
            $backslashes = 0
        }
        [void]$builder.Append($character)
    }
    if ($backslashes -gt 0) {
        [void]$builder.Append(('\' * (2 * $backslashes)))
    }
    [void]$builder.Append('"')
    return $builder.ToString()
}

$arguments = @(
    '-NoProfile'
    '-WindowStyle'
    'Hidden'
    '-ExecutionPolicy'
    'Bypass'
    '-File'
    (ConvertTo-WindowsCommandLineArgument $backupScript)
    '-Server'
    (ConvertTo-WindowsCommandLineArgument $Server)
    '-SshKey'
    (ConvertTo-WindowsCommandLineArgument $keyPath)
    '-UsbPath'
    (ConvertTo-WindowsCommandLineArgument $UsbPath)
    '-RecoveryRepositoryPath'
    (ConvertTo-WindowsCommandLineArgument $sourceRepositoryPath)
) -join ' '
if ($AllowFixedDrive) {
    $arguments += ' -AllowFixedDrive'
}

$action = New-ScheduledTaskAction -Execute $powershellExe -Argument $arguments
$user = "$env:USERDOMAIN\$env:USERNAME"
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$triggers = New-VaporUsbRecoveryTriggers -User $user
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable `
    -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew

Register-ScheduledTask -TaskName $TaskName -Action $action `
    -Trigger $triggers -Principal $principal -Settings $settings -Force | Out-Null
Write-Output "Scheduled hourly state-only recovery sync for $user. It will catch up when the USB drive is mounted and the user is signed in."
