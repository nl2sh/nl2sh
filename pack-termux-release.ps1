$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false

$ProjectDir = $PSScriptRoot
$DistDir = Join-Path $ProjectDir "dist"
$ApiLevel = if ($env:ANDROID_API_LEVEL) { $env:ANDROID_API_LEVEL } else { "26" }
$NdkDir = if ($env:ANDROID_NDK_HOME) { $env:ANDROID_NDK_HOME } elseif ($env:ANDROID_NDK_ROOT) { $env:ANDROID_NDK_ROOT } else { $null }

foreach ($Command in @("cargo", "rustup", "wsl.exe")) {
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) { throw "$Command was not found in PATH" }
}
if ([string]::IsNullOrWhiteSpace($NdkDir)) { throw "set ANDROID_NDK_HOME or ANDROID_NDK_ROOT" }
if (-not (Test-Path -LiteralPath $NdkDir -PathType Container)) { throw "Android NDK directory does not exist: $NdkDir" }
if ($ApiLevel -notmatch '^\d+$' -or [int]$ApiLevel -lt 26) { throw "ANDROID_API_LEVEL must be an integer greater than or equal to 26: $ApiLevel" }

& wsl.exe --exec sh -lc "command -v bash >/dev/null && command -v dpkg-deb >/dev/null"
if ($LASTEXITCODE -ne 0) { throw "the default WSL distribution must provide bash and dpkg-deb" }

function Convert-ToWslPath([string]$WindowsPath) {
    $Converted = (& wsl.exe --exec wslpath -a $WindowsPath | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($Converted)) {
        throw "failed to convert Windows path for WSL: $WindowsPath"
    }
    return $Converted
}

$Toolchain = Join-Path $NdkDir "toolchains\llvm\prebuilt\windows-x86_64"
$Ar = Join-Path $Toolchain "bin\llvm-ar.exe"
$Targets = @(
    @{ Rust = "aarch64-linux-android"; Clang = "aarch64-linux-android"; Prefix = "AARCH64_LINUX_ANDROID"; Suffix = "aarch64_linux_android"; Termux = "aarch64" },
    @{ Rust = "armv7-linux-androideabi"; Clang = "armv7a-linux-androideabi"; Prefix = "ARMV7_LINUX_ANDROIDEABI"; Suffix = "armv7_linux_androideabi"; Termux = "arm" },
    @{ Rust = "x86_64-linux-android"; Clang = "x86_64-linux-android"; Prefix = "X86_64_LINUX_ANDROID"; Suffix = "x86_64_linux_android"; Termux = "x86_64" }
)

$InstalledTargets = @(& rustup target list --installed)
if ($LASTEXITCODE -ne 0) { throw "failed to list installed Rust targets" }
foreach ($Target in $Targets) {
    if ($InstalledTargets -notcontains $Target.Rust) { throw "Rust target is missing. Run: rustup target add $($Target.Rust)" }
}

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
$WslProjectDir = Convert-ToWslPath $ProjectDir
$WslDistDir = Convert-ToWslPath $DistDir
$WslPackager = "$WslProjectDir/packaging/termux/build-deb.sh"

foreach ($Target in $Targets) {
    $Clang = Join-Path $Toolchain "bin\$($Target.Clang)$ApiLevel-clang.cmd"
    if (-not (Test-Path -LiteralPath $Clang -PathType Leaf) -or -not (Test-Path -LiteralPath $Ar -PathType Leaf)) {
        throw "NDK LLVM tools were not found under $Toolchain"
    }

    Set-Item -LiteralPath "Env:CARGO_TARGET_$($Target.Prefix)_LINKER" -Value $Clang
    Set-Item -LiteralPath "Env:CARGO_TARGET_$($Target.Prefix)_AR" -Value $Ar
    Set-Item -LiteralPath "Env:CC_$($Target.Suffix)" -Value $Clang
    Set-Item -LiteralPath "Env:AR_$($Target.Suffix)" -Value $Ar

    Write-Host "Building $($Target.Rust) for Termux..."
    Push-Location $ProjectDir
    try {
        & cargo build --locked --release --target $Target.Rust --no-default-features
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $($Target.Rust)" }
    } finally {
        Pop-Location
    }

    $Binary = Join-Path $ProjectDir "target\$($Target.Rust)\release\nl2sh"
    if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { throw "compiled binary was not found: $Binary" }
    $WslBinary = Convert-ToWslPath $Binary

    Write-Host "Packaging $($Target.Termux) with WSL dpkg-deb..."
    & wsl.exe --exec bash -lc 'cd "$1" && exec bash "$2" "$3" "$4" "$5"' bash `
        $WslProjectDir $WslPackager $Target.Termux $WslBinary $WslDistDir
    if ($LASTEXITCODE -ne 0) { throw "WSL dpkg-deb failed for $($Target.Termux)" }
}

Write-Host "Created Termux packages:"
Get-ChildItem -LiteralPath $DistDir -File | Where-Object {
    $_.Name -match '^nl2sh_.+_(aarch64|arm|x86_64)\.deb$'
} | ForEach-Object { Write-Host $_.FullName }
