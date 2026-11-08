function New-VaporUsbRecoveryTriggers {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$User
    )

    $daily = New-ScheduledTaskTrigger -Daily -At '00:00'
    $repetitionClass = Get-CimClass `
        -Namespace 'root/Microsoft/Windows/TaskScheduler' `
        -ClassName 'MSFT_TaskRepetitionPattern' `
        -ErrorAction Stop
    $repetition = New-CimInstance -CimClass $repetitionClass -ClientOnly -Property @{
        Interval = 'PT1H'
        Duration = 'P1D'
        StopAtDurationEnd = $false
    }
    $daily.Repetition = $repetition

    $atLogon = New-ScheduledTaskTrigger -AtLogOn -User $User
    return @($daily, $atLogon)
}

function Get-VaporUsbTaskPowerShellPath {
    [CmdletBinding()]
    param()

    $path = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw 'The stable Windows PowerShell executable is unavailable.'
    }
    return $path
}
