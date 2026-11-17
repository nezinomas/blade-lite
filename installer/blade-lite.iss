; The Blade Lite installer (stage 39, tickets 78 and 233), for Inno Setup 6.
; Per user, no admin. Build: ISCC /DAppVersion=x.y.z installer\blade-lite.iss
; (release.yml does it). Upgrade and uninstall end the running copy with
; `blade-lite.exe --quit`, so it sends its quit preset, then set the fans
; to auto with `--reset` (README, "For an installer").

#ifndef AppVersion
  #error Give the version: ISCC /DAppVersion=x.y.z
#endif
#ifndef AppExe
  #define AppExe "..\target\release\blade-lite.exe"
#endif
; Never change it: upgrades and the uninstall find the install by it.
#define AppGuid "6C78510F-E7B7-4E6D-B543-66FD0C1F6CD1"
#define RepoUrl "https://github.com/nezinomas/blade-lite"

[Setup]
AppId={{{#AppGuid}}
AppName=Blade Lite
AppVersion={#AppVersion}
AppVerName=Blade Lite {#AppVersion}
AppPublisherURL={#RepoUrl}
AppSupportURL={#RepoUrl}/issues
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
DefaultDirName={autopf}\Blade Lite
DisableDirPage=yes
DisableProgramGroupPage=yes
; Restart Manager's close only hides the window, and its kill sends no
; quit preset (ticket 142): the [Code] part quits the app instead.
CloseApplications=no
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 11.
MinVersion=10.0.22000
UninstallDisplayIcon={app}\blade-lite.exe
UninstallDisplayName=Blade Lite
OutputDir=..\target\installer
OutputBaseFilename=blade-lite-setup-{#AppVersion}
SetupLogging=yes
WizardStyle=modern
Compression=lzma2
SolidCompression=yes

[Messages]
FinishedLabel=Blade Lite is installed.%n%nIf the keyboard lights go off or blink, see "If the keyboard lights go off or blink" in the README on the Blade Lite GitHub page.

[Tasks]
; Only at the first install: an upgrade keeps "Start with Windows" as the
; tray menu left it.
Name: autostart; Description: "Start Blade Lite when I sign in"; Check: not IsUpgrade

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; DestName: "blade-lite.exe"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD_PARTY_LICENSES.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Blade Lite"; Filename: "{app}\blade-lite.exe"

[Registry]
; As the tray menu's "Start with Windows" writes it. The uninstall removes it
; in [Code], also when the tray menu set it.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Blade Lite"; ValueData: """{app}\blade-lite.exe"" --tray"; Tasks: autostart

[Run]
Filename: "{app}\blade-lite.exe"; Description: "Start Blade Lite"; Flags: postinstall nowait skipifsilent

[Code]
const
  AppMutex = 'Global\blade-lite';
  QuitFlag = '--quit';
  ResetFlag = '--reset';
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  ApprovedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run';
  RunValue = 'Blade Lite';
  { `--quit` exit codes (README): 0 done, 1 failed, 2 still quitting
    (`STILL_QUITTING` in blade-core). }
  QuitDone = 0;
  QuitFailed = 1;
  QuitStillQuitting = 2;
  { Held while a setup or the uninstaller runs, so two never run at once. }
  SetupMutexName = 'blade-lite-setup';
  { A silent run asks a copy that is still quitting this many times. Each
    `--quit` waits up to 140 s. }
  SilentQuitAsks = 3;
  { How long to wait for the lock to be free after a copy was ended. }
  LockWaitMs = 10000;
  LockPollMs = 100;

{ Inno's uninstall key of this app, for this user. }
function UninstallKey(): String;
begin
  Result := 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{' + '{#AppGuid}' + '}_is1';
end;

{ The version of the Blade Lite installed for this user, or '' if none. }
function InstalledVersion(): String;
begin
  if not RegQueryStringValue(HKCU, UninstallKey(), 'DisplayVersion', Result) then
    Result := '';
  Log('Installed version: "' + Result + '"');
end;

function IsUpgrade(): Boolean;
begin
  Result := InstalledVersion() <> '';
end;

function Silent(): Boolean;
begin
  if IsUninstaller() then
    Result := UninstallSilent()
  else
    Result := WizardSilent();
end;

{ Part Index (0 = major) of a version such as 0.1.0, or -1 if it is not a
  number. }
function VersionPart(Version: String; Index: Integer): Integer;
var
  Dot: Integer;
begin
  while Index > 0 do begin
    Dot := Pos('.', Version);
    if Dot = 0 then
      Version := ''
    else
      Version := Copy(Version, Dot + 1, Length(Version));
    Index := Index - 1;
  end;
  Dot := Pos('.', Version);
  if Dot > 0 then
    Version := Copy(Version, 1, Dot - 1);
  Result := StrToIntDef(Version, -1);
end;

{ Whether Older may be a lower version than Newer. A part that is not a
  number on either side counts as maybe lower, so it is asked about. }
function IsLowerVersion(Older, Newer: String): Boolean;
var
  Index, A, B: Integer;
begin
  Result := False;
  for Index := 0 to 2 do begin
    A := VersionPart(Older, Index);
    B := VersionPart(Newer, Index);
    if (A < 0) or (B < 0) then begin
      Result := True;
      Exit;
    end;
    if A <> B then begin
      Result := A < B;
      Exit;
    end;
  end;
end;

{ Takes the setup lock for this run. False, with a box, if another setup
  or uninstall holds it: two runs could end each other's tasks. }
function TakeSetupLock(): Boolean;
begin
  Result := not CheckForMutexes(SetupMutexName);
  if Result then
    CreateMutex(SetupMutexName)
  else
    SuppressibleMsgBox('Another Blade Lite setup or uninstall runs. Let it finish first.',
      mbError, MB_OK, IDOK);
end;

function InitializeUninstall(): Boolean;
begin
  Result := TakeSetupLock();
end;

{ An older version drops the settings it does not know at its next save
  (config.toml has no version field), so a downgrade is asked first. A
  silent run does not downgrade. }
function InitializeSetup(): Boolean;
var
  Installed: String;
begin
  Result := TakeSetupLock();
  if not Result then
    Exit;
  Installed := InstalledVersion();
  if (Installed <> '') and IsLowerVersion('{#AppVersion}', Installed) then
    Result := SuppressibleMsgBox('Blade Lite ' + Installed + ' is installed. This may be the older '
      + '{#AppVersion}. It forgets the settings it does not know the next time it saves them.'
      + #13#10#13#10 + 'Install {#AppVersion} anyway?',
      mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES;
end;

{ Runs Exe with one flag, hidden, and waits. Returns its exit code, or -1
  if it did not start. }
function RunApp(const Exe, Flag: String): Integer;
var
  Code: Integer;
begin
  if Exec(Exe, Flag, '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Result := Code
  else begin
    Log('Could not start ' + Exe + ': ' + SysErrorMessage(Code));
    Result := -1;
  end;
  Log(Exe + ' ' + Flag + ' gave ' + IntToStr(Result));
end;

{ Whether to ask a copy that is still quitting once more. It is never
  ended: it may be sending its quit preset. }
function AskAgain(Asks: Integer): Boolean;
begin
  if Silent() then
    Result := Asks < SilentQuitAsks
  else
    Result := MsgBox('Blade Lite is still quitting. If its tray menu is open, close it.'
      + #13#10#13#10 + 'Then click Retry.', mbError, MB_RETRYCANCEL) = IDRETRY;
end;

{ Ends every blade-lite.exe of this user. Only after a `--quit` that ran
  and gave 1 while a copy still holds the lock: that copy did not take the
  ask, so the reset below must then work. }
procedure EndCopies();
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'),
    '/F /IM blade-lite.exe /FI "USERNAME eq ' + GetUserNameString() + '"',
    '', SW_HIDE, ewWaitUntilTerminated, Code);
  Log('taskkill gave ' + IntToStr(Code));
end;

{ Whether the one-copy lock is free within LockWaitMs. }
function LockFree(): Boolean;
var
  Waited: Integer;
begin
  Waited := 0;
  while CheckForMutexes(AppMutex) and (Waited < LockWaitMs) do begin
    Sleep(LockPollMs);
    Waited := Waited + LockPollMs;
  end;
  Result := not CheckForMutexes(AppMutex);
end;

function ResetFailedText(Code: Integer): String;
begin
  Result := 'Blade Lite could not set the fans to auto (exit code ' + IntToStr(Code)
    + '). Its log in ' + ExpandConstant('{userappdata}\blade-lite') + ' says why.';
end;

procedure WarnResetFailed(Code: Integer);
var
  Text: String;
begin
  Text := ResetFailedText(Code) + #13#10#13#10
    + 'If the fans stay fast or slow, see docs/recovery.md on the Blade Lite GitHub page.';
  Log(Text);
  if not Silent() then
    MsgBox(Text, mbInformation, MB_OK);
end;

{ Runs `--quit` until it gives no "still quitting". Returns '' with its
  last code in Code, or why it stopped. }
function AskToQuit(const Exe: String; var Code: Integer): String;
var
  Asks: Integer;
begin
  Result := '';
  Asks := 0;
  repeat
    Code := RunApp(Exe, QuitFlag);
    Asks := Asks + 1;
    if (Code = QuitStillQuitting) and not AskAgain(Asks) then begin
      Result := 'Blade Lite is still quitting. Wait a minute, then run this again.';
      Exit;
    end;
  until Code <> QuitStillQuitting;
end;

{ Ends the running copy so that it sends its quit preset, then sets the
  fans to auto (ticket 233). Exe must know --quit and --reset (ticket 148
  or later). Returns '' when no copy runs any more and the fans are safe,
  else why not: the caller then stops, with nothing changed. }
function StopApp(const Exe: String): String;
var
  Code: Integer;
  Clean: Boolean;
begin
  Result := AskToQuit(Exe, Code);
  if Result <> '' then
    Exit;
  { A `--quit` that failed after its ask (an error gives 1) may have left a
    copy quitting: ask once more, which then gives 2 or 0. }
  if (Code = QuitFailed) and CheckForMutexes(AppMutex) then begin
    Result := AskToQuit(Exe, Code);
    if Result <> '' then
      Exit;
  end;
  { An exe that did not start asked nothing: end no copy. }
  if (Code <> QuitDone) and (Code <> QuitFailed) then begin
    Result := 'Blade Lite could not be asked to quit (code ' + IntToStr(Code)
      + '). Quit it from its tray menu, then run this again.';
    Exit;
  end;
  { 0: no copy ran, or it quit with its preset sent. }
  Clean := (Code = QuitDone) and not CheckForMutexes(AppMutex);
  if (Code <> QuitDone) and CheckForMutexes(AppMutex) then
    EndCopies();
  if not LockFree() then begin
    Result := 'Blade Lite is still running. Quit it from its tray menu, then run this again.';
    Exit;
  end;
  Code := RunApp(Exe, ResetFlag);
  if Code = 0 then
    Exit;
  { A copy that quit without its preset, or was ended, may have left a
    manual fan: stop rather than go on without the reset. }
  if not Clean then
    Result := ResetFailedText(Code) + #13#10#13#10
      + 'Start Blade Lite and pick Balanced, then run this again.'
  else
    WarnResetFailed(Code);
end;

{ An upgrade, or a copy running from anywhere: quit it with the new exe,
  which knows --quit. A fresh install with no copy sends nothing. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if IsUpgrade() or CheckForMutexes(AppMutex) then begin
    ExtractTemporaryFile('blade-lite.exe');
    Result := StopApp(ExpandConstant('{tmp}\blade-lite.exe'));
  end;
end;

{ The Run value, if it starts this install's exe; a value of another copy
  (such as a build in target\debug) stays. }
procedure RemoveRunValue();
var
  Value: String;
begin
  if RegQueryStringValue(HKCU, RunKey, RunValue, Value)
    and (Pos(Lowercase('"' + ExpandConstant('{app}\blade-lite.exe') + '"'), Lowercase(Value)) = 1) then begin
    RegDeleteValue(HKCU, RunKey, RunValue);
    RegDeleteValue(HKCU, ApprovedKey, RunValue);
  end;
end;

{ Asks whether to keep the settings and the log (ticket 78). A silent
  uninstall keeps them. }
procedure AskToRemoveSettings();
var
  Folder: String;
begin
  Folder := ExpandConstant('{userappdata}\blade-lite');
  if DirExists(Folder) and not Silent() then
    if MsgBox('Keep your Blade Lite settings and log?' + #13#10#13#10 + Folder + #13#10#13#10
      + 'Click No to delete them.', mbConfirmation, MB_YESNO) = IDNO then
      DelTree(Folder, True, True, True);
end;

procedure ShowWhatStays();
begin
  if not Silent() then
    MsgBox('Blade Lite is removed. These stay as they are:' + #13#10#13#10
      + '- The laptop keeps the charge limit, the keyboard colour and the screen''s native mode. '
      + 'The lid logo comes on again at the next sleep.' + #13#10
      + '- If you turned the touchpad off, Windows keeps it off.' + #13#10
      + '- If you hid the Razer drivers from Windows Update, run '
      + 'tools\Block-RazerDriverUpdates.ps1 -Undo to show them again.' + #13#10
      + '- If you turned USB suspend off, run tools\Set-RazerUsbSuspend.ps1 -Undo to turn it on again.'
      + #13#10#13#10 + 'The scripts are on {#RepoUrl}.', mbInformation, MB_OK);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Error: String;
begin
  case CurUninstallStep of
    usAppMutexCheck:
      begin
        Error := StopApp(ExpandConstant('{app}\blade-lite.exe'));
        if Error <> '' then begin
          Log(Error);
          if not Silent() then
            MsgBox(Error + #13#10#13#10 + 'Nothing was removed.', mbError, MB_OK);
          Abort();
        end;
      end;
    usUninstall:
      RemoveRunValue();
    usPostUninstall:
      begin
        AskToRemoveSettings();
        ShowWhatStays();
      end;
  end;
end;
