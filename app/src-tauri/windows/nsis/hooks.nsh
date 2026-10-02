; Nail's installer logic (plan M2b WP44, M4b WP112; design "Windows
; installer & distribution"; ADRs 0040, 0087), hooked into installer.nsi
; (Nail's copy of the Tauri template). Everything is per-user (HKCU) and
; needs no admin rights, except the optional Simple MAPI registration.
;
;   * Windows registration: ProgIDs for mailto / webcal / webcals and the
;     .eml / .ics / .vcf types, "Open with" entries, and Capabilities listed
;     under RegisteredApplications, so Nail appears in Settings > Default
;     apps. Nail never takes an association over itself: the user chooses.
;   * Installation facts the app reads (nail_api::desktop::registry):
;     HKCU\Software\Nail\Install with Installer = nsis, Scope = per-user,
;     InstallDir, Version, Mapi. Rewritten by every install and update.
;   * Options page (NAIL_PAGE_OPTIONS): start with Windows (HKCU Run key,
;     `--autostart`), `nail` CLI on the per-user PATH (via
;     `Nail.exe --setup-path-add`, which keeps long PATHs intact), and the
;     Simple MAPI provider. Silent installs: /AUTOSTART=0|1, /ADDTOPATH=0|1,
;     /MAPI=0|1.
;   * Simple MAPI (nail_api::mapi::registry): Windows reads the provider from
;     HKLM\Software\Clients\Mail\Nail (64-bit view: mapi\<arch>\NailMapi.dll,
;     32-bit view: mapi\x86\NailMapi.dll). Only this component needs admin
;     rights: the installer writes a .reg file and imports it with one
;     elevated reg.exe (one UAC prompt); declining leaves Nail installed
;     without it. Updates (/UPDATE) keep the registration as it is.
;   * Uninstall keeps settings, mail data and credentials unless the user
;     ticks the checkbox on the confirm page (silent: /DELETEDATA); then
;     %APPDATA%\Nail, %LOCALAPPDATA%\Nail and Nail's Credential Manager
;     entries (`Nail.exe --setup-purge-credentials`) are removed too. A MAPI
;     registration that points into this installation is removed (one UAC
;     prompt).
;
; User-visible text comes from languages\English.nsh ($(nail...) strings).
; These macros are expanded inside installer.nsi, after its !defines
; (PRODUCTNAME, MAINBINARYNAME, UNINSTKEY, VERSION, ARCH), so they may use
; them.

!include nsDialogs.nsh

!define NAIL_LANGUAGE_DIR "${__FILEDIR__}\languages"

; Registry locations (all under HKCU unless named otherwise).
!define NAIL_CAPABILITIES "Software\Nail\Capabilities"
!define NAIL_REGISTERED_APPS "Software\RegisteredApplications"
!define NAIL_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define NAIL_CLASSES "Software\Classes"
; Installation facts (nail_api::desktop::registry::INSTALL_KEY).
!define NAIL_INSTALL_KEY "Software\Nail\Install"
; Simple MAPI client (nail_api::mapi::registry::NAIL_MAIL_CLIENT_KEY), in the
; machine hive; the .reg files spell the hive out.
!define NAIL_MAPI_CLIENT_KEY "Software\Clients\Mail\Nail"
!define NAIL_MAPI_REG_NATIVE "HKEY_LOCAL_MACHINE\SOFTWARE\Clients\Mail\Nail"
!define NAIL_MAPI_REG_WOW "HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Clients\Mail\Nail"
; ProgIDs.
!define NAIL_PROGID_MAILTO "Nail.Url.mailto"
!define NAIL_PROGID_WEBCAL "Nail.Url.webcal"
!define NAIL_PROGID_EML "Nail.eml"
!define NAIL_PROGID_ICS "Nail.ics"
!define NAIL_PROGID_VCF "Nail.vcf"

Var NailOptionsRead
Var NailAutostart
Var NailAddToPath
Var NailMapi
Var NailAutostartBox
Var NailPathBox
Var NailMapiBox

; ---------------------------------------------------------------------------
; Registration helpers

; Open commands are `"Nail.exe" -- "%1"`: after `--` every argument is a
; launch target, so a crafted URL that breaks out of the quotes cannot pass
; options such as --setup-purge-credentials.

; A URL ProgID for the URL.
!macro NAIL_URL_PROGID PROGID NAME
  WriteRegStr HKCU "${NAIL_CLASSES}\${PROGID}" "" "${NAME}"
  WriteRegStr HKCU "${NAIL_CLASSES}\${PROGID}" "FriendlyTypeName" "${NAME}"
  WriteRegStr HKCU "${NAIL_CLASSES}\${PROGID}" "URL Protocol" ""
  WriteRegDWORD HKCU "${NAIL_CLASSES}\${PROGID}" "EditFlags" 2
  WriteRegStr HKCU "${NAIL_CLASSES}\${PROGID}\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr HKCU "${NAIL_CLASSES}\${PROGID}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" -- $\"%1$\""
!macroend

; A file-type ProgID plus an "Open with" entry for the extension. The
; extension's default (the user's choice) is never touched.
!macro NAIL_FILE_PROGID PROGID EXT NAME
  WriteRegStr HKCU "${NAIL_CLASSES}\${PROGID}" "" "${NAME}"
  WriteRegStr HKCU "${NAIL_CLASSES}\${PROGID}" "FriendlyTypeName" "${NAME}"
  WriteRegStr HKCU "${NAIL_CLASSES}\${PROGID}\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr HKCU "${NAIL_CLASSES}\${PROGID}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" -- $\"%1$\""
  WriteRegStr HKCU "${NAIL_CLASSES}\${EXT}\OpenWithProgids" "${PROGID}" ""
  WriteRegStr HKCU "${NAIL_CAPABILITIES}\FileAssociations" "${EXT}" "${PROGID}"
