$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false
Add-Type -AssemblyName System.IO.Compression.FileSystem

$HelperRoot = $PSScriptRoot
$PackagedJar = Join-Path $HelperRoot "app\build\distributions\jadx-helper.jar"
$DistDir = Join-Path $HelperRoot "dist"
$Jar = Join-Path $DistDir "jadx-helper.jar"
$GradleCommand = Join-Path $HelperRoot "gradlew.bat"
if (-not (Test-Path -LiteralPath $GradleCommand -PathType Leaf)) {
    throw "Gradle Wrapper was not found: $GradleCommand"
}

& $GradleCommand --project-dir $HelperRoot :app:packageHelper
if ($LASTEXITCODE -ne 0) { throw "Gradle release build failed" }
if (-not (Test-Path -LiteralPath $PackagedJar -PathType Leaf)) {
    throw "packaged helper was not found after the Gradle build: $PackagedJar"
}

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
Copy-Item -LiteralPath $PackagedJar -Destination $Jar -Force

function Test-ByteSequence {
    param(
        [Parameter(Mandatory = $true)][byte[]]$Bytes,
        [Parameter(Mandatory = $true)][byte[]]$Sequence
    )

    if ($Sequence.Length -eq 0 -or $Bytes.Length -lt $Sequence.Length) { return $false }
    $LastStart = $Bytes.Length - $Sequence.Length
    for ($Start = 0; $Start -le $LastStart; $Start++) {
        if ($Bytes[$Start] -ne $Sequence[0]) { continue }
        $Matches = $true
        for ($Offset = 1; $Offset -lt $Sequence.Length; $Offset++) {
            if ($Bytes[$Start + $Offset] -ne $Sequence[$Offset]) {
                $Matches = $false
                break
            }
        }
        if ($Matches) { return $true }
    }
    return $false
}

$Archive = $null
try {
    $Archive = [System.IO.Compression.ZipFile]::OpenRead($Jar)
    $DexEntry = $Archive.GetEntry("classes.dex")
    if ($null -eq $DexEntry) { throw "helper APK is missing classes.dex" }
    if ($null -ne ($Archive.Entries | Where-Object { $_.FullName.StartsWith("classes2.dex", [System.StringComparison]::Ordinal) } | Select-Object -First 1)) {
        throw "multidex helper is unsupported by the app_process loader"
    }

    $DexStream = $DexEntry.Open()
    $Memory = New-Object System.IO.MemoryStream
    try {
        $DexStream.CopyTo($Memory)
        $Dex = $Memory.ToArray()
    } finally {
        $Memory.Dispose()
        $DexStream.Dispose()
    }

    $DexMagic = [System.Text.Encoding]::ASCII.GetBytes("dex`n")
    $Entrypoint = [System.Text.Encoding]::ASCII.GetBytes("Lcom/nl2sh/jadx/Main;")
    $HasDexMagic = $Dex.Length -ge $DexMagic.Length
    for ($Index = 0; $HasDexMagic -and $Index -lt $DexMagic.Length; $Index++) {
        if ($Dex[$Index] -ne $DexMagic[$Index]) { $HasDexMagic = $false }
    }
    if (-not $HasDexMagic -or -not (Test-ByteSequence -Bytes $Dex -Sequence $Entrypoint)) {
        throw "helper APK is missing the DEX entrypoint"
    }
} finally {
    if ($null -ne $Archive) { $Archive.Dispose() }
}

$Hash = (Get-FileHash -LiteralPath $Jar -Algorithm SHA256).Hash.ToLowerInvariant()
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText(
    (Join-Path $DistDir "jadx-helper.jar.sha256"),
    "$Hash  jadx-helper.jar`n",
    [System.Text.Encoding]::ASCII
)
$Metadata = [ordered]@{
    helper_version = "0.1.0"
    jadx_version = "1.5.1"
    min_android_api = 26
    entrypoint = "com.nl2sh.jadx.Main"
    sha256 = $Hash
    size_bytes = (Get-Item -LiteralPath $Jar).Length
} | ConvertTo-Json
[System.IO.File]::WriteAllText(
    (Join-Path $DistDir "metadata.json"),
    "$Metadata`n",
    $Utf8NoBom
)

Write-Host $Jar
Write-Host $Hash
