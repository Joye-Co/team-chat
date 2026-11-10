<#
.SYNOPSIS
Makes this Windows computer one of the team's machines without the desktop app.

.DESCRIPTION
Downloads the machine daemon, checks its SHA-256, writes the daemon config and the machine
token into a folder only you can open, and registers the daemon as a scheduled task of your
user (it starts when you log on and is started again when it ends). Run it as the user whose
CLI logins (Claude Code, Codex) the bots should use, in Windows PowerShell 5.1 or later. It
needs no administrator rights.

The machine token is never a parameter: the script asks for it (the input is hidden), or
reads it from -TokenFile.

With -Update it only replaces the daemon program: the running program is renamed aside, the
new one takes its place, and the daemon restarts itself on the new version once it has no
running work. The config, the token and the scheduled task stay as they are.

.PARAMETER ServerUrl
The server, for example https://chat.example.com.

.PARAMETER MachineId
The id the server gave the machine when it was registered.

.PARAMETER Name
The machine's name in the team. Default: this computer's name.

.PARAMETER Labels
The machine's labels. Default: windows.

.PARAMETER DaemonUrl
Where to download the daemon from: the `<slug>-daemon-<version>-x86_64-pc-windows-msvc.exe`
asset of a release. Its `.sha256` file is read from the same place unless -Sha256 is given.

.PARAMETER DaemonFile
A daemon that is already on this computer, instead of -DaemonUrl.

.PARAMETER Sha256
The SHA-256 the daemon must have (hex).

.PARAMETER TokenFile
A file with the machine token, instead of typing it.

.PARAMETER Slug
The product's short name, when the daemon's file name does not start with `<slug>-daemon`.

.PARAMETER NoService
Only install the files: do not register or start the scheduled task.

.PARAMETER Update
Only replace the daemon program of an installed machine (no -ServerUrl, -MachineId or token).

