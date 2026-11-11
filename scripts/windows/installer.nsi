; The Windows installer of the desktop app. scripts/build-windows-installer.sh compiles it
; with makensis and passes every name as a define, so the product name stays in product.json:
;
;   NAME      product name (the app is <NAME>.exe)
;   SLUG      product slug (<SLUG>-update.exe, <SLUG>-daemon.exe)
;   APP_ID    bundle id: the AppUserModelID and the key of the uninstall entry
;   SCHEME    URL scheme
;   VERSION   MAJOR.MINOR.PATCH        VERSION4  MAJOR.MINOR.PATCH.0
;   STAGE     folder with the files to install     ICON  the .ico     OUT  the installer to write
;   WEBVIEW2  (optional) path of the WebView2 Evergreen bootstrapper
;   SIGN_CMD  (optional) command that signs the file given as its last argument; used for
;             the uninstaller, which exists only while makensis runs
;   HAS_DAEMON (optional) set when STAGE holds the machine daemon
;   CACHE_DIR folder name under %LOCALAPPDATA% where the app keeps what it can make again
;             (the WebView2 user data and the drawn diagrams)
;
; Per user, no administrator rights: files go to %LOCALAPPDATA%\Programs\<NAME>, registry
; entries to HKEY_CURRENT_USER. Silent install: <setup>.exe /S [/D=<folder>]. The update
; helper runs it as <setup>.exe /S /UPDATE /D=<folder> after it has moved the old files away.

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "WinVer.nsh"
!include "x64.nsh"

!define APP_EXE "${NAME}.exe"
!define HELPER_EXE "${SLUG}-update.exe"
!define DAEMON_EXE "${SLUG}-daemon.exe"
!define UNINSTALLER "uninstall.exe"
!define INSTALL_LOG "install.log"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
!define APP_ID_KEY "Software\Classes\AppUserModelId\${APP_ID}"
!define SCHEME_KEY "Software\Classes\${SCHEME}"
; Windows 10 version 22H2.
!define MIN_BUILD 19045
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define STARTUP_APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
; The WebView2 Runtime's entry in Edge Update.
!define WEBVIEW2_CLIENT "Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

Name "${NAME}"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\${NAME}"
; A reinstall goes where the app is.
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
BrandingText " "
ShowInstDetails nevershow
ShowUninstDetails nevershow

VIProductVersion "${VERSION4}"
VIAddVersionKey /LANG=0 "ProductName" "${NAME}"
VIAddVersionKey /LANG=0 "FileDescription" "${NAME}"
VIAddVersionKey /LANG=0 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "ProductVersion" "${VERSION}"

!ifdef SIGN_CMD
  !uninstfinalize '${SIGN_CMD} "%1"' = 0
!endif

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"

!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; The first language is the one used when the system's language is not in the list.
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

LangString NeedWindows10 ${LANG_SIMPCHINESE} "${NAME} 需要 64 位的 Windows 10（22H2）或 Windows 11。请先更新 Windows。"
LangString NeedWindows10 ${LANG_ENGLISH} "${NAME} needs 64-bit Windows 10 (version 22H2) or Windows 11. Update Windows first."
LangString NeedWindows11OnArm ${LANG_SIMPCHINESE} "在 ARM 芯片的电脑上，${NAME} 需要 Windows 11（Windows 10 不能运行 64 位 x86 程序）。"
LangString NeedWindows11OnArm ${LANG_ENGLISH} "On a computer with an ARM processor, ${NAME} needs Windows 11 (Windows 10 cannot run 64-bit x86 programs)."
LangString QuitAppFirst ${LANG_SIMPCHINESE} "${NAME} 还在运行。请先退出，再点「重试」。"
LangString QuitAppFirst ${LANG_ENGLISH} "${NAME} is still running. Quit it, then choose Retry."

Var IsUpdate
Var Log

; Appends one line to the install log.
!macro Log TEXT
  ${If} $Log != ""
    FileWrite $Log "${TEXT}$\r$\n"
  ${EndIf}
