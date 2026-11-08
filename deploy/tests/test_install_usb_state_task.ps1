$ErrorActionPreference = 'Stop'
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "vapor-usb-task-$([guid]::NewGuid().ToString('N'))"
$localAppData = Join-Path $testRoot 'local'
$keyPath = Join-Path $testRoot 'fixture-key'
New-Item -ItemType Directory -Path $testRoot | Out-Null
Set-Content -LiteralPath $keyPath -Value 'fixture' -Encoding ascii
$oldLocalAppData = $env:LOCALAPPDATA
$global:VaporUsbTestRegisteredTask = $null

function global:New-ScheduledTaskAction {
    param([string]$Execute, [string]$Argument)
    [pscustomobject]@{ Execute = $Execute; Arguments = $Argument }
}
function global:New-ScheduledTaskPrincipal {
    param([string]$UserId, [string]$LogonType, [string]$RunLevel)
    [pscustomobject]@{ UserId = $UserId; LogonType = $LogonType; RunLevel = $RunLevel }
}
function global:New-ScheduledTaskSettingsSet { [pscustomobject]@{ Settings = 'fixture' } }
function global:New-ScheduledTaskTrigger {
    param([switch]$Daily, [string]$At, [switch]$AtLogOn, [string]$User)
    [pscustomobject]@{ Daily = $Daily; At = $At; AtLogOn = $AtLogOn; User = $User; Repetition = $null }
}
function global:Get-CimClass { [pscustomobject]@{ ClassName = 'fixture' } }
function global:New-CimInstance { [pscustomobject]@{ Interval = 'PT1H' } }
function global:Register-ScheduledTask {
    param($TaskName, $Action, $Trigger, $Principal, $Settings, [switch]$Force)
    $global:VaporUsbTestRegisteredTask = [pscustomobject]@{ Name = $TaskName; Action = $Action }
}

try {
    $env:LOCALAPPDATA = $localAppData
    $installer = Join-Path $PSScriptRoot '..\scripts\install-usb-backup-task.ps1'
    & $installer -Server 'fixture.example' -SshKey $keyPath -UsbPath 'E:\VaporRecovery' `
        -RecoveryRepositoryPath $repoRoot -AllowFixedDrive | Out-Null

    $installed = Join-Path $localAppData 'Vapor\Recovery\backup-state-only-to-usb.ps1'
    if (-not (Test-Path -LiteralPath $installed -PathType Leaf)) {
        throw 'The installer did not install the state-only sync script.'
    }
    if (-not (Test-Path -LiteralPath (Join-Path (Split-Path $installed) 'recovery_bundle_metadata.py') -PathType Leaf)) {
        throw 'The installer did not install the recovery metadata helper.'
    }
    $arguments = [string]$global:VaporUsbTestRegisteredTask.Action.Arguments
    if ($arguments -notmatch 'backup-state-only-to-usb\.ps1' -or
        $arguments -notmatch '-RecoveryRepositoryPath\s+"[^"]+"' -or
        $arguments -match 'backup-to-usb\.ps1(?!\.)') {
        throw "The registered task does not use the state-only sync and explicit source path: $arguments"
    }
    if ($arguments -notmatch [regex]::Escape($repoRoot)) {
        throw 'The task did not preserve the configured recovery source path.'
    }

    & $installer -Server 'fixture.example' -SshKey $keyPath -UsbPath 'E:\VaporRecovery' `
        -AllowFixedDrive | Out-Null
    $defaultArguments = [string]$global:VaporUsbTestRegisteredTask.Action.Arguments
    $expectedCache = Join-Path $env:USERPROFILE 'Vapor\cache\repositories\Vapor-Platform-Server.git'
    if ($defaultArguments -notmatch [regex]::Escape($expectedCache)) {
        throw 'The default task source is not the durable Vapor-home repository cache.'
    }
    Write-Output 'state-only USB task installer tests passed'
} finally {
    $env:LOCALAPPDATA = $oldLocalAppData
    Remove-Item Function:\New-ScheduledTaskAction -ErrorAction SilentlyContinue
    Remove-Item Function:\New-ScheduledTaskPrincipal -ErrorAction SilentlyContinue
    Remove-Item Function:\New-ScheduledTaskSettingsSet -ErrorAction SilentlyContinue
    Remove-Item Function:\New-ScheduledTaskTrigger -ErrorAction SilentlyContinue
    Remove-Item Function:\Get-CimClass -ErrorAction SilentlyContinue
    Remove-Item Function:\New-CimInstance -ErrorAction SilentlyContinue
    Remove-Item Function:\Register-ScheduledTask -ErrorAction SilentlyContinue
    $fullTestRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $fullTestRoot.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing test cleanup outside the temporary directory.'
    }
    Remove-Item -LiteralPath $fullTestRoot -Recurse -Force -ErrorAction SilentlyContinue
}
