@echo off
rem ===========================================================================
rem  Favnyr - quick development launcher (Windows)
rem
rem  Builds and starts the Slint UI in the DEBUG profile: the fast loop for
rem  working on the interface. Double-click this file, or run it from any
rem  terminal - it always operates from the repository root, not from the
rem  current directory.
rem
rem  Usage:   dev.bat [arguments]
rem           Arguments are forwarded to favnyr itself; its public one is the
rem           visible name of a saved workspace, e.g.  dev.bat "My Workspace"
rem
rem  For a distributable binary, use the release profile instead:
rem           cargo run --release --bin favnyr
rem
rem  The console stays attached on purpose: in a debug build the logs go to
rem  stderr (as well as to cache/favnyr.log), and the app catches Ctrl+C for a
rem  clean shutdown - see install_console_ctrl_handler in main.rs. The first
rem  run compiles the whole workspace and takes a few minutes.
rem
rem  ASCII only, and no fancy characters: the file has to survive a cmd.exe
rem  console running under any code page.
rem ===========================================================================

setlocal EnableExtensions

rem -- Always work from the repository root, wherever this was started from ----
cd /d "%~dp0"
if not exist "Cargo.toml" goto :no_root

title Favnyr - dev build

rem -- Locate cargo -----------------------------------------------------------
rem rustup installs its shims into %USERPROFILE%\.cargo\bin, which is on PATH
rem only for the account that installed Rust. So: PATH first, then that folder,
rem then the installed toolchains themselves (a toolchain can be present while
rem its shims are not).
set "CARGO="

where cargo.exe >nul 2>nul
if not errorlevel 1 set "CARGO=cargo"

if not defined CARGO if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "CARGO=%USERPROFILE%\.cargo\bin\cargo.exe"

if not defined CARGO call :find_toolchain_cargo

if not defined CARGO goto :no_cargo

rem -- Development logging ----------------------------------------------------
rem The binary defaults to `info` and honours RUST_LOG. Ask for `debug` on
rem Favnyr's own crates only; everything else stays at `warn` so the console
rem remains readable. An RUST_LOG set outside this script always wins.
if not defined RUST_LOG set "RUST_LOG=favnyr=debug,favnyr_core=debug,warn"

echo [dev] running: %CARGO% run --bin favnyr %*
echo.

"%CARGO%" run --bin favnyr %*
set "CODE=%ERRORLEVEL%"

if not "%CODE%"=="0" goto :run_failed

exit /b 0

rem ---------------------------------------------------------------------------
rem  Fallback: read the pinned channel from rust-toolchain.toml and look for
rem  that toolchain under %USERPROFILE%\.rustup\toolchains. Kept in a
rem  subroutine so %CHANNEL% is expanded after it has been set (inside a
rem  single parenthesised block it would expand to nothing).
rem
rem  TOML line:  channel    = "1.97.1"   ->   %%~b = 1.97.1
rem ---------------------------------------------------------------------------
:find_toolchain_cargo
set "CHANNEL="
for /f "usebackq tokens=1,2 delims== " %%a in ("%~dp0rust-toolchain.toml") do if /i "%%a"=="channel" set "CHANNEL=%%~b"
if not defined CHANNEL exit /b 0
for /d %%d in ("%USERPROFILE%\.rustup\toolchains\%CHANNEL%-*") do if not defined CARGO if exist "%%~fd\bin\cargo.exe" set "CARGO=%%~fd\bin\cargo.exe"
exit /b 0

:no_root
echo [dev] Cargo.toml not found next to this script:
echo [dev]   %~dp0
echo [dev] Keep dev.bat in the root of the repository.
echo.
pause
exit /b 1

:no_cargo
echo [dev] cargo was not found.
echo [dev] Install Rust from https://rust-lang.org/tools/install/ and run this again.
echo.
pause
exit /b 1

:run_failed
echo.
echo [dev] the run failed with exit code %CODE%.
echo [dev] The build or application output above has the details.
echo.
pause
exit /b %CODE%
