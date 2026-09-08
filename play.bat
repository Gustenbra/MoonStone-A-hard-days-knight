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


rem The music has to be lifted out of the tune drivers before the bake can
rem put it in the pack. It needs python and unicorn, and the game plays
rem without it, so a failure here is a note rather than a stop.
if not exist "research\tunes.json" (
  where python >nul 2>&1
  if errorlevel 1 (
    echo   no python, so no music. The rest of the game is unaffected.
  ) else (
    echo Reading the music...
    python tools\tunes.py "%DATA%" research\tunes.json
    if errorlevel 1 echo   no music this time. It needs unicorn:  pip install unicorn
  )
)

rem The baker is asked every time. It compares the pack's recipe stamp against
rem its own and does nothing when they match; when they do not, it rebakes
rem without anyone having to remember a flag. Getting this wrong is silent:
rem the game starts, and quietly has no music, no animation scripts and no
rem overworld grid.
set "FORCE="
if defined REBAKE set "FORCE=--force"
cargo run --release --quiet --bin henge-bake -- "%DATA%" %FORCE%
rem Exit code 3 is the one failure this script can fix itself: the MAIN.EXE
rem image is missing or was left by an older unpacker. tools\symbolmap.py
rem needs the same python and unicorn the music does. Run it and ask once
rem more. Anything else stops here, loudly, rather than starting a game
rem with the wrong thing on screen.
if errorlevel 4 exit /b 1
if errorlevel 3 goto unpack
if errorlevel 1 exit /b 1
goto baked
:unpack
where python >nul 2>&1
if errorlevel 1 (
  echo   that needs python and unicorn:  pip install unicorn
  exit /b 1
)
echo Unpacking MAIN.EXE...
python tools\symbolmap.py "%DATA%\MAIN.EXE" research\symbols.json --image research\main.final.bin
if errorlevel 1 exit /b 1
cargo run --release --quiet --bin henge-bake -- "%DATA%" %FORCE%
if errorlevel 1 exit /b 1
:baked

echo Building...
cargo build --release
if errorlevel 1 exit /b 1

echo.
echo   menus: arrows move, enter or space takes
echo   fighting: arrows move, space swings, escape quits
echo   tab switches map and arena, [ and ] change arena, R restarts, C the sheet
echo   player two: WASD and F
echo   gamepads work, and F11 calibrates one
echo.
target\release\henge.exe %PASS%
