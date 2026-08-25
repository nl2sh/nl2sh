$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false

$ProjectDir = $PSScriptRoot
$ApiLevel = if ($env:ANDROID_API_LEVEL) { $env:ANDROID_API_LEVEL } else { "26" }
$AndroidDir = if ($env:ANDROID_DIR) { $env:ANDROID_DIR } else { "/data/local/tmp" }
$RemoteBinary = "$AndroidDir/nl2sh"
$NdkDir = if ($env:ANDROID_NDK_HOME) { $env:ANDROID_NDK_HOME } elseif ($env:ANDROID_NDK_ROOT) { $env:ANDROID_NDK_ROOT } else { $null }
$AdbArgs = @()

foreach ($Command in @("adb", "cargo", "rustup")) {
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) { throw "$Command was not found in PATH" }
}
if ([string]::IsNullOrWhiteSpace($NdkDir)) { throw "set ANDROID_NDK_HOME or ANDROID_NDK_ROOT" }
if (-not (Test-Path -LiteralPath $NdkDir -PathType Container)) { throw "Android NDK directory does not exist: $NdkDir" }
if ($ApiLevel -notmatch '^\d+$' -or [int]$ApiLevel -lt 26) { throw "ANDROID_API_LEVEL must be an integer greater than or equal to 26: $ApiLevel" }
if ($AndroidDir -notmatch '^/[A-Za-z0-9._/-]+$') { throw "ANDROID_DIR must be a safe absolute Android path: $AndroidDir" }
if ($env:ADB_SERIAL) {
    $DeviceState = (& adb -s $env:ADB_SERIAL get-state 2>$null | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $DeviceState -ne "device") { throw "ADB_SERIAL is not a usable device: $($env:ADB_SERIAL)" }
    $SelectedSerial = $env:ADB_SERIAL
} else {
    function Get-AdbDevices {
        $Lines = @(& adb devices 2>$null)
        return @($Lines | Select-Object -Skip 1 | ForEach-Object {
            if ($_ -match '^(\S+)\s+device\s*$') { $Matches[1] }
        })
    }
    $Devices = @(Get-AdbDevices)
    if ($Devices.Count -eq 0) {
        Write-Host "No connected ADB device was found."
        $DeviceIp = Read-Host "Enter Android device IP or IP:port"
        if ([string]::IsNullOrWhiteSpace($DeviceIp)) { throw "no IP address was entered" }
        & adb connect $DeviceIp
        if ($LASTEXITCODE -ne 0) { throw "adb connect failed: $DeviceIp" }
        $Devices = @(Get-AdbDevices)
    }
    if ($Devices.Count -eq 0) { throw "no usable ADB device is connected" }
    if ($Devices.Count -eq 1) {
        $SelectedSerial = $Devices[0]
    } else {
        Write-Host "Multiple ADB devices are connected:"
        for ($Index = 0; $Index -lt $Devices.Count; $Index++) { Write-Host "  $($Index + 1). $($Devices[$Index])" }
        $Choice = Read-Host "Enter device number"
        if ($Choice -notmatch '^[1-9][0-9]*$' -or [int]$Choice -gt $Devices.Count) { throw "invalid device number" }
        $SelectedSerial = $Devices[[int]$Choice - 1]
    }
}
$AdbArgs = @("-s", $SelectedSerial)
Write-Host "Selected device: $SelectedSerial"

$AbiList = (& adb @AdbArgs shell getprop ro.product.cpu.abilist 2>$null | Out-String).Trim()
if ([string]::IsNullOrWhiteSpace($AbiList)) { $AbiList = (& adb @AdbArgs shell getprop ro.product.cpu.abi 2>$null | Out-String).Trim() }
Write-Host "Device ABI: $AbiList"
if (($AbiList -split ',') -contains "arm64-v8a") { $DetectedTarget = "aarch64-linux-android" }
elseif (($AbiList -split ',') -contains "armeabi-v7a") { $DetectedTarget = "armv7-linux-androideabi" }
else { throw "unsupported device ABI '$AbiList'; supported ABIs are arm64-v8a and armeabi-v7a" }
if ($env:RUST_TARGET -and $env:RUST_TARGET -ne $DetectedTarget) { throw "RUST_TARGET=$($env:RUST_TARGET) does not match device ABI $AbiList ($DetectedTarget)" }
$Target = if ($env:RUST_TARGET) { $env:RUST_TARGET } else { $DetectedTarget }
$LocalBinary = Join-Path $ProjectDir "target\$Target\release\nl2sh"
Write-Host "Selected Rust target: $Target"

