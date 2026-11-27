; VanguardCV Windows x64 Installer Script (Inno Setup 6)
; Ships a clean application binary with zero pre-populated database files.

#ifndef AppVersion
  #define AppVersion "0.0.3"
#endif

#define AppName "VanguardCV"
#define AppPublisher "Omer Tawfig"
#define AppURL "https://github.com/omertawfig/VanguardCV"
#define AppExeName "vanguard-cv.exe"

[Setup]
; Fixed GUID so future releases upgrade in-place cleanly without duplicate entries.
AppId={{8B4E3D6A-9F2C-4E1B-A6D7-5C8F2E1A9B3D}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
AllowNoIcons=yes
LicenseFile=..\LICENSE
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=VanguardCV-Setup-{#AppVersion}-x64
SetupIconFile=..\assets\branding\vanguard-cv.ico
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
DisableProgramGroupPage=yes
CloseApplications=force
CloseApplicationsFilter={#AppExeName}
RestartApplications=no
VersionInfoVersion={#AppVersion}.0
VersionInfoCompany={#AppPublisher}
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#AppVersion}
VersionInfoDescription={#AppName} Windows Setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
; Interactive installation: offer checkbox to launch VanguardCV on finish.
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
; Silent auto-update (/SILENT): automatically relaunch the updated VanguardCV build.
Filename: "{app}\{#AppExeName}"; Flags: nowait skipifnotsilent
