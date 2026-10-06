# Run with: powershell -NoProfile -File scripts/tests/test_adb_devices.ps1
$ErrorActionPreference = 'Stop'
$Tokens = $null
$ParseErrors = $null
$Launcher = Join-Path $PSScriptRoot '../../android-build-run.ps1'
$Ast = [System.Management.Automation.Language.Parser]::ParseFile(
    (Resolve-Path $Launcher).Path, [ref]$Tokens, [ref]$ParseErrors)
if ($ParseErrors.Count) { throw ($ParseErrors | Out-String) }
$Selection = $Ast.Find({
    param($Node)
    $Node -is [System.Management.Automation.Language.IfStatementAst] -and
    $Node.Clauses[0].Item1.Extent.Text -eq '$env:ADB_SERIAL'
}, $true)
if (-not $Selection) { throw 'device selection block was not found' }
$SelectDevice = [scriptblock]::Create($Selection.Extent.Text)
$SavedSerial = $env:ADB_SERIAL
try {
    $env:ADB_SERIAL = $null
    function adb {
        if ($args[0] -ne 'devices') { throw 'unexpected ADB operation' }
        $global:LASTEXITCODE = $script:AdbExitCode
        $script:DeviceLines
    }
    function Read-Host {
        param($Prompt)
        if ($Prompt -ne 'Enter device number') { throw "unexpected prompt: $Prompt" }
        '2'
    }
    $script:AdbExitCode = 0
    foreach ($Serial in @('usb-123', '192.0.2.1:5555', 'adb-test (3)._adb-tls-connect._tcp')) {
        $script:DeviceLines = @('List of devices attached', "$Serial`tdevice", '')
        . $SelectDevice
        if ($SelectedSerial -cne $Serial) { throw "serial was changed: $SelectedSerial" }
    }
    $script:DeviceLines = @('List of devices attached', "offline-usb`toffline",
        "locked-usb`tunauthorized", "usb-123`tdevice",
        "adb-test (3)._adb-tls-connect._tcp`tdevice", '')
    . $SelectDevice
    if ($SelectedSerial -cne 'adb-test (3)._adb-tls-connect._tcp') {
        throw 'multiple-device selection failed'
    }
    $script:AdbExitCode = 1
    $Failure = $null
    try { . $SelectDevice } catch { $Failure = $_.Exception.Message }
    if ($Failure -ne 'failed to list ADB devices') { throw "unexpected failure: $Failure" }
    Write-Host 'ADB device selection regression checks passed.'
    # Exercise the actual Batch selection subroutines with a fake adb command.
    $BatchSource = Get-Content (Join-Path $PSScriptRoot '../../android-run-windows.bat') -Raw
    $BatchFunctions = [regex]::Match($BatchSource, '(?ms)^:select_device\r?\n.*?(?=^:local_sha256\r?$)').Value
    if (-not $BatchFunctions) { throw 'Batch device selection subroutines were not found' }
    $TestDir = Join-Path ([System.IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $TestDir | Out-Null
    $MockAdb = Join-Path $TestDir 'adb.cmd'
    $Fixture = Join-Path $TestDir 'devices.txt'
    $BatchTest = Join-Path $TestDir 'selection.cmd'
    try {
        Set-Content $MockAdb '@type "%~dp0devices.txt"' -Encoding ASCII
        $Header = "@echo off`r`nsetlocal EnableDelayedExpansion`r`nset ADB_SERIAL=`r`nset `"PATH=%~dp0;%PATH%`"`r`ncall :select_device`r`nif errorlevel 1 exit /b 1`r`necho SELECTED=!SERIAL!`r`nexit /b 0`r`n"
        Set-Content $BatchTest ($Header + $BatchFunctions) -Encoding ASCII
        foreach ($Multiple in @($false, $true)) {
            $Lines = @('List of devices attached', "locked-usb`tunauthorized", "offline-usb`toffline")
            if ($Multiple) { $Lines += "usb-123`tdevice" }
            $Lines += "adb-test (3)._adb-tls-connect._tcp`tdevice"
            Set-Content $Fixture $Lines -Encoding ASCII
            $Output = ('2' | & cmd.exe /d /c $BatchTest | Out-String)
            if ($LASTEXITCODE -ne 0 -or -not $Output.Contains("SELECTED=adb-test (3)._adb-tls-connect._tcp`r`n")) {
                throw "Batch device selection failed: $Output"
            }
        }
        Write-Host 'Batch device selection regression checks passed.'
    } finally {
        Remove-Item -LiteralPath $MockAdb, $Fixture, $BatchTest -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $TestDir -ErrorAction SilentlyContinue
    }
} finally {
    $env:ADB_SERIAL = $SavedSerial
}