.EXAMPLE
powershell -ExecutionPolicy Bypass -File install-daemon.ps1 -ServerUrl https://chat.example.com -MachineId 01ABC -DaemonUrl https://example.com/releases/x-daemon-1.3.0-x86_64-pc-windows-msvc.exe
#>
[CmdletBinding()]
param(
    [string]$ServerUrl,
    [string]$MachineId,
    [string]$Name = $env:COMPUTERNAME,
    [string[]]$Labels = @('windows'),
    [string]$DaemonUrl,
    [string]$DaemonFile,
    [string]$Sha256,
    [string]$TokenFile,
    [string]$Slug,
    [switch]$NoService,
    [switch]$Update
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function Write-TextFile([string]$Path, [string]$Text) {
    # UTF-8 without a byte order mark, and no line end added.
    [IO.File]::WriteAllText($Path, $Text, (New-Object Text.UTF8Encoding $false))
}

function ConvertTo-TomlString([string]$Value) {
    $out = New-Object Text.StringBuilder
    [void]$out.Append('"')
    foreach ($c in $Value.ToCharArray()) {
        if ($c -eq '"') { [void]$out.Append('\"') }
        elseif ($c -eq '\') { [void]$out.Append('\\') }
        elseif ([int]$c -lt 32 -or [int]$c -eq 127) { [void]$out.Append(('\u{0:X4}' -f [int]$c)) }
        else { [void]$out.Append($c) }
    }
    [void]$out.Append('"')
    $out.ToString()
}

function Get-FirstWord([string]$Text) {
    ($Text.Trim() -split '\s+')[0]
}

if (-not $Update -and (-not $ServerUrl -or -not $MachineId)) {
    throw 'Give -ServerUrl and -MachineId (or -Update to only replace the daemon program).'
}
if ([bool]$DaemonUrl -eq [bool]$DaemonFile) {
    throw 'Give either -DaemonUrl or -DaemonFile.'
}
if ($DaemonUrl) { $sourceName = ([Uri]$DaemonUrl).Segments[-1] } else { $sourceName = Split-Path -Leaf $DaemonFile }
if (-not $Slug) {
    if ($sourceName -match '^(.+?)-daemon') { $Slug = $Matches[1] }
    else { throw "Cannot tell the product from the file name '$sourceName': give -Slug." }
}
if ($Slug -notmatch '^[a-z0-9][a-z0-9-]*$') { throw "Not a product slug: $Slug" }

$configDir = Join-Path (Join-Path $env:USERPROFILE '.config') $Slug
$binDir = Join-Path (Join-Path $env:LOCALAPPDATA 'Programs') $Slug
$exe = Join-Path $binDir "$Slug-daemon.exe"
$new = "$exe.new"
$tokenPath = Join-Path $configDir 'machine-token'
$configPath = Join-Path $configDir 'daemon.toml'

# 1. The daemon, complete and checked, under another name next to where it will live.
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
if (Test-Path -LiteralPath $new) { Remove-Item -LiteralPath $new -Force }
if ($DaemonUrl) {
    Write-Host "Downloading $sourceName"
    Invoke-WebRequest -UseBasicParsing -Uri $DaemonUrl -OutFile $new
    if (-not $Sha256) {
        $sum = Invoke-WebRequest -UseBasicParsing -Uri "$DaemonUrl.sha256"
        $content = $sum.Content
        if ($content -is [byte[]]) { $content = [Text.Encoding]::ASCII.GetString($content) }
        $Sha256 = Get-FirstWord $content
    }
}
else {
    Copy-Item -LiteralPath $DaemonFile -Destination $new
    if (-not $Sha256 -and (Test-Path -LiteralPath "$DaemonFile.sha256")) {
        $Sha256 = Get-FirstWord (Get-Content -Raw -LiteralPath "$DaemonFile.sha256")
    }
}
if ($Sha256 -notmatch '^[0-9a-fA-F]{64}$') {
    Remove-Item -LiteralPath $new -Force
    throw 'No SHA-256 to check the daemon against: give -Sha256.'
}
$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $new).Hash
if ($actual -ne $Sha256) {
    Remove-Item -LiteralPath $new -Force
    throw "The daemon's SHA-256 is $($actual.ToLower()), expected $($Sha256.ToLower()). Nothing was installed."
}

# 2. In place. A running daemon cannot be overwritten, but it can be renamed: it deletes the
#    renamed file itself the next time it starts (the replace protocol of daemon::selfupdate).
if (Test-Path -LiteralPath $exe) {
    $aside = "$exe.old-$([DateTime]::UtcNow.Ticks)"
    Move-Item -LiteralPath $exe -Destination $aside
    try { Move-Item -LiteralPath $new -Destination $exe }
    catch { Move-Item -LiteralPath $aside -Destination $exe; throw }
}
else {
    Move-Item -LiteralPath $new -Destination $exe
}
Write-Host "Installed $exe"
if ($Update) {
    Write-Host 'A running daemon restarts on this version by itself once it has no running work.'
    return
}

# 3. A config folder only this user can open, before the token is written into it.
New-Item -ItemType Directory -Force -Path $configDir | Out-Null
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$icacls = Join-Path $env:SystemRoot 'System32\icacls.exe'
& $icacls $configDir /inheritance:r /grant:r "*${sid}:(OI)(CI)F" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Could not restrict $configDir to your user (icacls exit code $LASTEXITCODE)." }

# 4. The token: from a file, or typed without showing it. Never printed.
if ($TokenFile) {
    $token = (Get-Content -Raw -LiteralPath $TokenFile).Trim()
}
else {
    $secure = Read-Host -AsSecureString 'Machine token'
    $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try { $token = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr).Trim() }
    finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
}
if (-not $token) { throw 'The machine token is empty.' }
Write-TextFile $tokenPath $token
$token = $null

# 5. The config, the same keys the app writes.
$labelList = ($Labels | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ } | ForEach-Object { ConvertTo-TomlString $_ }) -join ', '
$config = @(
    "server_url = $(ConvertTo-TomlString $ServerUrl)"
    "token_file = $(ConvertTo-TomlString $tokenPath)"
    "machine_id = $(ConvertTo-TomlString $MachineId)"
    "name = $(ConvertTo-TomlString $Name)"
    "labels = [$labelList]"
    ''
) -join "`n"
Write-TextFile $configPath $config
Write-Host "Wrote $configPath"

# 6. What the daemon finds on this computer, then the scheduled task.
& $exe doctor --config $configPath
if ($LASTEXITCODE -ne 0) {
    Write-Warning 'doctor found problems (see above). The daemon is installed; fix them and run this again, or run doctor yourself.'
}
if ($NoService) {
    Write-Host "Not registered as a scheduled task. To do it: & '$exe' install-service --config '$configPath'"
    return
}
& $exe install-service --config $configPath
if ($LASTEXITCODE -ne 0) { throw "install-service failed (exit code $LASTEXITCODE)." }
Write-Host "The daemon runs as a scheduled task of $env:USERNAME and starts when you log on."
Write-Host "Log: $(Join-Path (Join-Path $env:LOCALAPPDATA "$Slug-daemon") 'daemon.log')"
