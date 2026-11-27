@echo off
chcp 65001 >nul
title SPBFTUIT Bot Control
cd /d "%~dp0"

:menu
cls
echo  +--------------------------------------------------+
echo  ^|             SPBFTUIT  BOT  CONTROL               ^|
echo  +--------------------------------------------------+
echo  ^|                                                  ^|
echo  ^|   [1] Start bot                                  ^|
echo  ^|   [2] Stop bot                                   ^|
echo  ^|   [3] Show status                                ^|
echo  ^|   [4] Help                                       ^|
echo  ^|   [5] Sync database + publish schedule to GitHub            ^|
echo  ^|   [6] Download latest project code from GitHub   ^|
echo  ^|   [0] Exit                                       ^|
echo  ^|                                                  ^|
echo  +--------------------------------------------------+
echo.
set "choice="
set /p "choice=  Select command [0-5]: "

if "%choice%"=="1" goto start
if "%choice%"=="2" goto stop
if "%choice%"=="3" goto status
if "%choice%"=="4" goto help
if "%choice%"=="5" goto sync
if "%choice%"=="6" goto update
if "%choice%"=="0" goto end
echo.
echo  [!] Unknown command. Choose 0, 1, 2, 3, 4 or 5.
pause
goto menu

:start
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\bot-control.ps1" start
pause
goto menu

:stop
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\bot-control.ps1" stop
pause
goto menu

:status
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\bot-control.ps1" status
pause
goto menu

:sync
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\bot-control.ps1" sync
pause
goto menu

:update
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\bot-control.ps1" update
pause
goto menu

:help
echo.
echo  [1] Builds and starts the backend and Telegram bot.
echo  [2] Stops the backend process started for this project.
echo  [3] Checks whether the backend is running.
echo  [5] Downloads schedule data from EIOS and updates the local PostgreSQL database.
echo      This runs the same synchronization as /sync in Telegram.
echo  [6] Downloads the latest code from origin/feature/smart-campus-step-1.
echo      Stop the local bot first (option 2); the working tree must be clean.
echo.
echo  Requirements: Rust/Cargo, backend\.env with valid settings,
echo  and a reachable PostgreSQL/Neon database, Python 3, and Git origin access.
echo  The log is saved to backend\bot.log.
echo.
pause
goto menu

:end
echo.
echo  [ OK ] Control panel closed.
