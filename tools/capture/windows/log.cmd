@echo off
REM Append an ISO-8601 wall-clock line to the session log so physical actions
REM can be lined up with the traces. Run from any elevated prompt:
REM   C:\kit\log "started tilt sweep"
REM The log lives at C:\mpp-captures\session-log.txt (override with set LOGFILE=...).
setlocal
if "%LOGFILE%"=="" set "LOGFILE=C:\mpp-captures\session-log.txt"
if not exist "%LOGFILE%" (
  mkdir "C:\mpp-captures" 2>nul
  echo # session log > "%LOGFILE%"
)
for /f "usebackq tokens=*" %%i in (`powershell -NoProfile -Command "Get-Date -Format s"`) do set "TS=%%i"
>>"%LOGFILE%" echo %TS% :: %*
echo %TS% :: %*
