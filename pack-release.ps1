$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false

$ProjectDir = $PSScriptRoot
$PackageName = "nl2sh-android"
$DistDir = Join-Path $ProjectDir "dist"
$StagingRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("nl2sh-release-" + [guid]::NewGuid().ToString("N"))
$PackageDir = Join-Path $StagingRoot $PackageName
$ApiLevel = if ($env:ANDROID_API_LEVEL) { $env:ANDROID_API_LEVEL } else { "26" }
$NdkDir = if ($env:ANDROID_NDK_HOME) { $env:ANDROID_NDK_HOME } elseif ($env:ANDROID_NDK_ROOT) { $env:ANDROID_NDK_ROOT } else { $null }

foreach ($Command in @("cargo", "rustup")) {
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) { throw "$Command was not found in PATH" }
}
if ([string]::IsNullOrWhiteSpace($NdkDir)) { throw "set ANDROID_NDK_HOME or ANDROID_NDK_ROOT" }
if (-not (Test-Path -LiteralPath $NdkDir -PathType Container)) { throw "Android NDK directory does not exist: $NdkDir" }
if ($ApiLevel -notmatch '^\d+$' -or [int]$ApiLevel -lt 26) { throw "ANDROID_API_LEVEL must be an integer greater than or equal to 26: $ApiLevel" }

$Toolchain = Join-Path $NdkDir "toolchains\llvm\prebuilt\windows-x86_64"
$Ar = Join-Path $Toolchain "bin\llvm-ar.exe"
$Targets = @(
    @{ Rust = "aarch64-linux-android"; Clang = "aarch64-linux-android"; Prefix = "AARCH64_LINUX_ANDROID"; Suffix = "aarch64_linux_android"; Abi = "arm64-v8a" },
    @{ Rust = "armv7-linux-androideabi"; Clang = "armv7a-linux-androideabi"; Prefix = "ARMV7_LINUX_ANDROIDEABI"; Suffix = "armv7_linux_androideabi"; Abi = "armeabi-v7a" }
)

$InstalledTargets = @(& rustup target list --installed)
if ($LASTEXITCODE -ne 0) { throw "failed to list installed Rust targets" }
foreach ($Target in $Targets) {
    if ($InstalledTargets -notcontains $Target.Rust) { throw "Rust target is missing. Run: rustup target add $($Target.Rust)" }
}

try {
    foreach ($Target in $Targets) {
        $Clang = Join-Path $Toolchain "bin\$($Target.Clang)$ApiLevel-clang.cmd"
        if (-not (Test-Path -LiteralPath $Clang -PathType Leaf) -or -not (Test-Path -LiteralPath $Ar -PathType Leaf)) { throw "NDK LLVM tools were not found under $Toolchain" }
        Set-Item -LiteralPath "Env:CARGO_TARGET_$($Target.Prefix)_LINKER" -Value $Clang
        Set-Item -LiteralPath "Env:CARGO_TARGET_$($Target.Prefix)_AR" -Value $Ar
        Set-Item -LiteralPath "Env:CC_$($Target.Suffix)" -Value $Clang
        Set-Item -LiteralPath "Env:AR_$($Target.Suffix)" -Value $Ar
        Write-Host "Building $($Target.Rust)..."
        Push-Location $ProjectDir
        try {
            & cargo build --release --target $Target.Rust
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $($Target.Rust)" }
        } finally { Pop-Location }
    }

    New-Item -ItemType Directory -Force -Path (Join-Path $PackageDir "bin\arm64-v8a"), (Join-Path $PackageDir "bin\armeabi-v7a"), $DistDir | Out-Null
    Copy-Item -LiteralPath (Join-Path $ProjectDir "target\aarch64-linux-android\release\nl2sh") -Destination (Join-Path $PackageDir "bin\arm64-v8a\nl2sh")
    Copy-Item -LiteralPath (Join-Path $ProjectDir "target\armv7-linux-androideabi\release\nl2sh") -Destination (Join-Path $PackageDir "bin\armeabi-v7a\nl2sh")
    foreach ($File in @("android-run-linux.sh", "android-run-windows.bat", "config.toml.example", "使用说明.md")) {
        Copy-Item -LiteralPath (Join-Path $ProjectDir $File) -Destination $PackageDir
    }
    Copy-Item -LiteralPath (Join-Path $ProjectDir "screenshots") -Destination (Join-Path $PackageDir "screenshots") -Recurse

    $Archive = Join-Path $DistDir "$PackageName.zip"
    Compress-Archive -LiteralPath $PackageDir -DestinationPath $Archive -CompressionLevel Optimal -Force
    $Hash = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath (Join-Path $DistDir "SHA256SUMS") -Value "$Hash  $PackageName.zip" -Encoding ascii
    Write-Host "Created: $Archive"
    Write-Host "Checksum: $(Join-Path $DistDir 'SHA256SUMS')"
} finally {
    if (Test-Path -LiteralPath $StagingRoot) { Remove-Item -LiteralPath $StagingRoot -Recurse -Force }
}
