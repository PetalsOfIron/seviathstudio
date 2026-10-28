; SPDX-License-Identifier: GPL-3.0-or-later
; The installer of SeviathStudio for Windows (Inno Setup, free). It installs the program only: the AI tools are large and have their own licences, so
; they are not inside it; the program downloads them when you press a button (File, Downloads). Build it with `installer\build.cmd`.

#define AppName "SeviathStudio"
#define AppVersion "0.1.0"
#define AppExe "seviathstudio.exe"

[Setup]
AppId={{6F1E2C3A-9B7D-4C58-A1E0-5D3B8C7F2A41}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=SeviathStudio
AppPublisherURL=https://github.com/PetalsOfIron/seviathstudio
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
UninstallDisplayIcon={app}\{#AppExe}
OutputDir=..\target\installer
OutputBaseFilename=SeviathStudio-Setup-{#AppVersion}
SetupIconFile=..\assets\seviath.ico
LicenseFile=..\COPYING
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern
DisableProgramGroupPage=yes

[Tasks]
Name: "desktopicon"; Description: "A shortcut on the desktop"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\COPYING"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\docs\virtual-camera.md"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent
