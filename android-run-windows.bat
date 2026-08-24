@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"
title nl2sh Android Launcher

if not defined ANDROID_DIR set "ANDROID_DIR=/data/local/tmp"
set "REMOTE_BINARY=%ANDROID_DIR%/nl2sh"

where adb >nul 2>&1
if errorlevel 1 (
  echo ERROR: adb was not found in PATH.
  echo Install Android SDK Platform-Tools, then reopen this launcher.
  goto :fail
)
echo(!ANDROID_DIR!| findstr /r /x /c:"/[A-Za-z0-9._/-]*" >nul
if errorlevel 1 (
  echo ERROR: ANDROID_DIR must be a safe absolute Android path: %ANDROID_DIR%
  goto :fail
)

call :select_device
if errorlevel 1 goto :fail
echo Selected device: !SERIAL!

set "ABILIST="
for /f "usebackq delims=" %%A in (`adb -s "!SERIAL!" shell getprop ro.product.cpu.abilist 2^>nul`) do if not defined ABILIST set "ABILIST=%%A"
if not defined ABILIST for /f "usebackq delims=" %%A in (`adb -s "!SERIAL!" shell getprop ro.product.cpu.abi 2^>nul`) do if not defined ABILIST set "ABILIST=%%A"
echo Device ABI: !ABILIST!

echo(!ABILIST!| findstr /i /c:"arm64-v8a" >nul
if not errorlevel 1 (
  set "LOCAL_BINARY=%~dp0bin\arm64-v8a\nl2sh"
  set "SELECTED_ABI=arm64-v8a (64-bit)"
) else (
  echo(!ABILIST!| findstr /i /c:"armeabi-v7a" >nul
  if errorlevel 1 (
    echo ERROR: unsupported device ABI: !ABILIST!
    echo This package supports arm64-v8a and armeabi-v7a only.
    goto :fail
  )
  set "LOCAL_BINARY=%~dp0bin\armeabi-v7a\nl2sh"
  set "SELECTED_ABI=armeabi-v7a (32-bit)"
)
if not exist "!LOCAL_BINARY!" (
  echo ERROR: packaged binary is missing: !LOCAL_BINARY!
  goto :fail
)
echo Selected binary: !SELECTED_ABI!

set "ADB_IS_ROOT=false"
echo Restarting adbd with root privileges...
adb -s "!SERIAL!" root
adb -s "!SERIAL!" wait-for-device
if errorlevel 1 (
  echo ERROR: device did not reconnect after adb root.
  goto :fail
)
set "DEVICE_UID="
for /f "usebackq delims=" %%A in (`adb -s "!SERIAL!" shell id -u 2^>nul`) do if not defined DEVICE_UID set "DEVICE_UID=%%A"
if "!DEVICE_UID!"=="0" (
  set "ADB_IS_ROOT=true"
  echo adbd is running as root.
) else (
  echo WARNING: adb root is unsupported or denied; trying normal adbd.
)

echo Creating Android directory: %ANDROID_DIR%
adb -s "!SERIAL!" shell mkdir -p "%ANDROID_DIR%"
if errorlevel 1 goto :adb_fail
echo Pushing: !LOCAL_BINARY! ^> %REMOTE_BINARY%
adb -s "!SERIAL!" push "!LOCAL_BINARY!" "%REMOTE_BINARY%"
if errorlevel 1 goto :adb_fail
adb -s "!SERIAL!" shell chmod 755 "%REMOTE_BINARY%"
if errorlevel 1 goto :adb_fail

if "!ADB_IS_ROOT!"=="true" (
  echo Starting %REMOTE_BINARY% through root adbd.
  echo Press Ctrl+Q in nl2sh to exit.
  adb -s "!SERIAL!" shell -t "%REMOTE_BINARY%"
  set "RUN_EXIT=!ERRORLEVEL!"
  goto :done
)

echo Trying Android su as a fallback...
adb -s "!SERIAL!" shell su -c id >nul 2>&1
if not errorlevel 1 (
  echo su access granted; starting %REMOTE_BINARY% as root.
  echo Press Ctrl+Q in nl2sh to exit.
  adb -s "!SERIAL!" shell -t su -c "%REMOTE_BINARY%"
  set "RUN_EXIT=!ERRORLEVEL!"
  goto :done
)

adb -s "!SERIAL!" shell test -e "%ANDROID_DIR%/config.toml" >nul 2>&1
if not errorlevel 1 (
  adb -s "!SERIAL!" shell test -r "%ANDROID_DIR%/config.toml" >nul 2>&1
  if errorlevel 1 (
    echo ERROR: config.toml exists but is unreadable without root.
    echo Permissions were left unchanged to protect the API key.
    goto :fail
  )
)

echo WARNING: adb root and su are unavailable; starting as adb shell user.
echo Press Ctrl+Q in nl2sh to exit.
adb -s "!SERIAL!" shell -t "%REMOTE_BINARY%"
set "RUN_EXIT=!ERRORLEVEL!"
goto :done

:select_device
if defined ADB_SERIAL (
  adb -s "!ADB_SERIAL!" get-state 2>nul | findstr /x "device" >nul
  if errorlevel 1 (
    echo ERROR: ADB_SERIAL is not a usable device: !ADB_SERIAL!
    exit /b 1
  )
  set "SERIAL=!ADB_SERIAL!"
  exit /b 0
)
call :collect_devices
if !DEVICE_COUNT! EQU 0 (
  echo No connected ADB device was found.
  set /p "DEVICE_IP=Enter Android device IP or IP:port: "
  if not defined DEVICE_IP (
    echo ERROR: no IP address was entered.
    exit /b 1
  )
  adb connect "!DEVICE_IP!"
  if errorlevel 1 exit /b 1
  call :collect_devices
)
if !DEVICE_COUNT! EQU 0 (
  echo ERROR: no usable ADB device is connected.
  exit /b 1
)
if !DEVICE_COUNT! EQU 1 (
  set "SERIAL=!DEVICE_1!"
  exit /b 0
)

echo Multiple ADB devices are connected:
for /l %%N in (1,1,!DEVICE_COUNT!) do echo   %%N. !DEVICE_%%N!
set /p "DEVICE_CHOICE=Enter device number: "
echo(!DEVICE_CHOICE!| findstr /r /x "[1-9][0-9]*" >nul
if errorlevel 1 (
  echo ERROR: invalid device number.
  exit /b 1
)
if !DEVICE_CHOICE! GTR !DEVICE_COUNT! (
  echo ERROR: device number is out of range.
  exit /b 1
)
for %%N in (!DEVICE_CHOICE!) do set "SERIAL=!DEVICE_%%N!"
exit /b 0

:collect_devices
set "DEVICE_COUNT=0"
for /f "skip=1 tokens=1,2" %%A in ('adb devices 2^>nul') do (
  if "%%B"=="device" (
    set /a DEVICE_COUNT+=1
    set "DEVICE_!DEVICE_COUNT!=%%A"
  )
)
exit /b 0

:adb_fail
echo ERROR: an ADB deployment command failed.
goto :fail

:done
if not defined RUN_EXIT set "RUN_EXIT=0"
if not defined NL2SH_NO_PAUSE pause
exit /b !RUN_EXIT!

:fail
if not defined NL2SH_NO_PAUSE pause
exit /b 1
