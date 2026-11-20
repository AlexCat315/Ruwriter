; Ruwriter Windows installer (Inno Setup 6).
; Build: iscc packaging\windows\Ruwriter.iss /DAppVersion=0.4.0 /DOutputName=Ruwriter-v0.4.0-windows-x64-setup
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef OutputName
  #define OutputName "Ruwriter"
#endif

[Setup]
AppId=dev.cyke.ruwriter
AppName=Ruwriter
AppVersion={#AppVersion}
AppPublisher=Ruwriter
AppPublisherURL=https://github.com/AlexCat315/Ruwriter
AppSupportURL=https://github.com/AlexCat315/Ruwriter/issues
AppUpdatesURL=https://github.com/AlexCat315/Ruwriter/releases
DefaultDirName={autopf}\Ruwriter
DefaultGroupName=Ruwriter
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename={#OutputName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\ruwriter.exe
SourceDir=..\..

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加图标:"; Flags: unchecked

[Files]
Source: "dist\Ruwriter\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Ruwriter"; Filename: "{app}\ruwriter.exe"
Name: "{autodesktop}\Ruwriter"; Filename: "{app}\ruwriter.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ruwriter.exe"; Description: "启动 Ruwriter"; Flags: nowait postinstall skipifsilent
