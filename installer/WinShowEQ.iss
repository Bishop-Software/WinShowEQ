#ifndef AppVersion
  #define AppVersion "0.1.0-dev"
#endif

#define AppName "WinShowEQ"
#define AppPublisher "WinShowEQ"
#define StageDir "..\target\installer-stage"
#define OutputDir "..\target\installer-output"

[Setup]
AppId={{A132E7A2-D70B-4F66-924F-BB4C873787BC}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\WinShowEQ
DefaultGroupName=WinShowEQ
UninstallDisplayIcon={app}\bin\WinShowEQServer.exe
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
DisableProgramGroupPage=yes
LicenseFile={#StageDir}\docs\LICENSE.txt
OutputDir={#OutputDir}
OutputBaseFilename=WinShowEQ-Setup-{#AppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Types]
Name: "server"; Description: "Server only (recommended)"
Name: "custom"; Description: "Custom install"

[Components]
Name: "server"; Description: "WinShowEQ server"; Types: server custom; Flags: fixed
#ifdef IncludeClient
Name: "client"; Description: "WinShowEQ client (88/88 tests passing; C1-C7 complete, C8 in progress)"; Types: custom
#endif

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut for WinShowEQ Server"; Flags: unchecked
Name: "launchserver"; Description: "Launch WinShowEQ Server after setup completes"; Flags: unchecked

[Dirs]
Name: "{app}\bin"; Components: server
Name: "{app}\docs"; Components: server
Name: "{commonappdata}\WinShowEQ"; Permissions: users-modify; Components: server
#ifdef IncludeClient
Name: "{commonappdata}\WinShowEQ\client"; Permissions: users-modify; Components: client
Name: "{commonappdata}\WinShowEQ\client\filters"; Permissions: users-modify; Components: client
Name: "{commonappdata}\WinShowEQ\client\timers"; Permissions: users-modify; Components: client
Name: "{commonappdata}\WinShowEQ\client\annotations"; Permissions: users-modify; Components: client
Name: "{commonappdata}\WinShowEQ\client\maps"; Permissions: users-modify; Components: client
Name: "{commonappdata}\WinShowEQ\client\logs"; Permissions: users-modify; Components: client
#endif

[Files]
Source: "{#StageDir}\bin\WinShowEQServer.exe"; DestDir: "{app}\bin"; Flags: ignoreversion; Components: server
Source: "{#StageDir}\config\myseqserver.ini"; DestDir: "{commonappdata}\WinShowEQ"; Flags: onlyifdoesntexist uninsneveruninstall; Components: server
Source: "{#StageDir}\config\patterns.ini"; DestDir: "{commonappdata}\WinShowEQ"; Flags: onlyifdoesntexist uninsneveruninstall; Components: server
Source: "{#StageDir}\docs\README.md"; DestDir: "{app}\docs"; DestName: "README.md"; Flags: ignoreversion; Components: server
Source: "{#StageDir}\docs\INSTALLER-README.md"; DestDir: "{app}\docs"; DestName: "INSTALLER-README.md"; Flags: ignoreversion; Components: server
Source: "{#StageDir}\docs\LICENSE.txt"; DestDir: "{app}\docs"; DestName: "LICENSE.txt"; Flags: ignoreversion; Components: server
#ifdef IncludeClient
Source: "{#StageDir}\bin\WinShowEQClient.exe"; DestDir: "{app}\bin"; Flags: ignoreversion; Components: client
Source: "{#StageDir}\config\client.ini"; DestDir: "{commonappdata}\WinShowEQ\client"; Flags: onlyifdoesntexist uninsneveruninstall; Components: client
#endif

[Icons]
Name: "{group}\WinShowEQ Server"; Filename: "{app}\bin\WinShowEQServer.exe"; WorkingDir: "{commonappdata}\WinShowEQ"; Components: server
Name: "{group}\WinShowEQ Configuration Folder"; Filename: "{commonappdata}\WinShowEQ"; Components: server
Name: "{autodesktop}\WinShowEQ Server"; Filename: "{app}\bin\WinShowEQServer.exe"; WorkingDir: "{commonappdata}\WinShowEQ"; Tasks: desktopicon; Components: server
#ifdef IncludeClient
Name: "{group}\WinShowEQ Client"; Filename: "{app}\bin\WinShowEQClient.exe"; WorkingDir: "{commonappdata}\WinShowEQ\client"; Components: client
Name: "{group}\WinShowEQ Client Configuration"; Filename: "{commonappdata}\WinShowEQ\client"; Components: client
#endif

[Run]
Filename: "{app}\bin\WinShowEQServer.exe"; Description: "Launch WinShowEQ Server"; WorkingDir: "{commonappdata}\WinShowEQ"; Flags: nowait postinstall skipifsilent; Tasks: launchserver; Components: server

