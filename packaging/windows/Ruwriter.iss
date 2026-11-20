; Ruwriter Windows installer (Inno Setup 6).
; Build: substitute @APP_VERSION@ and @OUTPUT_NAME@, then run iscc on the generated file.
#ifndef AppVersion
  #define AppVersion "@APP_VERSION@"
#endif
#ifndef OutputName
  #define OutputName "@OUTPUT_NAME@"
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
