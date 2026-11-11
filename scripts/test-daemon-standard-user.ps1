<#
.SYNOPSIS
Runs the daemon's tests as a user without administrator rights. For CI.

.DESCRIPTION
Every other Windows test of the daemon runs as the runner's own user, an elevated
administrator: privileges let that user past access lists, and what it creates belongs to the
Administrators group. A person's computer is not like that. This makes a local standard user
and runs, as that user, with that user's own profile and environment:

- tests\process_tree.rs: a run, cancel, pause, wrappers, the pseudoconsole (Job Objects);
- tests\integration.rs: the whole daemon against a mock server and the fake CLIs (run
  folders, the named pipe, the request folder, a side chat's read-only checkout, gates);
- tests\service_task.rs: the scheduled task of that user: install, restart, the single
  daemon, an update by the replace protocol, uninstall. The user has no logon session here
  (it was never at the logon screen), and the daemon's task runs only in its user's session:
  the Task Scheduler registers the task, with its logon trigger, and never starts it. The test
  then starts the task's own actions, read from the registered definition, the way the
  scheduler does. What stays unseen is the scheduler itself starting the task for a standard
  user; it does for the runner's own user, by the same definition.

The user is removed at the end. A test that fails fails this script.
#>
[CmdletBinding()]
param(
    [string]$Target = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\debug')
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

$user = 'tcstandard'
$secret = 'Tc!' + [guid]::NewGuid().ToString('N') + 'aA1'
$secure = ConvertTo-SecureString $secret -AsPlainText -Force
New-LocalUser -Name $user -Password $secure -AccountNeverExpires | Out-Null
Add-LocalGroupMember -Group 'Users' -Member $user
$cred = New-Object Management.Automation.PSCredential("$env:COMPUTERNAME\$user", $secure)
$admins = @(Get-LocalGroupMember -Group 'Administrators' | ForEach-Object { $_.Name })
if ($admins -contains "$env:COMPUTERNAME\$user") { throw "$user is an administrator" }

# A folder the user may write to, for the tests' output.
$work = 'C:\tc-standard'
New-Item -ItemType Directory -Force -Path $work | Out-Null
& icacls.exe $work /inheritance:r /grant:r '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F' "${user}:(OI)(CI)M" | Out-Null

$failed = @()
try {
    foreach ($test in @('process_tree', 'integration', 'service_task')) {
        $exe = Get-ChildItem -Path (Join-Path $Target 'deps') -Filter "$test-*.exe" |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $exe) { throw "the test program $test is not built in $Target\deps" }
        Write-Host "=== $test as $user ($($exe.Name))"
        $out = Join-Path $work "$test.out"
        $err = Join-Path $work "$test.err"
        # A process started with other credentials gets this session's environment, not the
        # new user's: the profile folders in it are the runner's own, which the standard
        # user may not open. A small batch file gives the test the environment the user has
        # after a logon (the profile is loaded, so its folders exist).
        $more = ''
        # The test harness prints each test's own output only when it fails.
        if ($test -eq 'integration') { $more = ' --test-threads=4' }
        $wrapper = Join-Path $work "$test.cmd"
        $lines = @(
            '@echo off'
            "set `"USERNAME=$user`""
            "set `"USERPROFILE=C:\Users\$user`""
            'set "HOMEDRIVE=C:"'
            "set `"HOMEPATH=\Users\$user`""
            'set "APPDATA=%USERPROFILE%\AppData\Roaming"'
            'set "LOCALAPPDATA=%USERPROFILE%\AppData\Local"'
            'set "TEMP=%LOCALAPPDATA%\Temp"'
            'set "TMP=%LOCALAPPDATA%\Temp"'
            'set "HOME="'
            # This user has no logon session of its own: its scheduled task is registered but
            # never started (tests\service_task.rs then starts the task's actions itself).
            'set "TC_NO_LOGON_SESSION=1"'
            'if not exist "%TEMP%" mkdir "%TEMP%"'
            'echo running as %USERNAME% with profile %USERPROFILE%'
            'whoami /groups | findstr /c:"S-1-16-"'
            "`"$($exe.FullName)`"$more"
            'exit /b %ERRORLEVEL%'
        )
        Set-Content -LiteralPath $wrapper -Value $lines -Encoding Ascii
        $start = @{
            FilePath               = Join-Path $env:SystemRoot 'System32\cmd.exe'
            ArgumentList           = @('/d', '/c', "`"$wrapper`"")
            Credential             = $cred
            LoadUserProfile        = $true
            WorkingDirectory       = $work
            Wait                   = $true
            PassThru               = $true
            RedirectStandardOutput = $out
            RedirectStandardError  = $err
        }
        $p = Start-Process @start
        Get-Content -LiteralPath $out -ErrorAction SilentlyContinue | Select-Object -Last 200
        if ($p.ExitCode -ne 0) {
            Write-Host "--- standard error of $test"
            Get-Content -LiteralPath $err -ErrorAction SilentlyContinue | Select-Object -Last 200
            $failed += "$test (exit code $($p.ExitCode))"
        }
    }
}
finally {
    $ErrorActionPreference = 'Continue'
    # What a failed test may have left: the user's tasks and processes.
    Get-ScheduledTask -ErrorAction SilentlyContinue |
        Where-Object { $_.Principal.UserId -like "*$user" } |
        Unregister-ScheduledTask -Confirm:$false -ErrorAction SilentlyContinue
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | ForEach-Object {
        $owner = Invoke-CimMethod -InputObject $_ -MethodName GetOwner -ErrorAction SilentlyContinue
        if ($owner -and $owner.User -eq $user) { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    }
    Remove-LocalUser -Name $user -ErrorAction SilentlyContinue
}
if ($failed.Count -gt 0) { throw "as a standard user these failed: $($failed -join ', ')" }
Write-Host 'the daemon tests pass as a standard user'