!macroend

; Stops until the program FILE of the app can be written, which means it does not run. A silent
; run does not wait: it ends with exit code 2.
!macro CheckClosed FILE
  ${Do}
    ${IfNot} ${FileExists} "$INSTDIR\${FILE}"
      ${ExitDo}
    ${EndIf}
    ClearErrors
    FileOpen $0 "$INSTDIR\${FILE}" a
    ${IfNot} ${Errors}
      FileClose $0
      ${ExitDo}
    ${EndIf}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(QuitAppFirst)" /SD IDCANCEL IDRETRY +2
    Abort
  ${Loop}
!macroend

; The names the product had before (product.json formerNames). An app installed under one of
; them has that name in its update helper, its shortcut and its sign-in entry: the helper
; starts <former name>.exe after the installer and rolls the update back when that file is
; missing. So every former name stays as a second name of the app's program, and its Start
; menu shortcut goes. FORMER_NSH defines the macro FormerNames, which runs its argument (one
; of the macros below) once for each former name.
!macro FormerNameClosed OLD
  !insertmacro CheckClosed "${OLD}.exe"
!macroend
!macro FormerNameInstall OLD
  Delete "$INSTDIR\${OLD}.exe"
  ; A hard link: the same file under a second name, no second copy on the disk.
  System::Call 'kernel32::CreateHardLinkW(w "$INSTDIR\${OLD}.exe", w "$INSTDIR\${APP_EXE}", p 0) i .r1'
  ${If} $1 == 0
    CopyFiles /SILENT "$INSTDIR\${APP_EXE}" "$INSTDIR\${OLD}.exe"
    !insertmacro Log "former name ${OLD}.exe: copy"
  ${Else}
    !insertmacro Log "former name ${OLD}.exe: link"
  ${EndIf}
  Delete "$SMPROGRAMS\${OLD}.lnk"
!macroend
!macro FormerNameUninstall OLD
  Delete "$INSTDIR\${OLD}.exe"
  Delete "$SMPROGRAMS\${OLD}.lnk"
  DeleteRegValue HKCU "${RUN_KEY}Once" "${OLD}.update-recovery"
!macroend
!ifdef FORMER_NSH
  !include "${FORMER_NSH}"
!else
  !macro FormerNames ACTION
  !macroend
!endif

Function .onInit
  StrCpy $IsUpdate "0"
  StrCpy $Log ""
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/UPDATE" $1
  ${IfNot} ${Errors}
    StrCpy $IsUpdate "1"
  ${EndIf}
  ; 64-bit Windows 10 version 22H2 (build 19045) or later, as docs/INSTALL.md says: the
  ; oldest Windows 10 that still gets fixes, and the one the app is built against. Every
  ; Windows 11 has a higher build.
  ${IfNot} ${RunningX64}
  ${OrIfNot} ${AtLeastBuild} ${MIN_BUILD}
    MessageBox MB_OK|MB_ICONSTOP "$(NeedWindows10)" /SD IDOK
    Abort
  ${EndIf}
  ; The programs are x64. Windows 11 on ARM runs them under its x64 emulation; Windows 10 on
  ; ARM emulates only 32-bit x86, so this installer would start there and the app would not.
  ${If} ${IsNativeARM64}
  ${AndIfNot} ${AtLeastBuild} 22000
    MessageBox MB_OK|MB_ICONSTOP "$(NeedWindows11OnArm)" /SD IDOK
    Abort
  ${EndIf}
FunctionEnd

