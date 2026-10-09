@echo off
chcp 65001 >nul
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"
title nl2sh Android Launcher

if not defined ANDROID_DIR set "ANDROID_DIR=/data/local/tmp"
set "REMOTE_BINARY=%ANDROID_DIR%/nl2sh"
set "REMOTE_CONFIG=%ANDROID_DIR%/config.toml"
set "WEB_ONLY=false"

:parse_args
if "%~1"=="" goto :args_done
if /i "%~1"=="--web-only" (
  set "WEB_ONLY=true"
  shift
  goto :parse_args
)
if /i "%~1"=="-h" goto :usage
if /i "%~1"=="--help" goto :usage
echo ERROR: unknown option: %~1
goto :fail

:args_done

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

echo(,!ABILIST!,| findstr /i /c:",x86_64," >nul
if not errorlevel 1 (
  set "LOCAL_BINARY=%~dp0bin\x86_64\nl2sh"
  set "SELECTED_ABI=x86_64 (64-bit)"
) else (
  echo(,!ABILIST!,| findstr /i /c:",arm64-v8a," >nul
  if not errorlevel 1 (
    set "LOCAL_BINARY=%~dp0bin\arm64-v8a\nl2sh"
    set "SELECTED_ABI=arm64-v8a (64-bit)"
  ) else (
    echo(,!ABILIST!,| findstr /i /c:",armeabi-v7a," >nul
    if errorlevel 1 (
      echo ERROR: unsupported device ABI: !ABILIST!
      echo This package supports arm64-v8a, armeabi-v7a and x86_64.
      goto :fail
    )
    set "LOCAL_BINARY=%~dp0bin\armeabi-v7a\nl2sh"
    set "SELECTED_ABI=armeabi-v7a (32-bit)"
  )
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
call :local_sha256 "!LOCAL_BINARY!"
if errorlevel 1 goto :fail
call :remote_sha256
if /i "!REMOTE_SHA256!"=="!LOCAL_SHA256!" (
  echo Binary checksum matches; skipping adb push.
) else (
  echo Pushing: !LOCAL_BINARY! ^> %REMOTE_BINARY%
  adb -s "!SERIAL!" push "!LOCAL_BINARY!" "%REMOTE_BINARY%"
  if errorlevel 1 goto :adb_fail
  call :remote_sha256
  if /i not "!REMOTE_SHA256!"=="!LOCAL_SHA256!" (
    echo ERROR: remote binary checksum verification failed after adb push.
    goto :fail
  )
  echo Verified SHA-256: !LOCAL_SHA256!
)
adb -s "!SERIAL!" shell chmod 755 "%REMOTE_BINARY%"
if errorlevel 1 goto :adb_fail

if defined NL2SH_CONFIG_SOURCE (
  if not exist "!NL2SH_CONFIG_SOURCE!" (
    echo ERROR: NL2SH_CONFIG_SOURCE is not a file: !NL2SH_CONFIG_SOURCE!
    goto :fail
  )
  echo Deploying configuration: !NL2SH_CONFIG_SOURCE! ^> %REMOTE_CONFIG%
  if "!ADB_IS_ROOT!"=="true" (
    adb -s "!SERIAL!" push "!NL2SH_CONFIG_SOURCE!" "%REMOTE_CONFIG%" >nul
    if errorlevel 1 goto :adb_fail
    adb -s "!SERIAL!" shell chmod 600 "%REMOTE_CONFIG%"
    if errorlevel 1 goto :adb_fail
  ) else (
    set "REMOTE_CONFIG_TEMP=%ANDROID_DIR%/.config.toml.nl2sh-adb"
    adb -s "!SERIAL!" push "!NL2SH_CONFIG_SOURCE!" "!REMOTE_CONFIG_TEMP!" >nul
    if errorlevel 1 goto :adb_fail
    adb -s "!SERIAL!" shell su -c id >nul 2>&1
    if not errorlevel 1 (
      adb -s "!SERIAL!" shell su -c "cp '!REMOTE_CONFIG_TEMP!' '%REMOTE_CONFIG%' && chmod 600 '%REMOTE_CONFIG%' && rm -f '!REMOTE_CONFIG_TEMP!'"
      if errorlevel 1 goto :adb_fail
    ) else (
      adb -s "!SERIAL!" shell mv "!REMOTE_CONFIG_TEMP!" "%REMOTE_CONFIG%"
      if errorlevel 1 goto :adb_fail
      adb -s "!SERIAL!" shell chmod 600 "%REMOTE_CONFIG%"
      if errorlevel 1 goto :adb_fail
    )
  )
)

if "!ADB_IS_ROOT!"=="true" (
  if not "!WEB_ONLY!"=="true" (
    call :stop_existing_nl2sh false
    if errorlevel 1 goto :adb_fail
  )
  echo Starting %REMOTE_BINARY% through root adbd.
  if "!WEB_ONLY!"=="true" (
    call :start_web_only false
    set "RUN_EXIT=!ERRORLEVEL!"
    goto :done
  )
  echo Press Ctrl+Q in nl2sh to exit.
  adb -s "!SERIAL!" shell -t env NL2SH_WINDOWS_SCROLL=1 "%REMOTE_BINARY%"
  set "RUN_EXIT=!ERRORLEVEL!"
  goto :done
)

echo Trying Android su as a fallback...
adb -s "!SERIAL!" shell su -c id >nul 2>&1
if not errorlevel 1 (
  if not "!WEB_ONLY!"=="true" (
    call :stop_existing_nl2sh true
    if errorlevel 1 goto :adb_fail
  )
  echo su access granted; starting %REMOTE_BINARY% as root.
  if "!WEB_ONLY!"=="true" (
    call :start_web_only true
    set "RUN_EXIT=!ERRORLEVEL!"
    goto :done
  )
  echo Press Ctrl+Q in nl2sh to exit.
  adb -s "!SERIAL!" shell -t su -c "NL2SH_WINDOWS_SCROLL=1 %REMOTE_BINARY%"
  set "RUN_EXIT=!ERRORLEVEL!"
  goto :done
)

adb -s "!SERIAL!" shell test -e "%REMOTE_CONFIG%" >nul 2>&1
if not errorlevel 1 (
  adb -s "!SERIAL!" shell test -r "%REMOTE_CONFIG%" >nul 2>&1
  if errorlevel 1 (
    echo ERROR: config.toml exists but is unreadable without root.
    echo Permissions were left unchanged to protect the API key.
    goto :fail
  )
)

echo WARNING: adb root and su are unavailable; starting as adb shell user.
if not "!WEB_ONLY!"=="true" (
  call :stop_existing_nl2sh false
  if errorlevel 1 goto :adb_fail
)
if "!WEB_ONLY!"=="true" (
  call :start_web_only false
  set "RUN_EXIT=!ERRORLEVEL!"
  goto :done
)
echo Press Ctrl+Q in nl2sh to exit.
adb -s "!SERIAL!" shell -t env NL2SH_WINDOWS_SCROLL=1 "%REMOTE_BINARY%"
set "RUN_EXIT=!ERRORLEVEL!"
goto :done

:start_web_only
set "WEB_COMMAND='%REMOTE_BINARY%' --config '%REMOTE_CONFIG%' service start --json"
if /i "%~1"=="true" (
  adb -s "!SERIAL!" shell su -c "!WEB_COMMAND!"
) else (
  adb -s "!SERIAL!" shell "!WEB_COMMAND!"
)
if errorlevel 1 (
  echo ERROR: the native background service did not start.
  echo Inspect the config.service directory next to %REMOTE_CONFIG% on the device.
  exit /b 1
)
echo Forward the reported port to open the Web UI: adb -s "!SERIAL!" forward tcp:9999 tcp:PORT
echo Stop the owned service with: %REMOTE_BINARY% --config %REMOTE_CONFIG% service stop --json
exit /b 0

:stop_existing_nl2sh
set "STOP_COMMAND='%REMOTE_BINARY%' --config '%REMOTE_CONFIG%' service stop --json"
echo Stopping the owned native service for this configuration...
if /i "%~1"=="true" (
  adb -s "!SERIAL!" shell su -c "!STOP_COMMAND!"
) else (
  adb -s "!SERIAL!" shell "!STOP_COMMAND!"
)
exit /b !ERRORLEVEL!

:usage
echo Usage: android-run-windows.bat [--web-only]
echo Supported Android ABIs: arm64-v8a, armeabi-v7a, x86_64 ^(API 26+^).
echo Device ABI is detected automatically; native x86_64 takes priority.
exit /b 0

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
rem Use only a literal tab as delimiter; mDNS serials may contain spaces.
for /f "skip=1 tokens=1,2 delims=	" %%A in ('adb devices 2^>nul') do (
  if "%%B"=="device" (
    set /a DEVICE_COUNT+=1
    set "DEVICE_!DEVICE_COUNT!=%%A"
  )
)
exit /b 0

:local_sha256
set "LOCAL_SHA256="
for /f "skip=1 tokens=*" %%A in ('certutil -hashfile "%~1" SHA256 2^>nul') do if not defined LOCAL_SHA256 set "LOCAL_SHA256=%%A"
set "LOCAL_SHA256=!LOCAL_SHA256: =!"
echo(!LOCAL_SHA256!| findstr /r /i /x "[0-9a-f][0-9a-f]*" >nul
if errorlevel 1 (
  echo ERROR: failed to calculate the local SHA-256 checksum with certutil.
  exit /b 1
)
exit /b 0

:remote_sha256
set "REMOTE_SHA256="
for /f "usebackq tokens=1" %%A in (`adb -s "!SERIAL!" shell toybox sha256sum "%REMOTE_BINARY%" 2^>nul`) do if not defined REMOTE_SHA256 set "REMOTE_SHA256=%%A"
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
