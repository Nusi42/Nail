; Nail's installer logic (plan M2b WP44, design "Windows installer &
; distribution"), hooked into installer.nsi (Nail's copy of the Tauri
; template). Everything is per-user (HKCU); nothing needs admin rights.
;
;   * Windows registration: ProgIDs for mailto / webcal / webcals and the
;     .eml / .ics / .vcf types, "Open with" entries, and Capabilities listed
;     under RegisteredApplications, so Nail appears in Settings > Default
;     apps. Nail never takes an association over itself: the user chooses.
;   * Options page (NAIL_PAGE_OPTIONS): start with Windows (HKCU Run key,
;     `--autostart`), `nail` CLI on the per-user PATH (via
;     `Nail.exe --setup-path-add`, which keeps long PATHs intact).
;     Silent installs: /AUTOSTART=0|1 and /ADDTOPATH=0|1.
;   * Uninstall keeps settings, mail data and credentials unless the user
;     ticks the checkbox on the confirm page (silent: /DELETEDATA); then
;     %APPDATA%\Nail, %LOCALAPPDATA%\Nail and Nail's Credential Manager
;     entries (`Nail.exe --setup-purge-credentials`) are removed too.
;
; User-visible text comes from languages\English.nsh ($(nail...) strings).
; These macros are expanded inside installer.nsi, after its !defines
; (PRODUCTNAME, MAINBINARYNAME, UNINSTKEY), so they may use them.

!include nsDialogs.nsh

!define NAIL_LANGUAGE_DIR "${__FILEDIR__}\languages"

; Registry locations (all under HKCU).
!define NAIL_CAPABILITIES "Software\Nail\Capabilities"
!define NAIL_REGISTERED_APPS "Software\RegisteredApplications"
!define NAIL_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define NAIL_CLASSES "Software\Classes"
; ProgIDs.
!define NAIL_PROGID_MAILTO "Nail.Url.mailto"
!define NAIL_PROGID_WEBCAL "Nail.Url.webcal"
!define NAIL_PROGID_EML "Nail.eml"
!define NAIL_PROGID_ICS "Nail.ics"
!define NAIL_PROGID_VCF "Nail.vcf"

Var NailOptionsRead
Var NailAutostart
Var NailAddToPath
Var NailAutostartBox
Var NailPathBox

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
  DeleteRegKey /ifempty HKCU "Software\Nail"
  DeleteRegKey HKCU "${NAIL_CLASSES}\${NAIL_PROGID_MAILTO}"
  DeleteRegKey HKCU "${NAIL_CLASSES}\${NAIL_PROGID_WEBCAL}"
  !insertmacro NAIL_REMOVE_FILE_PROGID "${NAIL_PROGID_EML}" ".eml"
  !insertmacro NAIL_REMOVE_FILE_PROGID "${NAIL_PROGID_ICS}" ".ics"
  !insertmacro NAIL_REMOVE_FILE_PROGID "${NAIL_PROGID_VCF}" ".vcf"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

; ---------------------------------------------------------------------------
; Options: defaults from the previous installation, then the command line.

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

    ClearErrors
    ${GetOptions} $CMDLINE "/AUTOSTART=" $0
    ${IfNot} ${Errors}
      ${If} $0 == 1
        StrCpy $NailAutostart 1
      ${Else}
        StrCpy $NailAutostart 0
      ${EndIf}
    ${EndIf}
    ClearErrors
    ${GetOptions} $CMDLINE "/ADDTOPATH=" $0
    ${IfNot} ${Errors}
      ${If} $0 == 1
        StrCpy $NailAddToPath 1
      ${Else}
        StrCpy $NailAddToPath 0
      ${EndIf}
    ${EndIf}
  FunctionEnd
!macroend

!macro NAIL_PAGE_OPTIONS
  !insertmacro NAIL_OPTIONS_FUNCTIONS

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
  FunctionEnd
!macroend

; ---------------------------------------------------------------------------
; Tauri hooks

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro NAIL_REGISTER

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
