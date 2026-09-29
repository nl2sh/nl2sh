[CmdletBinding()]
param(
    [ValidateSet("openrouter", "openai", "deepseek", "moonshot", "kimi", "siliconflow", "ollama", "custom")]
    [string]$Provider = "openrouter",
    [string]$Model = "",
    [string]$ApiKey = "",
    [string]$Endpoint = "",
    [string]$InstallDir = (Join-Path (Get-Location) "nl2sh-android"),
    [string]$Repository = "https://github.com/nl2sh/nl2sh",
    [switch]$KeepExistingConfig,
    [switch]$WebOnly
)

$ErrorActionPreference = "Stop"
$Repository = $Repository.TrimEnd('/')
if ([string]::IsNullOrEmpty($ApiKey)) { $ApiKey = $env:NL2SH_API_KEY }

$Defaults = @{
    openrouter  = @("https://openrouter.ai/api/v1", "openrouter/free")
    openai      = @("https://api.openai.com/v1", "gpt-4o-mini")
    deepseek    = @("https://api.deepseek.com", "deepseek-flash")
    moonshot    = @("https://api.moonshot.cn/v1", "kimi-k2-turbo-preview")
    kimi        = @("https://api.moonshot.cn/v1", "kimi-k2-turbo-preview")
    siliconflow = @("https://api.siliconflow.cn/v1", "Qwen/Qwen3-8B")
    ollama      = @("http://127.0.0.1:11434/v1", "qwen3")
    custom      = @("", "custom-model")
}
if ($Provider -eq "custom" -and [string]::IsNullOrWhiteSpace($Endpoint)) {
    throw "-Endpoint is required when -Provider is custom"
}
if ([string]::IsNullOrWhiteSpace($Endpoint)) { $Endpoint = $Defaults[$Provider][0] }
if ([string]::IsNullOrWhiteSpace($Model)) { $Model = $Defaults[$Provider][1] }
foreach ($Value in @($Endpoint, $Model, $ApiKey)) {
    if ($Value -match "[`r`n]") { throw "configuration values must not contain newlines" }
}

