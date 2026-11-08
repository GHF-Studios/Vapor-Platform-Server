$ErrorActionPreference = 'Stop'

$helperPath = Join-Path $PSScriptRoot '..\scripts\usb_backup_task.ps1'
. (Resolve-Path -LiteralPath $helperPath).Path

$user = "$env:USERDOMAIN\$env:USERNAME"
$triggers = @(New-VaporUsbRecoveryTriggers -User $user)
if ($triggers.Count -ne 2) {
    throw "Expected daily and logon triggers; got $($triggers.Count)."
}

$daily = $triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskDailyTrigger' }
if ($null -eq $daily -or $daily.Repetition.Interval -ne 'PT1H' -or $daily.Repetition.Duration -ne 'P1D') {
    throw 'Daily recovery trigger must repeat hourly for one day.'
}

$logon = $triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' }
if ($null -eq $logon -or $logon.UserId -ne $user) {
    throw 'Recovery catch-up trigger must be scoped to the current user.'
}

$expectedPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
if ((Get-VaporUsbTaskPowerShellPath) -cne $expectedPowerShell) {
    throw 'Scheduled recovery task must use the stable Windows PowerShell installation.'
}

Write-Output 'USB recovery task trigger tests passed.'