!macroend

!macro NAIL_REMOVE_FILE_PROGID PROGID EXT
  DeleteRegKey HKCU "${NAIL_CLASSES}\${PROGID}"
  DeleteRegValue HKCU "${NAIL_CLASSES}\${EXT}\OpenWithProgids" "${PROGID}"
  DeleteRegKey /ifempty HKCU "${NAIL_CLASSES}\${EXT}\OpenWithProgids"
  DeleteRegKey /ifempty HKCU "${NAIL_CLASSES}\${EXT}"
!macroend

!macro NAIL_REGISTER
  !insertmacro NAIL_URL_PROGID "${NAIL_PROGID_MAILTO}" "$(nailMailtoName)"
  !insertmacro NAIL_URL_PROGID "${NAIL_PROGID_WEBCAL}" "$(nailWebcalName)"
  !insertmacro NAIL_FILE_PROGID "${NAIL_PROGID_EML}" ".eml" "$(nailEmlName)"
  !insertmacro NAIL_FILE_PROGID "${NAIL_PROGID_ICS}" ".ics" "$(nailIcsName)"
  !insertmacro NAIL_FILE_PROGID "${NAIL_PROGID_VCF}" ".vcf" "$(nailVcfName)"

  WriteRegStr HKCU "${NAIL_CAPABILITIES}" "ApplicationName" "${PRODUCTNAME}"
  WriteRegStr HKCU "${NAIL_CAPABILITIES}" "ApplicationDescription" "$(nailAppDescription)"
  WriteRegStr HKCU "${NAIL_CAPABILITIES}" "ApplicationIcon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr HKCU "${NAIL_CAPABILITIES}\URLAssociations" "mailto" "${NAIL_PROGID_MAILTO}"
  WriteRegStr HKCU "${NAIL_CAPABILITIES}\URLAssociations" "webcal" "${NAIL_PROGID_WEBCAL}"
  WriteRegStr HKCU "${NAIL_CAPABILITIES}\URLAssociations" "webcals" "${NAIL_PROGID_WEBCAL}"
  WriteRegStr HKCU "${NAIL_REGISTERED_APPS}" "${PRODUCTNAME}" "${NAIL_CAPABILITIES}"

  ; SHCNE_ASSOCCHANGED: Explorer reloads the associations.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NAIL_UNREGISTER
  DeleteRegValue HKCU "${NAIL_REGISTERED_APPS}" "${PRODUCTNAME}"
  DeleteRegKey HKCU "${NAIL_CAPABILITIES}"
  DeleteRegKey HKCU "${NAIL_INSTALL_KEY}"
  DeleteRegKey /ifempty HKCU "Software\Nail"
  DeleteRegKey HKCU "${NAIL_CLASSES}\${NAIL_PROGID_MAILTO}"
  DeleteRegKey HKCU "${NAIL_CLASSES}\${NAIL_PROGID_WEBCAL}"
  !insertmacro NAIL_REMOVE_FILE_PROGID "${NAIL_PROGID_EML}" ".eml"
  !insertmacro NAIL_REMOVE_FILE_PROGID "${NAIL_PROGID_ICS}" ".ics"
  !insertmacro NAIL_REMOVE_FILE_PROGID "${NAIL_PROGID_VCF}" ".vcf"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

