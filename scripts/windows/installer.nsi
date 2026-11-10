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
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
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

LangString NeedWindows10 ${LANG_SIMPCHINESE} "${NAME} 需要 64 位的 Windows 10 或更高版本。"
LangString NeedWindows10 ${LANG_ENGLISH} "${NAME} needs 64-bit Windows 10 or later."
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

; Stops until the app's program can be written, which means it does not run. A silent run
; does not wait: it ends with exit code 2.
!macro CheckAppClosed
  ${Do}
    ${IfNot} ${FileExists} "$INSTDIR\${APP_EXE}"
      ${ExitDo}
    ${EndIf}
    ClearErrors
    FileOpen $0 "$INSTDIR\${APP_EXE}" a
    ${IfNot} ${Errors}
      FileClose $0
      ${ExitDo}
    ${EndIf}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(QuitAppFirst)" /SD IDCANCEL IDRETRY +2
    Abort
  ${Loop}
!macroend

Function .onInit
  StrCpy $IsUpdate "0"
  StrCpy $Log ""
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/UPDATE" $1
  ${IfNot} ${Errors}
    StrCpy $IsUpdate "1"
  ${EndIf}
  ${IfNot} ${RunningX64}
  ${OrIfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "$(NeedWindows10)" /SD IDOK
    Abort
  ${EndIf}
FunctionEnd

Section "Install"
  ; The update helper has waited for the app and emptied the folder already.
  ${If} $IsUpdate == "0"
    !insertmacro CheckAppClosed
  ${EndIf}

  SetOutPath "$INSTDIR"
  !ifdef HAS_DAEMON
    ; A daemon that runs keeps its file open: a rename works, an overwrite does not.
    ${If} ${FileExists} "$INSTDIR\${DAEMON_EXE}"
      Delete "$INSTDIR\${DAEMON_EXE}.old"
      Rename "$INSTDIR\${DAEMON_EXE}" "$INSTDIR\${DAEMON_EXE}.old"
    ${EndIf}
  !endif
  File /r "${STAGE}\*.*"
  !ifdef HAS_DAEMON
    Delete "$INSTDIR\${DAEMON_EXE}.old"
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
  !insertmacro CheckAppClosed
  !ifdef HAS_DAEMON
    ; The machine daemon of this install; nothing else has this file name.
    nsExec::Exec 'taskkill /F /IM "${DAEMON_EXE}"'
    Pop $1
    Delete "$INSTDIR\${DAEMON_EXE}"
    Delete "$INSTDIR\${DAEMON_EXE}.old"
  !endif
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\${HELPER_EXE}"
  Delete "$INSTDIR\${INSTALL_LOG}"
  Delete "$INSTDIR\${UNINSTALLER}"
  ; Only the files the installer wrote, then the folder when nothing else is in it.
  RMDir "$INSTDIR"

  ; Backups an update left next to the install folder (.<NAME>.old-<version>).
  ${GetParent} "$INSTDIR" $1
  FindFirst $2 $3 "$1\.${NAME}.old-*"
  ${DoWhile} $3 != ""
    RMDir /r "$1\$3"
    FindNext $2 $3
  ${Loop}
  FindClose $2

  Delete "$SMPROGRAMS\${NAME}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "${SCHEME_KEY}"
  DeleteRegKey HKCU "${APP_ID_KEY}"
  ; 登录时打开, when the user turned it on in the app.
  DeleteRegValue HKCU "${RUN_KEY}" "${APP_ID}"
  ; Messages, settings and the sign-in stay (%APPDATA%\<APP_ID>): docs/INSTALL.md says how
  ; to remove them.
SectionEnd
