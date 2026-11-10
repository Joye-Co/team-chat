<#
.SYNOPSIS
Checks scripts/install-daemon.ps1 with a real daemon and the real Task Scheduler. For CI.

.DESCRIPTION
Installs the daemon just built as a scheduled task of the current user (the server is an
address nothing listens on, the daemon keeps trying), then checks: a wrong SHA-256 installs
nothing; the config folder is open to this user alone; the task runs the daemon, which writes
its log; an update by the replace protocol makes the running daemon restart on the new file
and delete the old one; uninstall leaves no task and no process. Everything it made is
removed at the end. It refuses to run on a computer that already has a daemon config.
#>
[CmdletBinding()]
param(
    [string]$Daemon = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\debug\daemon.exe')
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

function Assert([bool]$Condition, [string]$What) {
    if (-not $Condition) { throw "FAILED: $What" }
    Write-Host "ok: $What"
}

function Wait-For([int]$Seconds, [string]$What, [scriptblock]$Done) {
    $start = Get-Date
    while (-not (& $Done)) {
        if (((Get-Date) - $start).TotalSeconds -gt $Seconds) {
            if (Test-Path -LiteralPath $script:log) { Write-Host '--- daemon.log'; Get-Content -LiteralPath $script:log | Select-Object -Last 40 }
            throw "FAILED: $What (not within $Seconds s)"
        }
        Start-Sleep -Milliseconds 500
    }
    Write-Host ("ok: {0} (after {1:n0} s)" -f $What, ((Get-Date) - $start).TotalSeconds)
}

$root = Split-Path -Parent $PSScriptRoot
$product = Get-Content -Raw -LiteralPath (Join-Path $root 'product.json') | ConvertFrom-Json
$slug = $product.slug
$taskName = "$($product.bundleId).daemon"
$install = Join-Path $PSScriptRoot 'install-daemon.ps1'
$configDir = Join-Path (Join-Path $env:USERPROFILE '.config') $slug
$binDir = Join-Path (Join-Path $env:LOCALAPPDATA 'Programs') $slug
$stateDir = Join-Path $env:LOCALAPPDATA "$slug-daemon"
$exe = Join-Path $binDir "$slug-daemon.exe"
$script:log = Join-Path $stateDir 'daemon.log'
$schtasks = Join-Path $env:SystemRoot 'System32\schtasks.exe'

if ((Test-Path -LiteralPath $configDir) -or (Test-Path -LiteralPath $exe)) {
    throw "This computer already has a daemon ($configDir or $exe): not touching it."
}
Assert (Test-Path -LiteralPath $Daemon) "the daemon is built at $Daemon"

$work = Join-Path ([IO.Path]::GetTempPath()) ("install-daemon-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$asset = Join-Path $work "$slug-daemon-0.0.0-x86_64-pc-windows-msvc.exe"
Copy-Item -LiteralPath $Daemon -Destination $asset
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $asset).Hash.ToLower()
[IO.File]::WriteAllText("$asset.sha256", "$hash  $(Split-Path -Leaf $asset)`n")
$token = 'tm_test_' + [guid]::NewGuid().ToString('N')
$tokenFile = Join-Path $work 'token'
[IO.File]::WriteAllText($tokenFile, "$token`n")
$common = @{
    ServerUrl  = 'http://127.0.0.1:9'
    MachineId  = '01TESTMACHINE'
    Name       = 'ci "runner" \ one'
    Labels     = 'windows', 'ci'
    DaemonFile = $asset
    TokenFile  = $tokenFile
}

try {
    # A daemon that is not the one expected is not installed.
    $refused = $false
    try { & $install @common -Sha256 ('0' * 64) } catch { $refused = $true }
    Assert $refused 'a wrong SHA-256 stops the install'
    Assert (-not (Test-Path -LiteralPath $exe)) 'and nothing was installed'
    Assert (-not (Test-Path -LiteralPath $configDir)) 'and no token was written'

    # The install: the SHA-256 comes from the .sha256 file next to the daemon.
    & $install @common
    Assert (Test-Path -LiteralPath $exe) 'the daemon is installed'
    $config = Get-Content -Raw -LiteralPath (Join-Path $configDir 'daemon.toml')
    Assert ($config.Contains('machine_id = "01TESTMACHINE"')) 'the config names the machine'
    Assert ($config.Contains('name = "ci \"runner\" \\ one"')) 'the config quotes the name'
    Assert ($config.Contains('labels = ["windows", "ci"]')) 'the config has the labels'
    Assert ([IO.File]::ReadAllText((Join-Path $configDir 'machine-token')) -eq $token) 'the token file holds the token'

    # Only this user may open the config folder and the token in it.
    $me = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    foreach ($path in @($configDir, (Join-Path $configDir 'machine-token'), (Join-Path $configDir 'daemon.toml'))) {
        $acl = Get-Acl -LiteralPath $path
        $who = @($acl.Access | ForEach-Object { $_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value } | Sort-Object -Unique)
        Assert ($who.Count -eq 1 -and $who[0] -eq $me) "only this user is on the access list of $(Split-Path -Leaf $path)"
    }
    Assert ((Get-Acl -LiteralPath $configDir).AreAccessRulesProtected) 'the config folder inherits no access from its parent'

    # The scheduled task runs the daemon, hidden, and the daemon writes its log.
    & $schtasks /Query /TN $taskName | Out-Null
    Assert ($LASTEXITCODE -eq 0) "the task $taskName is registered"
    Wait-For 60 'the daemon started and writes its log' {
        (Test-Path -LiteralPath $script:log) -and ((Get-Content -Raw -LiteralPath $script:log) -match 'daemon started')
    }
    $first = @(Get-Process -Name "$slug-daemon" -ErrorAction SilentlyContinue)
    Assert ($first.Count -eq 1) 'one daemon process runs'
    Assert ($first[0].MainWindowHandle -eq [IntPtr]::Zero) 'it has no window'

    # An update by the replace protocol, while the daemon runs: it restarts itself on the new
    # file (it looks every minute and wants the file unchanged between two looks), and the
    # task starts it again within a minute.
    & $install -Update -DaemonFile $asset
    Assert (@(Get-ChildItem -LiteralPath $binDir -Filter '*.old-*').Count -eq 1) 'the running program was renamed aside'
    Wait-For 300 'the daemon saw the new version and stopped' {
        (Get-Content -Raw -LiteralPath $script:log) -match 'a new version of the daemon is installed'
    }
    Wait-For 120 'the task started the new version' {
        $now = @(Get-Process -Name "$slug-daemon" -ErrorAction SilentlyContinue)
        $now.Count -eq 1 -and $now[0].Id -ne $first[0].Id -and
            ([regex]::Matches((Get-Content -Raw -LiteralPath $script:log), 'daemon started').Count -ge 2)
    }
    Wait-For 30 'the new version deleted the old program' {
        @(Get-ChildItem -LiteralPath $binDir -Filter '*.old-*').Count -eq 0
    }
    Assert (-not ((Get-Content -Raw -LiteralPath $script:log).Contains($token))) 'the token is not in the log'

    # Uninstall: no task, no process.
    & $exe uninstall-service
    Assert ($LASTEXITCODE -eq 0) 'uninstall-service worked'
    $ErrorActionPreference = 'Continue'
    & $schtasks /Query /TN $taskName 2>&1 | Out-Null
    $gone = $LASTEXITCODE -ne 0
    $ErrorActionPreference = 'Stop'
    Assert $gone 'the task is gone'
    Wait-For 30 'the daemon ended' {
        @(Get-Process -Name "$slug-daemon" -ErrorAction SilentlyContinue).Count -eq 0
    }
    Write-Host 'install-daemon.ps1: all checks passed'
}
finally {
    $ErrorActionPreference = 'Continue'
    if (Test-Path -LiteralPath $exe) { & $exe uninstall-service 2>&1 | Out-Null }
    Get-Process -Name "$slug-daemon" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    foreach ($dir in @($configDir, $binDir, $stateDir, $work)) {
        if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue }
    }
}