; The installation facts (nail_api::desktop::registry). Mapi is written by
; the MAPI step; DisableUpdates is the MSI's (a per-user install updates
; itself unless a policy says otherwise).
!macro NAIL_WRITE_INSTALL_FACTS
  WriteRegStr HKCU "${NAIL_INSTALL_KEY}" "Installer" "nsis"
  WriteRegStr HKCU "${NAIL_INSTALL_KEY}" "Scope" "per-user"
  WriteRegStr HKCU "${NAIL_INSTALL_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${NAIL_INSTALL_KEY}" "Version" "${VERSION}"
!macroend

; ---------------------------------------------------------------------------
; Simple MAPI (installer side). One elevated `reg.exe import` per change:
; reg.exe is told /reg:64, and the 32-bit view is addressed as
; WOW6432Node, so one file (and one UAC prompt) covers both views even from
; this 32-bit installer.

!macro NAIL_MAPI_FUNCTIONS
  ; $0 in: a path; $0 out: the path escaped for a .reg string ("\" -> "\\",
  ; quote -> backslash quote).
  Function NailRegEscape
    Push $1
    Push $2
    Push $3
    StrCpy $1 ""
    StrCpy $2 0
    ${Do}
      StrCpy $3 $0 1 $2
      ${If} $3 == ""
        ${Break}
      ${EndIf}
      ${If} $3 == "\"
        StrCpy $1 "$1\\"
      ${ElseIf} $3 == '"'
        StrCpy $1 '$1\"'
      ${Else}
        StrCpy $1 "$1$3"
      ${EndIf}
      IntOp $2 $2 + 1
    ${Loop}
    StrCpy $0 $1
    Pop $3
    Pop $2
    Pop $1
  FunctionEnd

  ; $0 = 1 when HKLM already names this installation's DLL (64-bit view).
  Function NailMapiRegisteredHere
    SetRegView 64
    ReadRegStr $1 HKLM "${NAIL_MAPI_CLIENT_KEY}" "DLLPath"
    SetRegView lastused
    ${If} $1 == "$INSTDIR\mapi\${ARCH}\NailMapi.dll"
      StrCpy $0 1
    ${Else}
      StrCpy $0 0
    ${EndIf}
  FunctionEnd

  ; Writes $PLUGINSDIR\nail-mapi.reg (UTF-16LE, as regedit expects for
  ; "Version 5.00") and imports it elevated. Sets Mapi in the facts to what
  ; the registry says afterwards.
  Function NailRegisterMapi
    Call NailMapiRegisteredHere
    ${If} $0 = 1
      WriteRegDWORD HKCU "${NAIL_INSTALL_KEY}" "Mapi" 1
      Return
    ${EndIf}
    ${IfNot} ${FileExists} "$INSTDIR\mapi\${ARCH}\NailMapi.dll"
      DetailPrint "$(nailMapiMissing)"
      WriteRegDWORD HKCU "${NAIL_INSTALL_KEY}" "Mapi" 0
      Return
    ${EndIf}
    InitPluginsDir
    FileOpen $9 "$PLUGINSDIR\nail-mapi.reg" w
    FileWriteWord $9 0xFEFF
    FileWriteUTF16LE $9 "Windows Registry Editor Version 5.00$\r$\n$\r$\n"
    StrCpy $0 "$INSTDIR\mapi\${ARCH}\NailMapi.dll"
    Call NailRegEscape
    StrCpy $5 $0
    StrCpy $0 "$INSTDIR\mapi\x86\NailMapi.dll"
    Call NailRegEscape
    StrCpy $6 $0
    StrCpy $0 "$INSTDIR\${MAINBINARYNAME}.exe"
    Call NailRegEscape
    StrCpy $7 $0
    FileWriteUTF16LE $9 "[${NAIL_MAPI_REG_NATIVE}]$\r$\n@=$\"${PRODUCTNAME}$\"$\r$\n"
    FileWriteUTF16LE $9 "$\"DLLPath$\"=$\"$5$\"$\r$\n$\"DLLPathEx$\"=$\"$5$\"$\r$\n$\r$\n"
    FileWriteUTF16LE $9 "[${NAIL_MAPI_REG_NATIVE}\shell\open\command]$\r$\n@=$\"\$\"$7\$\"$\"$\r$\n$\r$\n"
    ${If} ${FileExists} "$INSTDIR\mapi\x86\NailMapi.dll"
      FileWriteUTF16LE $9 "[${NAIL_MAPI_REG_WOW}]$\r$\n@=$\"${PRODUCTNAME}$\"$\r$\n"
      FileWriteUTF16LE $9 "$\"DLLPath$\"=$\"$6$\"$\r$\n$\"DLLPathEx$\"=$\"$6$\"$\r$\n$\r$\n"
      FileWriteUTF16LE $9 "[${NAIL_MAPI_REG_WOW}\shell\open\command]$\r$\n@=$\"\$\"$7\$\"$\"$\r$\n$\r$\n"
    ${EndIf}
    FileClose $9
    DetailPrint "$(nailMapiElevating)"
    ExecShellWait "runas" "$SYSDIR\reg.exe" 'import "$PLUGINSDIR\nail-mapi.reg" /reg:64' SW_HIDE
    Delete "$PLUGINSDIR\nail-mapi.reg"
    Call NailMapiRegisteredHere
    WriteRegDWORD HKCU "${NAIL_INSTALL_KEY}" "Mapi" $0
    ${If} $0 <> 1
      DetailPrint "$(nailMapiFailed)"
    ${EndIf}
  FunctionEnd
