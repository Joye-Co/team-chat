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

The script first gives the user a real logon session (Remote Desktop to this computer, three
attempts). With one, the scheduler starts the task and the install command is tested too.
Without one the tests above still run, and the script fails unless -AllowNoSession is given.

The user is removed at the end. A test that fails fails this script.
#>
[CmdletBinding()]
param(
    [string]$Target = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\debug'),
    # Without a logon session for the user the tests still run (the test starts the task's
    # actions itself), and the script then fails: the Task Scheduler starting the task was
    # not seen. With this switch that outcome passes; CI does not use it.
    [switch]$AllowNoSession
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

# A logon session for the user. The daemon's task runs only in its user's logon session, and
# a process started with other credentials (what this script does) is not one: without a
# session the Task Scheduler registers the task and never starts it. The only way to a real
# session without a person at the screen is Remote Desktop to this computer itself. Tried
# here; what happened is printed, and the tests below take the path that fits.
function Get-UserSession {
    $ErrorActionPreference = 'Continue'
    $lines = & (Join-Path $env:SystemRoot 'System32\quser.exe') 2>&1 | Out-String
    $ErrorActionPreference = 'Stop'
    return ($lines -split "`r?`n" | Where-Object { $_ -match "(^|\s|>)$user\s" } | Select-Object -First 1)
}
$session = $null
try {
    Write-Host '=== a logon session for the standard user (Remote Desktop to this computer)'
    $ts = 'HKLM:\System\CurrentControlSet\Control\Terminal Server'
    Set-ItemProperty -Path $ts -Name fDenyTSConnections -Value 0
    Set-ItemProperty -Path "$ts\WinStations\RDP-Tcp" -Name UserAuthentication -Value 0
    Add-LocalGroupMember -Group 'Remote Desktop Users' -Member $user
    Start-Service -Name TermService -ErrorAction SilentlyContinue
    Write-Host "Remote Desktop service: $((Get-Service -Name TermService).Status)"
    $client = 'HKCU:\Software\Microsoft\Terminal Server Client'
    New-Item -Path $client -Force | Out-Null
    Set-ItemProperty -Path $client -Name AuthenticationLevelOverride -Value 0
    & cmdkey.exe "/generic:TERMSRV/127.0.0.2" "/user:$env:COMPUTERNAME\$user" "/pass:$secret" | Out-Null
    foreach ($attempt in 1..3) {
        $rdp = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\mstsc.exe') -ArgumentList '/v:127.0.0.2', '/w:1024', '/h:768' -PassThru
        foreach ($i in 1..45) {
            $session = Get-UserSession
            if ($session) { break }
            if ($rdp.HasExited) { Write-Host "the Remote Desktop client ended (exit code $($rdp.ExitCode))"; break }
            Start-Sleep -Seconds 2
        }
        if ($session) { break }
        Write-Host "attempt ${attempt}: no logon session yet"
        if (-not $rdp.HasExited) { Stop-Process -Id $rdp.Id -Force -ErrorAction SilentlyContinue }
        Start-Sleep -Seconds 5
    }
    if ($session) { Write-Host "the user is logged on: $($session.Trim())" }
    else {
        Write-Host 'no logon session for the user. Sessions on this computer:'
        $ErrorActionPreference = 'Continue'
        & (Join-Path $env:SystemRoot 'System32\quser.exe') 2>&1 | Out-Host
        $ErrorActionPreference = 'Stop'
    }
}
catch {
    Write-Host "no logon session for the user: $($_.Exception.Message)"
    $session = $null
}

# Runs a command line as the user, with the environment the user has after a logon. A process
# started with other credentials gets this session's environment, not the new user's: the
# profile folders in it are the runner's own, which the standard user may not open. A small
# batch file sets the user's own (the profile is loaded, so its folders exist).
function Invoke-AsUser([string]$Name, [string]$CommandLine) {
    $out = Join-Path $work "$Name.out"
    $err = Join-Path $work "$Name.err"
    $wrapper = Join-Path $work "$Name.cmd"
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
    )
    # Without a logon session the user's scheduled task is registered but never started:
    # tests\service_task.rs then starts the task's actions itself.
    if (-not $session) { $lines += 'set "TC_NO_LOGON_SESSION=1"' }
    $lines += @(
        'if not exist "%TEMP%" mkdir "%TEMP%"'
        'echo running as %USERNAME% with profile %USERPROFILE%'
        'whoami /groups | findstr /c:"S-1-16-"'
        $CommandLine
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
    # To the host, not to the pipeline: the function's value is the exit code alone.
    Get-Content -LiteralPath $out -ErrorAction SilentlyContinue | Select-Object -Last 200 | Out-Host
    if ($p.ExitCode -ne 0) {
        Write-Host "--- standard error of $Name"
        Get-Content -LiteralPath $err -ErrorAction SilentlyContinue | Select-Object -Last 200 | Out-Host
    }
    return $p.ExitCode
}

$failed = @()
try {
    foreach ($test in @('process_tree', 'integration', 'service_task')) {
        $exe = Get-ChildItem -Path (Join-Path $Target 'deps') -Filter "$test-*.exe" |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $exe) { throw "the test program $test is not built in $Target\deps" }
        Write-Host "=== $test as $user ($($exe.Name))"
        $more = ''
        # The test harness prints each test's own output only when it fails.
        if ($test -eq 'integration') { $more = ' --test-threads=4' }
        $code = Invoke-AsUser $test "`"$($exe.FullName)`"$more"
        if ($code -ne 0) { $failed += "$test (exit code $code)" }
    }
    # The install command, as that user: only with a logon session, because it waits for the
    # Task Scheduler to start the real daemon.
    if ($session) {
        Write-Host "=== the PowerShell install command as $user"
        $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
        $script = Join-Path $PSScriptRoot 'test-install-daemon.ps1'
        $daemon = Join-Path $Target 'daemon.exe'
        $code = Invoke-AsUser 'install' "`"$ps`" -NoProfile -ExecutionPolicy Bypass -File `"$script`" -Daemon `"$daemon`""
        if ($code -ne 0) { $failed += "install command (exit code $code)" }
    }
    else {
        Write-Host '=== the PowerShell install command as a standard user: not run (it needs the Task Scheduler to start the task, which needs a logon session)'
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
    & cmdkey.exe "/delete:TERMSRV/127.0.0.2" 2>&1 | Out-Null
    Get-Process -Name mstsc -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-LocalUser -Name $user -ErrorAction SilentlyContinue
}
if ($failed.Count -gt 0) { throw "as a standard user these failed: $($failed -join ', ')" }
if ($session) { Write-Host 'the daemon tests pass as a standard user, with the Task Scheduler starting the task in the user''s logon session' }
elseif ($AllowNoSession) { Write-Host 'the daemon tests pass as a standard user (no logon session: the task''s actions were started by the test)' }
else { throw 'the daemon tests pass as a standard user, but the user got no logon session: the Task Scheduler starting the task and the install command were NOT tested. This step proves the real session or fails.' }