switch ($Target) {
    "aarch64-linux-android" { $ClangTarget = "aarch64-linux-android"; $CargoPrefix = "AARCH64_LINUX_ANDROID"; $CcSuffix = "aarch64_linux_android" }
    "armv7-linux-androideabi" { $ClangTarget = "armv7a-linux-androideabi"; $CargoPrefix = "ARMV7_LINUX_ANDROIDEABI"; $CcSuffix = "armv7_linux_androideabi" }
    default { throw "unsupported Rust target: $Target" }
}

$Toolchain = Join-Path $NdkDir "toolchains\llvm\prebuilt\windows-x86_64"
$Clang = Join-Path $Toolchain "bin\$ClangTarget$ApiLevel-clang.cmd"
$Ar = Join-Path $Toolchain "bin\llvm-ar.exe"
if (-not (Test-Path -LiteralPath $Clang -PathType Leaf) -or -not (Test-Path -LiteralPath $Ar -PathType Leaf)) { throw "NDK LLVM tools were not found under $Toolchain" }

$InstalledTargets = @(& rustup target list --installed)
if ($LASTEXITCODE -ne 0) { throw "failed to list installed Rust targets" }
if ($InstalledTargets -notcontains $Target) { throw "Rust target is missing. Run: rustup target add $Target" }

$AdbIsRoot = $false
Write-Host "Restarting adbd with root privileges..."
$AdbRootOutput = (& adb @AdbArgs root 2>&1 | Out-String).Trim()
if ($AdbRootOutput) { Write-Host $AdbRootOutput }
& adb @AdbArgs wait-for-device
if ($LASTEXITCODE -ne 0) { throw "adb device did not reconnect after adb root" }
$DeviceUid = (& adb @AdbArgs shell id -u 2>$null | Out-String).Trim()
if ($LASTEXITCODE -eq 0 -and $DeviceUid -eq "0") { $AdbIsRoot = $true; Write-Host "adbd is running as root." } else { Write-Warning "adb root is unsupported or was denied; adbd remains non-root." }

# cc-rs builds native dependencies separately from Cargo's final link step.
Set-Item -LiteralPath "Env:CARGO_TARGET_${CargoPrefix}_LINKER" -Value $Clang
Set-Item -LiteralPath "Env:CARGO_TARGET_${CargoPrefix}_AR" -Value $Ar
Set-Item -LiteralPath "Env:CC_${CcSuffix}" -Value $Clang
Set-Item -LiteralPath "Env:AR_${CcSuffix}" -Value $Ar

Push-Location $ProjectDir
try {
    & cargo build --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $Target" }
} finally { Pop-Location }
if (-not (Test-Path -LiteralPath $LocalBinary -PathType Leaf)) { throw "compiled binary was not found: $LocalBinary" }

Write-Host "Creating Android directory: $AndroidDir"
& adb @AdbArgs shell mkdir -p $AndroidDir
if ($LASTEXITCODE -ne 0) { throw "failed to create Android directory: $AndroidDir" }
Write-Host "Pushing: $LocalBinary -> $RemoteBinary"
& adb @AdbArgs push $LocalBinary $RemoteBinary
if ($LASTEXITCODE -ne 0) { throw "failed to push nl2sh" }
& adb @AdbArgs shell chmod 755 $RemoteBinary
if ($LASTEXITCODE -ne 0) { throw "failed to make nl2sh executable" }

if ($AdbIsRoot) {
    Write-Host "Starting $RemoteBinary through root adbd."
    Write-Host "Press Ctrl+Q in nl2sh to exit."
    & adb @AdbArgs shell -t env NL2SH_WINDOWS_SCROLL=1 $RemoteBinary
    exit $LASTEXITCODE
}

Write-Host "Trying Android su as a fallback..."
& adb @AdbArgs shell su -c id *> $null
if ($LASTEXITCODE -eq 0) {
    Write-Host "su access granted; starting $RemoteBinary as root."
    Write-Host "Press Ctrl+Q in nl2sh to exit."
    & adb @AdbArgs shell -t su -c "NL2SH_WINDOWS_SCROLL=1 $RemoteBinary"
    exit $LASTEXITCODE
}

$RemoteConfig = "$AndroidDir/config.toml"
& adb @AdbArgs shell test -e $RemoteConfig
$ConfigExists = $LASTEXITCODE -eq 0
if ($ConfigExists) {
    & adb @AdbArgs shell test -r $RemoteConfig
    if ($LASTEXITCODE -ne 0) { throw "adb root and su are unavailable, and $RemoteConfig is not readable; permissions were left unchanged to protect the API key" }
}
Write-Warning "adb root and su are unavailable; starting as adb shell user."
Write-Host "Press Ctrl+Q in nl2sh to exit."
& adb @AdbArgs shell -t env NL2SH_WINDOWS_SCROLL=1 $RemoteBinary
exit $LASTEXITCODE