!macroend

; Removes the machine-wide MAPI registration if it points into $INSTDIR
; (another installation, e.g. the MSI, may own it). Used by the installer
; (option turned off) and the uninstaller, so it is a macro, not a Function.
!macro NAIL_UNREGISTER_MAPI
  SetRegView 64
  ReadRegStr $1 HKLM "${NAIL_MAPI_CLIENT_KEY}" "DLLPath"
  SetRegView lastused
  ${If} $1 == "$INSTDIR\mapi\${ARCH}\NailMapi.dll"
    InitPluginsDir
    FileOpen $9 "$PLUGINSDIR\nail-mapi-remove.reg" w
    FileWriteWord $9 0xFEFF
    FileWriteUTF16LE $9 "Windows Registry Editor Version 5.00$\r$\n$\r$\n"
    FileWriteUTF16LE $9 "[-${NAIL_MAPI_REG_NATIVE}]$\r$\n$\r$\n[-${NAIL_MAPI_REG_WOW}]$\r$\n"
    FileClose $9
    DetailPrint "$(nailMapiElevating)"
    ExecShellWait "runas" "$SYSDIR\reg.exe" 'import "$PLUGINSDIR\nail-mapi-remove.reg" /reg:64' SW_HIDE
    Delete "$PLUGINSDIR\nail-mapi-remove.reg"
    SetRegView 64
    ReadRegStr $1 HKLM "${NAIL_MAPI_CLIENT_KEY}" "DLLPath"
    SetRegView lastused
    ${If} $1 == "$INSTDIR\mapi\${ARCH}\NailMapi.dll"
      DetailPrint "$(nailMapiRemoveFailed)"
    ${EndIf}
  ${EndIf}
