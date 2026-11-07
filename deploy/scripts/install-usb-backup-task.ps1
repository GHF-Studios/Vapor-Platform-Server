[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9.-]+$')]
    [string]$Server,

    [Parameter(Mandatory = $true)]
    [string]$SshKey,

    [Parameter(Mandatory = $true)]
    [string]$UsbPath,

    [string]$TaskName = 'Vapor USB Recovery Sync',

    [switch]$AllowFixedDrive
)

$ErrorActionPreference = 'Stop'
$keyPath = (Resolve-Path -LiteralPath $SshKey).Path
$sourceBackupScript = Join-Path $PSScriptRoot 'backup-to-usb.ps1'
if (-not (Test-Path -LiteralPath $sourceBackupScript -PathType Leaf)) {
    throw 'The USB backup script is missing beside the task installer.'
}
if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    throw 'Run this installer as the Windows user who will own the scheduled task.'
}
$localScriptDirectory = Join-Path $env:LOCALAPPDATA 'Vapor\Recovery'
New-Item -ItemType Directory -Path $localScriptDirectory -Force | Out-Null
$backupScript = Join-Path $localScriptDirectory 'backup-to-usb.ps1'
Copy-Item -LiteralPath $sourceBackupScript -Destination $backupScript -Force
$powershellName = if ($PSVersionTable.PSVersion.Major -ge 7) { 'pwsh.exe' } else { 'powershell.exe' }
$powershellExe = Join-Path $PSHOME $powershellName
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
) -join ' '
if ($AllowFixedDrive) {
    $arguments += ' -AllowFixedDrive'
}

$action = New-ScheduledTaskAction -Execute $powershellExe -Argument $arguments
$user = "$env:USERDOMAIN\$env:USERNAME"
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$hourly = New-ScheduledTaskTrigger -Daily -At '00:00' `
    -RepetitionInterval (New-TimeSpan -Hours 1) `
    -RepetitionDuration (New-TimeSpan -Days 1)
$atLogon = New-ScheduledTaskTrigger -AtLogOn -User $user
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable `
    -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew

Register-ScheduledTask -TaskName $TaskName -Action $action `
    -Trigger @($hourly, $atLogon) -Principal $principal -Settings $settings -Force | Out-Null
Write-Output "Scheduled hourly recovery sync for $user. It will catch up when the USB drive is mounted and the user is signed in."
