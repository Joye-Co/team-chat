<#
.SYNOPSIS
Makes this Windows computer one of the team's machines without the desktop app.

.DESCRIPTION
Downloads the machine daemon over HTTPS, checks it against the SHA-256 you give, writes the daemon config and the machine
token into a private folder (you, SYSTEM and Administrators; nothing inherited), and registers the daemon as a scheduled task of your
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
asset of a release. Must be https, and needs -Sha256.

.PARAMETER DaemonFile
A daemon that is already on this computer, instead of -DaemonUrl.

.PARAMETER Sha256
The SHA-256 the daemon must have (hex). Required with -DaemonUrl. Take it from where you trust
the release (the release page, your operator), never from the place the daemon is downloaded
from: whoever can change the download can change a hash next to it too. The script does not
fetch it.

.PARAMETER TokenFile
A file with the machine token, instead of typing it.

.PARAMETER Slug
The product's short name, when the daemon's file name does not start with `<slug>-daemon`.

.PARAMETER NoService
Only install the files: do not register or start the scheduled task.

.PARAMETER Update
Only replace the daemon program of an installed machine (no -ServerUrl, -MachineId or token).

.EXAMPLE
powershell -ExecutionPolicy Bypass -File install-daemon.ps1 -ServerUrl https://chat.example.com -MachineId 01ABC -DaemonUrl https://example.com/releases/x-daemon-1.3.0-x86_64-pc-windows-msvc.exe -Sha256 <hex>
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
if ($DaemonUrl) {
    if (([Uri]$DaemonUrl).Scheme -ne 'https') { throw '-DaemonUrl must be an https:// address.' }
    if (-not $Sha256) { throw '-DaemonUrl needs -Sha256: the hash the daemon must have, from a source you trust (not from the download location).' }
}
if ($Sha256 -and $Sha256 -notmatch '^[0-9a-fA-F]{64}$') { throw '-Sha256 must be 64 hex digits.' }
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
}
else {
    Copy-Item -LiteralPath $DaemonFile -Destination $new
}
# A daemon file you brought yourself is checked when you give its hash.
if ($Sha256) {
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $new).Hash
    if ($actual -ne $Sha256) {
        Remove-Item -LiteralPath $new -Force
        throw "The daemon's SHA-256 is $($actual.ToLower()), expected $($Sha256.ToLower()). Nothing was installed."
    }
}

# 2. In place. A running daemon cannot be overwritten, but it can be renamed: it deletes the
#    renamed file itself the next time it starts (the replace protocol of daemon::selfupdate).
#    If the computer loses power between the two renames, the scheduled task renames the old
#    file back before it starts the daemon.
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

# 3. A private config folder, before the token is written into it: nothing inherited from the
#    parent folder, and exactly three entries (you, SYSTEM, Administrators), which the files
#    made in it inherit. The same set the app writes and the daemon accepts.
New-Item -ItemType Directory -Force -Path $configDir | Out-Null
$acl = New-Object Security.AccessControl.DirectorySecurity
$acl.SetAccessRuleProtection($true, $false)
$inherit = [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
foreach ($who in @([Security.Principal.WindowsIdentity]::GetCurrent().User.Value, 'S-1-5-18', 'S-1-5-32-544')) {
    $sid = New-Object Security.Principal.SecurityIdentifier $who
    $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', $inherit, 'None', 'Allow')))
}
[IO.Directory]::SetAccessControl($configDir, $acl)

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
