; Inno Setup script for FormatCraft. Built by `cargo xtask dist-windows`,
; which passes AppVersion, StageDir and OutDir.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef StageDir
  #define StageDir "..\..\dist\windows\FormatCraft"
#endif
#ifndef OutDir
  #define OutDir "..\..\dist"
#endif

[Setup]
AppId={{6F1C2A9E-3B7D-4E21-9A55-0C4F7B8D2E61}
AppName=FormatCraft
AppVersion={#AppVersion}
AppVerName=FormatCraft {#AppVersion}
AppPublisher=FormatCraft contributors
AppPublisherURL=https://github.com/miniamoci-tech/formatcraft
AppSupportURL=https://github.com/miniamoci-tech/formatcraft/issues
DefaultDirName={autopf}\FormatCraft
DefaultGroupName=FormatCraft
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutDir}
OutputBaseFilename=FormatCraft-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\icon\formatcraft.ico
UninstallDisplayIcon={app}\formatcraft.exe
LicenseFile=..\..\LICENSE-MIT
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=no

[Languages]
Name: "italian"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "sendto"; Description: "Send To → FormatCraft"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\FormatCraft"; Filename: "{app}\formatcraft.exe"
Name: "{group}\{cm:UninstallProgram,FormatCraft}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\FormatCraft"; Filename: "{app}\formatcraft.exe"; Tasks: desktopicon
Name: "{usersendto}\FormatCraft"; Filename: "{app}\formatcraft.exe"; Tasks: sendto

[Run]
Filename: "{app}\formatcraft.exe"; Description: "{cm:LaunchProgram,FormatCraft}"; Flags: nowait postinstall skipifsilent