function ConvertTo-TomlBasicString([string]$Value) {
    return $Value.Replace('\', '\\').Replace('"', '\"')
}

function Set-TomlScalar([string]$Content, [string]$Name, [string]$Line) {
    $Pattern = '(?m)^' + [regex]::Escape($Name) + '\s*=.*(?:\r?\n|$)'
    if ([regex]::IsMatch($Content, $Pattern)) {
        return [regex]::Replace($Content, $Pattern, { param($Match) $Line + "`r`n" })
    }
    return $Line + "`r`n" + $Content
}

function Write-InstallerConfig([string]$Path, [bool]$MergeExisting) {
    $Content = if ($MergeExisting -and (Test-Path -LiteralPath $Path -PathType Leaf)) {
        [System.IO.File]::ReadAllText($Path)
    } else {
        ""
    }
    $Content = Set-TomlScalar $Content "model" ('model = "' + (ConvertTo-TomlBasicString $Model) + '"')
    $Content = Set-TomlScalar $Content "endpoint" ('endpoint = "' + (ConvertTo-TomlBasicString $Endpoint) + '"')
    $Content = Set-TomlScalar $Content "api_type" 'api_type = "auto"'
    if (-not $MergeExisting -or -not [string]::IsNullOrEmpty($ApiKey)) {
        $Content = Set-TomlScalar $Content "api_key" ('api_key = "' + (ConvertTo-TomlBasicString $ApiKey) + '"')
    }
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

$ExistingLauncher = Join-Path $InstallDir "android-run-windows.bat"
$ExistingArm64 = Join-Path $InstallDir "bin\arm64-v8a\nl2sh"
$ExistingArmv7 = Join-Path $InstallDir "bin\armeabi-v7a\nl2sh"
$ExistingConfig = Join-Path $InstallDir "config.toml"
if (Test-Path -LiteralPath $InstallDir) {
    if (-not (Test-Path -LiteralPath $InstallDir -PathType Container)) {
        throw "install path exists but is not a directory: $InstallDir"
    }
    $Complete = ((Test-Path -LiteralPath $ExistingLauncher -PathType Leaf) -and
        (Test-Path -LiteralPath $ExistingArm64 -PathType Leaf) -and
        (Test-Path -LiteralPath $ExistingArmv7 -PathType Leaf))
    if (-not $Complete) {
        throw "install directory exists but is incomplete: $InstallDir"
    }
    if (-not $KeepExistingConfig -or -not (Test-Path -LiteralPath $ExistingConfig -PathType Leaf)) {
        Write-InstallerConfig $ExistingConfig $true
        Write-Host "Updated existing provider configuration without replacing other settings."
    } else {
        Write-Host "Keeping the existing provider configuration."
    }
    Write-Host "Existing verified installation found: $InstallDir"
    $env:NL2SH_CONFIG_SOURCE = $ExistingConfig
    if ($WebOnly) { & $ExistingLauncher "--web-only" } else { & $ExistingLauncher }
    exit $LASTEXITCODE
}

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("nl2sh-install-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $TempDir | Out-Null
try {
    $Archive = Join-Path $TempDir "nl2sh-android.zip"
    $Sums = Join-Path $TempDir "SHA256SUMS"
    $RepositoryUri = [Uri]$Repository
    if ($RepositoryUri.Host -eq "gitee.com") {
        $RepositoryPath = $RepositoryUri.AbsolutePath.Trim('/')
        try {
            $Release = Invoke-RestMethod -UseBasicParsing -Uri "https://gitee.com/api/v5/repos/$RepositoryPath/releases/latest"
        } catch {
            throw "failed to query the latest Gitee release; ensure the mirror has synchronized release assets: $($_.Exception.Message)"
        }
        if ([string]::IsNullOrWhiteSpace($Release.tag_name) -or $Release.tag_name -notmatch '^[A-Za-z0-9._-]+$') {
            throw "the latest Gitee release returned an invalid or missing tag"
        }
        $DownloadBase = "$Repository/releases/download/$($Release.tag_name)"
    } else {
        $DownloadBase = "$Repository/releases/latest/download"
    }
    Write-Host "Downloading the latest nl2sh Android release from $Repository..."
    Invoke-WebRequest -UseBasicParsing -Uri "$DownloadBase/nl2sh-android.zip" -OutFile $Archive
    Invoke-WebRequest -UseBasicParsing -Uri "$DownloadBase/SHA256SUMS" -OutFile $Sums
    $ChecksumLine = Get-Content -LiteralPath $Sums | Where-Object { $_ -match '^[0-9a-fA-F]{64}\s+\*?nl2sh-android\.zip$' } | Select-Object -First 1
    if (-not $ChecksumLine) { throw "release checksum for nl2sh-android.zip is missing" }
    $Expected = ($ChecksumLine -split '\s+')[0]
    $Actual = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash
    if ($Actual -ne $Expected) { throw "downloaded ZIP checksum mismatch" }
    $Unpacked = Join-Path $TempDir "unpacked"
    Expand-Archive -LiteralPath $Archive -DestinationPath $Unpacked
    $PackageDir = Join-Path $Unpacked "nl2sh-android"
    if (-not (Test-Path -LiteralPath $PackageDir -PathType Container)) { throw "release ZIP has an unexpected layout" }
    Move-Item -LiteralPath $PackageDir -Destination $InstallDir

    $ConfigFile = Join-Path $InstallDir "config.toml"
    Write-InstallerConfig $ConfigFile $false
    Write-Host "Installed and verified: $InstallDir"
    $env:NL2SH_CONFIG_SOURCE = $ConfigFile
    if ($WebOnly) { & (Join-Path $InstallDir "android-run-windows.bat") "--web-only" } else { & (Join-Path $InstallDir "android-run-windows.bat") }
    exit $LASTEXITCODE
} finally {
    Remove-Item -LiteralPath $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
