@echo off
rem Builds the program and then the installer: target\installer\SeviathStudio-Setup-<version>.exe
rem Needs Inno Setup 6 (free: winget install JRSoftware.InnoSetup).
cd /d "%~dp0.."
cargo build --release || exit /b 1
set ISCC=%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe
if not exist "%ISCC%" set ISCC=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe
if not exist "%ISCC%" set ISCC=%ProgramFiles%\Inno Setup 6\ISCC.exe
"%ISCC%" installer\seviathstudio.iss || exit /b 1
echo done: target\installer