!macroend

; ---------------------------------------------------------------------------
; Options: defaults from the previous installation, then the command line.

!macro NAIL_OPTION_FROM_CMDLINE SWITCH VAR
  ClearErrors
  ${GetOptions} $CMDLINE "${SWITCH}" $0
  ${IfNot} ${Errors}
    ${If} $0 == 1
      StrCpy ${VAR} 1
    ${Else}
      StrCpy ${VAR} 0
    ${EndIf}
  ${EndIf}
!macroend

!macro NAIL_OPTIONS_FUNCTIONS
  Function NailReadOptions
    ${If} $NailOptionsRead == 1
      Return
    ${EndIf}
    StrCpy $NailOptionsRead 1

    StrCpy $NailAutostart 0
    ReadRegStr $0 HKCU "${NAIL_RUN_KEY}" "${PRODUCTNAME}"
    ${If} $0 != ""
      StrCpy $NailAutostart 1
    ${EndIf}
    StrCpy $NailAddToPath 0
    ReadRegDWORD $0 HKCU "${UNINSTKEY}" "NailAddToPath"
    ${If} $0 = 1
      StrCpy $NailAddToPath 1
    ${EndIf}
    StrCpy $NailMapi 0
    ReadRegDWORD $0 HKCU "${NAIL_INSTALL_KEY}" "Mapi"
    ${If} $0 = 1
      StrCpy $NailMapi 1
    ${EndIf}

    !insertmacro NAIL_OPTION_FROM_CMDLINE "/AUTOSTART=" $NailAutostart
    !insertmacro NAIL_OPTION_FROM_CMDLINE "/ADDTOPATH=" $NailAddToPath
    !insertmacro NAIL_OPTION_FROM_CMDLINE "/MAPI=" $NailMapi
  FunctionEnd
!macroend

!macro NAIL_PAGE_OPTIONS
  !insertmacro NAIL_OPTIONS_FUNCTIONS
  !insertmacro NAIL_MAPI_FUNCTIONS

  Page custom NailOptionsShow NailOptionsLeave

  Function NailOptionsShow
    Call SkipIfPassive
    Call NailReadOptions
    !insertmacro MUI_HEADER_TEXT "$(nailOptionsTitle)" "$(nailOptionsSubtitle)"
    nsDialogs::Create 1018
    Pop $0
    ${If} $0 == error
      Abort
    ${EndIf}
    ${NSD_CreateCheckbox} 0 0 100% 12u "$(nailOptionAutostart)"
    Pop $NailAutostartBox
    ${If} $NailAutostart == 1
      ${NSD_Check} $NailAutostartBox
    ${EndIf}
    ${NSD_CreateLabel} 12u 14u -12u 20u "$(nailOptionAutostartHint)"
    Pop $0
    ${NSD_CreateCheckbox} 0 40u 100% 12u "$(nailOptionPath)"
    Pop $NailPathBox
    ${If} $NailAddToPath == 1
      ${NSD_Check} $NailPathBox
    ${EndIf}
    ${NSD_CreateLabel} 12u 54u -12u 20u "$(nailOptionPathHint)"
    Pop $0
    ${NSD_CreateCheckbox} 0 80u 100% 12u "$(nailOptionMapi)"
    Pop $NailMapiBox
    ${If} $NailMapi == 1
      ${NSD_Check} $NailMapiBox
    ${EndIf}
    ${NSD_CreateLabel} 12u 94u -12u 28u "$(nailOptionMapiHint)"
    Pop $0
    nsDialogs::Show
  FunctionEnd

  Function NailOptionsLeave
    ${NSD_GetState} $NailAutostartBox $0
    ${If} $0 == ${BST_CHECKED}
      StrCpy $NailAutostart 1
    ${Else}
      StrCpy $NailAutostart 0
    ${EndIf}
    ${NSD_GetState} $NailPathBox $0
    ${If} $0 == ${BST_CHECKED}
      StrCpy $NailAddToPath 1
    ${Else}
      StrCpy $NailAddToPath 0
    ${EndIf}
    ${NSD_GetState} $NailMapiBox $0
    ${If} $0 == ${BST_CHECKED}
      StrCpy $NailMapi 1
    ${Else}
      StrCpy $NailMapi 0
    ${EndIf}
  FunctionEnd