Section "Install"
  ; The update helper has waited for the app and emptied the folder already.
  ${If} $IsUpdate == "0"
    !insertmacro CheckClosed "${APP_EXE}"
    !insertmacro FormerNames FormerNameClosed
  ${EndIf}

  SetOutPath "$INSTDIR"
  !ifdef HAS_DAEMON
    File /r /x "${DAEMON_EXE}" "${STAGE}\*.*"
    ; The machine daemon may be running a bot, so it is replaced by its own protocol
    ; (docs/machine-protocol.md) and never stopped: the whole new program is written beside
    ; it, the running file is renamed (a rename works where an overwrite does not), and the
    ; new file takes the name. The daemon sees the new file, exits when idle, its task
    ; starts the new one, and a starting daemon removes the .old-* files next to it.
    File "/oname=$INSTDIR\${DAEMON_EXE}.new" "${STAGE}\${DAEMON_EXE}"
    StrCpy $1 ""
    ${If} ${FileExists} "$INSTDIR\${DAEMON_EXE}"
      System::Call 'kernel32::GetTickCount()i.r1'
      Rename "$INSTDIR\${DAEMON_EXE}" "$INSTDIR\${DAEMON_EXE}.old-$1"
    ${EndIf}
    ClearErrors
    Rename "$INSTDIR\${DAEMON_EXE}.new" "$INSTDIR\${DAEMON_EXE}"
    ${If} ${Errors}
      ; Put the old program back and stop: the update helper rolls the rest back.
      ${If} $1 != ""
        Rename "$INSTDIR\${DAEMON_EXE}.old-$1" "$INSTDIR\${DAEMON_EXE}"
      ${EndIf}
      Abort
    ${EndIf}
    ${If} $1 != ""
      ; Gone at once when no daemon runs from it.
      Delete "$INSTDIR\${DAEMON_EXE}.old-$1"
    ${EndIf}
  !else
    File /r "${STAGE}\*.*"
  !endif
  WriteUninstaller "$INSTDIR\${UNINSTALLER}"

  ClearErrors
  FileOpen $Log "$INSTDIR\${INSTALL_LOG}" w
  ${If} ${Errors}
    StrCpy $Log ""
  ${EndIf}
  !insertmacro Log "version ${VERSION}"
  !insertmacro Log "update $IsUpdate"

  ; WebView2 Runtime (the 产出 window and diagrams): Windows 11 has it, Windows 10 may not.
  ; Machine-wide installs are under HKLM (this 32-bit installer reads the 32-bit view, where
  ; Edge Update writes), per-user installs under HKCU.
  ReadRegStr $1 HKLM "${WEBVIEW2_CLIENT}" "pv"
  ${If} $1 == ""
  ${OrIf} $1 == "0.0.0.0"
    ReadRegStr $1 HKCU "${WEBVIEW2_CLIENT}" "pv"
  ${EndIf}
  ${If} $1 != ""
  ${AndIf} $1 != "0.0.0.0"
    !insertmacro Log "webview2 found $1"
  ${Else}
    !ifdef WEBVIEW2
      InitPluginsDir
      File "/oname=$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2}"
      ; No rights needed: without them the bootstrapper installs for this user. A failure
      ; (no network) does not stop the install; the app says so when it needs the runtime.
      ExecWait '"$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" /silent /install' $1
      !insertmacro Log "webview2 bootstrapper exit $1"
    !else
      !insertmacro Log "webview2 missing, no bootstrapper in this installer"
    !endif
  ${EndIf}

  !insertmacro FormerNames FormerNameInstall

  ; Start menu shortcut with the AppUserModelID: Windows shows the app's notifications
  ; under the shortcut that carries the id the app sets for its process.
  nsExec::Exec '"$INSTDIR\${HELPER_EXE}" --shortcut "$SMPROGRAMS\${NAME}.lnk" --target "$INSTDIR\${APP_EXE}" --app-id "${APP_ID}"'
  Pop $1
  ${If} $1 == "0"
    !insertmacro Log "shortcut with app id ${APP_ID}"
  ${Else}
    CreateShortcut "$SMPROGRAMS\${NAME}.lnk" "$INSTDIR\${APP_EXE}"
    !insertmacro Log "shortcut without app id (helper exit $1)"
  ${EndIf}
  WriteRegStr HKCU "${APP_ID_KEY}" "DisplayName" "${NAME}"

  ; <scheme>:// links open the app.
  WriteRegStr HKCU "${SCHEME_KEY}" "" "URL:${NAME}"
  WriteRegStr HKCU "${SCHEME_KEY}" "URL Protocol" ""
  WriteRegStr HKCU "${SCHEME_KEY}\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "${SCHEME_KEY}\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'

  ; The entry in 应用和功能.
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${NAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $1 $2 $3
  IntFmt $1 "0x%08X" $1
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$1"

  !insertmacro Log "done"
  ${If} $Log != ""
    FileClose $Log
  ${EndIf}
SectionEnd

Section "Uninstall"
  !insertmacro CheckClosed "${APP_EXE}"
  !insertmacro FormerNames FormerNameClosed
  !ifdef HAS_DAEMON
    ; The machine daemon of this install: remove its task (用这台电脑), then stop it; nothing
    ; else has this file name.
    ${If} ${FileExists} "$INSTDIR\${DAEMON_EXE}"
      nsExec::Exec '"$INSTDIR\${DAEMON_EXE}" uninstall-service'
      Pop $1
    ${EndIf}
    nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "${DAEMON_EXE}"'
    Pop $1
    Delete "$INSTDIR\${DAEMON_EXE}"
    Delete "$INSTDIR\${DAEMON_EXE}.new"
    FindFirst $2 $3 "$INSTDIR\${DAEMON_EXE}.old-*"
    ${DoWhile} $3 != ""
      Delete "$INSTDIR\$3"
      FindNext $2 $3
    ${Loop}
    FindClose $2
  !endif
  Delete "$INSTDIR\${APP_EXE}"
  !insertmacro FormerNames FormerNameUninstall
  Delete "$INSTDIR\${HELPER_EXE}"
  Delete "$INSTDIR\${INSTALL_LOG}"
  Delete "$INSTDIR\${UNINSTALLER}"
  ; Only the files the installer wrote, then the folder when nothing else is in it.
  RMDir "$INSTDIR"

  ; Backups an update left next to the install folder (.<folder>.old-<version>). The folder
  ; keeps the name the app was first installed under.
  ${GetParent} "$INSTDIR" $1
  ${GetFileName} "$INSTDIR" $4
  FindFirst $2 $3 "$1\.$4.old-*"
  ${DoWhile} $3 != ""
    RMDir /r "$1\$3"
    FindNext $2 $3
  ${Loop}
  FindClose $2

  Delete "$SMPROGRAMS\${NAME}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "${SCHEME_KEY}"
  DeleteRegKey HKCU "${APP_ID_KEY}"
  ; 登录时打开, when the user turned it on in the app, and what Task Manager remembers of
  ; that entry (enabled or disabled there).
  DeleteRegValue HKCU "${RUN_KEY}" "${APP_ID}"
  DeleteRegValue HKCU "${STARTUP_APPROVED_KEY}" "${APP_ID}"
  ; What the app can make again: the WebView2 user data (产出 pages, diagrams) and the cache
  ; of drawn diagrams. The folder itself goes only when nothing else is in it.
  RMDir /r "$LOCALAPPDATA\${CACHE_DIR}\webview2"
  RMDir /r "$LOCALAPPDATA\${CACHE_DIR}\diagrams"
  RMDir "$LOCALAPPDATA\${CACHE_DIR}"
  ; The recovery of an update that was cut short (the update helper registers it).
  DeleteRegValue HKCU "${RUN_KEY}Once" "${NAME}.update-recovery"
  ; What is the user's stays, as it does on a Mac when the app goes to the Trash: messages,
  ; settings and logs (%APPDATA%\<APP_ID>), the sign-in (Credential Manager), and this
  ; computer's identity as a machine of the team (%USERPROFILE%\.config\<SLUG>, with the
  ; daemon's log in %LOCALAPPDATA%\<SLUG>-daemon), so a reinstall carries on where it was.
  ; docs/INSTALL.md says how to remove them.
SectionEnd
