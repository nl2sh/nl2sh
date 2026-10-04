@echo off
setlocal EnableExtensions DisableDelayedExpansion

set "SCRIPT_DIR=%~dp0"
set "PROVIDER=openrouter"
set "MODEL="
set "API_KEY="
set "ENDPOINT="
set "INSTALL_DIR=%CD%\nl2sh-android"
set "REPOSITORY=https://github.com/nl2sh/nl2sh"
set "CONFIG_REQUESTED=false"
set "WEB_ONLY=false"

:parse
if "%~1"=="" goto :run
if /i "%~1"=="--provider" goto :arg_provider
if /i "%~1"=="--model" goto :arg_model
if /i "%~1"=="--api-key" goto :arg_api_key
if /i "%~1"=="--endpoint" goto :arg_endpoint
if /i "%~1"=="--install-dir" goto :arg_install_dir
if /i "%~1"=="--repository" goto :arg_repository
if /i "%~1"=="--web-only" goto :arg_web_only
if /i "%~1"=="-h" goto :usage_ok
if /i "%~1"=="--help" goto :usage_ok
echo ERROR: unknown option: %~1 1>&2
goto :usage_fail

:arg_provider
if "%~2"=="" goto :missing_value
set "PROVIDER=%~2"
set "CONFIG_REQUESTED=true"
shift
shift
goto :parse

:arg_model
if "%~2"=="" goto :missing_value
set "MODEL=%~2"
set "CONFIG_REQUESTED=true"
shift
shift
goto :parse

:arg_api_key
if "%~2"=="" goto :missing_value
set "API_KEY=%~2"
set "CONFIG_REQUESTED=true"
shift
shift
goto :parse

:arg_endpoint
if "%~2"=="" goto :missing_value
set "ENDPOINT=%~2"
set "CONFIG_REQUESTED=true"
shift
shift
goto :parse

:arg_install_dir
if "%~2"=="" goto :missing_value
set "INSTALL_DIR=%~f2"
shift
shift
goto :parse

:arg_repository
if "%~2"=="" goto :missing_value
set "REPOSITORY=%~2"
shift
shift
goto :parse

:arg_web_only
set "WEB_ONLY=true"
shift
goto :parse

:run
where powershell.exe >nul 2>&1
if errorlevel 1 (
  echo ERROR: powershell.exe was not found in PATH. 1>&2
  exit /b 1
)

set "PS_INSTALLER=%SCRIPT_DIR%install-android.ps1"
set "REMOVE_PS_INSTALLER=false"
set "PS_INSTALLER_URL=https://raw.githubusercontent.com/nl2sh/nl2sh/master/install-android.ps1"
if /i "%REPOSITORY%"=="https://gitee.com/nl2sh/nl2sh" set "PS_INSTALLER_URL=https://gitee.com/nl2sh/nl2sh/raw/master/install-android.ps1"
if exist "%PS_INSTALLER%" goto :installer_ready

where curl.exe >nul 2>&1
if errorlevel 1 (
  echo ERROR: curl.exe was not found in PATH. 1>&2
  exit /b 1
)
set "PS_INSTALLER=%TEMP%\nl2sh-install-%RANDOM%-%RANDOM%.ps1"
set "REMOVE_PS_INSTALLER=true"
echo Downloading the Windows installer...
curl.exe -fL --retry 3 --proto "=https" --tlsv1.2 -o "%PS_INSTALLER%" "%PS_INSTALLER_URL%"
if errorlevel 1 (
  echo ERROR: failed to download install-android.ps1. 1>&2
  exit /b 1
)

:installer_ready
set "WEB_ONLY_ARG="
if /i "%WEB_ONLY%"=="true" set "WEB_ONLY_ARG=-WebOnly"
if /i "%CONFIG_REQUESTED%"=="false" goto :invoke_preserving_config
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_INSTALLER%" -Provider "%PROVIDER%" -Model "%MODEL%" -ApiKey "%API_KEY%" -Endpoint "%ENDPOINT%" -InstallDir "%INSTALL_DIR%" -Repository "%REPOSITORY%" %WEB_ONLY_ARG%
goto :installer_finished

:invoke_preserving_config
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_INSTALLER%" -Provider "%PROVIDER%" -Model "%MODEL%" -ApiKey "%API_KEY%" -Endpoint "%ENDPOINT%" -InstallDir "%INSTALL_DIR%" -Repository "%REPOSITORY%" -KeepExistingConfig %WEB_ONLY_ARG%

:installer_finished
set "INSTALL_EXIT=%ERRORLEVEL%"
if /i "%REMOVE_PS_INSTALLER%"=="true" del /q "%PS_INSTALLER%" >nul 2>&1
exit /b %INSTALL_EXIT%

:missing_value
echo ERROR: %~1 requires a value. 1>&2
exit /b 1

:usage_ok
call :usage
exit /b 0

:usage_fail
call :usage 1>&2
exit /b 1

:usage
echo Usage: install-android.bat [options]
echo Supported Android ABIs: arm64-v8a, armeabi-v7a, x86_64 ^(API 26+^).
echo The launcher detects the device ABI and prefers native x86_64.
echo   --provider NAME       openrouter, openai, deepseek, moonshot, siliconflow,
echo                         ollama, or custom ^(default: openrouter^)
echo   --model NAME          model name; provider default is used when omitted
echo   --api-key KEY         API key; prefer NL2SH_API_KEY to avoid command history
echo   --endpoint URL        required for custom; optional override for other providers
echo   --install-dir PATH    extraction directory ^(default: .\nl2sh-android^)
echo   --repository URL      GitHub or Gitee repository URL ^(default: GitHub^)
echo   --web-only           start the Web UI in the background without a TUI
exit /b 0