!macroend

; ---------------------------------------------------------------------------
; Tauri hooks

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro NAIL_REGISTER
  !insertmacro NAIL_WRITE_INSTALL_FACTS

  ; Updates (/UPDATE, the Tauri updater) keep the user's choices as they are.
  ${If} $UpdateMode <> 1
    Call NailReadOptions

    ${If} $NailAutostart == 1
      WriteRegStr HKCU "${NAIL_RUN_KEY}" "${PRODUCTNAME}" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --autostart"
    ${Else}
      DeleteRegValue HKCU "${NAIL_RUN_KEY}" "${PRODUCTNAME}"
    ${EndIf}

    ; The CLI ships in bin\ (only bin\ goes on PATH, not the app directory).
    ${If} $NailAddToPath == 1
    ${AndIf} ${FileExists} "$INSTDIR\bin\nail.exe"
      ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --setup-path-add "$INSTDIR\bin"' $0
      ${If} $0 = 0
        WriteRegDWORD HKCU "${UNINSTKEY}" "NailAddToPath" 1
      ${Else}
        DetailPrint "$(nailPathFailed)"
      ${EndIf}
    ${Else}
      ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --setup-path-remove "$INSTDIR\bin"' $0
      DeleteRegValue HKCU "${UNINSTKEY}" "NailAddToPath"
    ${EndIf}

    ; Simple MAPI: the only step that asks for admin rights.
    ${If} $NailMapi == 1
      Call NailRegisterMapi
    ${Else}
      !insertmacro NAIL_UNREGISTER_MAPI
      WriteRegDWORD HKCU "${NAIL_INSTALL_KEY}" "Mapi" 0
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ClearErrors
  ${GetOptions} $CMDLINE "/DELETEDATA" $0
  ${IfNot} ${Errors}
    StrCpy $DeleteAppDataCheckboxState 1
  ${EndIf}

  ${If} $UpdateMode <> 1
    ; Nail.exe is still there: let it undo the PATH entry and, if asked,
    ; remove the stored credentials.
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --setup-path-remove "$INSTDIR\bin"' $0
    ${If} $DeleteAppDataCheckboxState = 1
      ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --setup-purge-credentials' $0
      ${If} $0 <> 0
        DetailPrint "$(nailCredentialsFailed)"
      ${EndIf}
    ${EndIf}
    !insertmacro NAIL_UNREGISTER_MAPI
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ; The remembered install directory and installer language (Tauri keeps
    ; them unless app data is deleted): a later install must not default to
    ; a directory that is gone.
    DeleteRegKey HKCU "${MANUPRODUCTKEY}"
    !insertmacro NAIL_UNREGISTER
    Delete "$INSTDIR\bin\nail.exe"
    RMDir "$INSTDIR\bin"
    Delete "$INSTDIR\mapi\x64\NailMapi.dll"
    Delete "$INSTDIR\mapi\x86\NailMapi.dll"
    Delete "$INSTDIR\mapi\arm64\NailMapi.dll"
    RMDir "$INSTDIR\mapi\x64"
    RMDir "$INSTDIR\mapi\x86"
    RMDir "$INSTDIR\mapi\arm64"
    RMDir "$INSTDIR\mapi"
    RMDir "$INSTDIR"

    ; Settings, mail data, index and logs stay unless the user asked.
    ${If} $DeleteAppDataCheckboxState = 1
      SetShellVarContext current
      ${If} $APPDATA != ""
        RMDir /r "$APPDATA\${PRODUCTNAME}"
      ${EndIf}
      ${If} $LOCALAPPDATA != ""
        RMDir /r "$LOCALAPPDATA\${PRODUCTNAME}"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend
