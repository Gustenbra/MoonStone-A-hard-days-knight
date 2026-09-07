@echo off
setlocal enabledelayedexpansion
rem ---------------------------------------------------------------------------
rem  Build and run Moonstone in Rust.
rem
rem    play.bat                 build if needed, then play
rem    play.bat --rebake        re-read the original game files first
rem    play.bat --data "C:\path\to\Moonstone"     point at the original elsewhere
rem
rem  Anything else you pass is handed straight to the game, so the debug flags
rem  in README.md work here too:  play.bat --start select
rem ---------------------------------------------------------------------------
cd /d "%~dp0"

set "DATA=%~dp0..\Moonstone"
set "REBAKE="
set "PASS="

:args
if "%~1"=="" goto args_done
if /i "%~1"=="--rebake" ( set "REBAKE=1" & shift & goto args )
if /i "%~1"=="--data"   ( set "DATA=%~2" & shift & shift & goto args )
set "PASS=!PASS! %1"
shift
goto args
:args_done

where cargo >nul 2>&1
if errorlevel 1 (
  echo Rust is not installed, or cargo is not on your PATH.
  echo Install it from https://rustup.rs and open a new terminal.
  exit /b 1
)

if not exist "%DATA%\KN1.OB" (
  echo Could not find the original Moonstone files.
  echo Looked in: %DATA%
  echo That folder needs to hold KN1.OB, MAP.CMP and the rest.
  echo Point at the right one with:  play.bat --data "C:\path\to\Moonstone"
  exit /b 1
)

if not exist "packs\reference\manifest.json" set "REBAKE=1"

if defined REBAKE (
  echo Reading the original game files...
  cargo run --release --bin henge-bake -- "%DATA%"
  if errorlevel 1 exit /b 1
)

echo Building...
cargo build --release
if errorlevel 1 exit /b 1

echo.
echo   arrows move, space swings, escape quits
echo   tab switches map and arena, [ and ] change arena, R restarts, C the sheet
echo   player two: WASD and F
echo.
target\release\henge.exe %PASS%
