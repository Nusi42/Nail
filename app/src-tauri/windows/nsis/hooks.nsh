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
;     rights. A machine-wide registration must never name a file the user
;     (or any of the user's unelevated programs) can replace: elevated
;     programs and other users load it. So one elevated cmd.exe (one UAC
;     prompt) copies the DLLs from $INSTDIR to "Program Files\Nail MAPI"
;     (writable by administrators only) and points HKLM there with reg.exe.
;     Everything it runs is on its command line, fixed at the UAC prompt; no
;     file in the user's %TEMP% is read elevated. HKLM gets no
;     shell\open\command (it would name the per-user Nail.exe); the DLL
;     starts each user's own Nail from HKCU\Software\Nail\Install. Declining
;     leaves Nail installed without it. Updates (/UPDATE) keep the
;     registration as it is (the pipe protocol is versioned).
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
; machine hive; reg.exe spells the hive out (/reg:64 native, /reg:32 the
; 32-bit view).
!define NAIL_MAPI_CLIENT_KEY "Software\Clients\Mail\Nail"
!define NAIL_MAPI_REG_KEY "HKLM\SOFTWARE\Clients\Mail\Nail"
; The protected copy of the DLLs the registration names (administrators
; only): <dir>\mapi\<arch>\NailMapi.dll, as nail_api::mapi_dll_relative_path.
!define NAIL_MAPI_MACHINE_DIR "$PROGRAMFILES64\Nail MAPI"
; Longest installation folder the elevated MAPI command line has room for.
!define NAIL_MAPI_MAX_DIR 110
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
; Simple MAPI (installer side). One elevated cmd.exe per change (one UAC
; prompt): its whole work is on its command line, so nothing the user's
; programs can modify is read with administrator rights.

!macro NAIL_MAPI_FUNCTIONS
  ; $0 in: a string; $0 out: 1 when it contains "%" (cmd.exe would expand
  ; it on the elevated command line), else 0.
  Function NailHasPercent
    Push $1
    Push $2
    StrCpy $2 0
    ${Do}
      StrCpy $1 $0 1 $2
      ${If} $1 == ""
        StrCpy $0 0
        ${Break}
      ${EndIf}
      ${If} $1 == "%"
        StrCpy $0 1
        ${Break}
      ${EndIf}
      IntOp $2 $2 + 1
    ${Loop}
    Pop $2
    Pop $1
  FunctionEnd

  ; $0 = 1 when HKLM names the protected machine copy and it exists, 2 when
  ; it names this installation's own DLL (left by an earlier build: replaced
  ; by the protected copy), else 0.
  Function NailMapiRegisteredHere
    SetRegView 64
    ReadRegStr $1 HKLM "${NAIL_MAPI_CLIENT_KEY}" "DLLPath"
    SetRegView lastused
    ${If} $1 == "${NAIL_MAPI_MACHINE_DIR}\mapi\${ARCH}\NailMapi.dll"
    ${AndIf} ${FileExists} "${NAIL_MAPI_MACHINE_DIR}\mapi\${ARCH}\NailMapi.dll"
      StrCpy $0 1
    ${ElseIf} $1 == "$INSTDIR\mapi\${ARCH}\NailMapi.dll"
      StrCpy $0 2
    ${Else}
      StrCpy $0 0
    ${EndIf}
  FunctionEnd

  ; Copies the DLLs to the protected folder and registers them there, in
  ; one elevated cmd.exe. Sets Mapi in the facts to what the registry says
  ; afterwards.
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
    ; The command line must fit NSIS_MAX_STRLEN (1024): short commands,
    ; and installation folders up to NAIL_MAPI_MAX_DIR characters.
    StrCpy $0 "$INSTDIR"
    Call NailHasPercent
    StrLen $1 "$INSTDIR"
    ${If} $0 = 1
    ${OrIf} $1 > ${NAIL_MAPI_MAX_DIR}
      DetailPrint "$(nailMapiPathUnsupported)"
      WriteRegDWORD HKCU "${NAIL_INSTALL_KEY}" "Mapi" 0
      Return
    ${EndIf}
    ; Every path is quoted (cmd.exe takes & | < > ^ ( ) literally inside
    ; quotes; Windows paths cannot contain quotes; "%" was refused above).
    ; `reg` runs from the system folder (the current directory, searched
    ; first). %v is the loop variable of `for` on a command line.
    StrCpy $5 "${NAIL_MAPI_MACHINE_DIR}\mapi\${ARCH}"
    StrCpy $6 "${NAIL_MAPI_MACHINE_DIR}\mapi\x86"
    StrCpy $7 'reg add "${NAIL_MAPI_REG_KEY}"'
    StrCpy $8 'cd /d "$SYSDIR" & md "$5" 2>nul & md "$6" 2>nul & copy /b /y "$INSTDIR\mapi\${ARCH}\NailMapi.dll" "$5\NailMapi.dll"'
    StrCpy $8 '$8 && $7 /ve /d "${PRODUCTNAME}" /f /reg:64 && (for %v in (DLLPath DLLPathEx) do @$7 /v %v /d "$5\NailMapi.dll" /f /reg:64)'
    ${If} ${FileExists} "$INSTDIR\mapi\x86\NailMapi.dll"
      StrCpy $8 '$8 && copy /b /y "$INSTDIR\mapi\x86\NailMapi.dll" "$6\NailMapi.dll"'
      StrCpy $8 '$8 && $7 /ve /d "${PRODUCTNAME}" /f /reg:32 && (for %v in (DLLPath DLLPathEx) do @$7 /v %v /d "$6\NailMapi.dll" /f /reg:32)'
    ${EndIf}
    DetailPrint "$(nailMapiElevating)"
    ExecShellWait "runas" "$SYSDIR\cmd.exe" '/d /v:off /s /c "$8"' SW_HIDE
    Call NailMapiRegisteredHere
    ${If} $0 = 1
      WriteRegDWORD HKCU "${NAIL_INSTALL_KEY}" "Mapi" 1
    ${Else}
      WriteRegDWORD HKCU "${NAIL_INSTALL_KEY}" "Mapi" 0
      DetailPrint "$(nailMapiFailed)"
    ${EndIf}
  FunctionEnd
!macroend

; Removes the machine-wide MAPI registration if it names the protected copy
; or this installation's own DLL (another installation, e.g. the MSI, may
; own it), and the protected copy. Fixed command line, one UAC prompt. Used
; by the installer (option turned off) and the uninstaller, so it is a
; macro, not a Function.
!macro NAIL_UNREGISTER_MAPI
  SetRegView 64
  ReadRegStr $1 HKLM "${NAIL_MAPI_CLIENT_KEY}" "DLLPath"
  SetRegView lastused
  ${If} $1 == "${NAIL_MAPI_MACHINE_DIR}\mapi\${ARCH}\NailMapi.dll"
  ${OrIf} $1 == "$INSTDIR\mapi\${ARCH}\NailMapi.dll"
    DetailPrint "$(nailMapiElevating)"
    ExecShellWait "runas" "$SYSDIR\cmd.exe" '/d /v:off /s /c ""$SYSDIR\reg.exe" delete "${NAIL_MAPI_REG_KEY}" /f /reg:64 & "$SYSDIR\reg.exe" delete "${NAIL_MAPI_REG_KEY}" /f /reg:32 & rd /s /q "${NAIL_MAPI_MACHINE_DIR}""' SW_HIDE
    SetRegView 64
    ReadRegStr $2 HKLM "${NAIL_MAPI_CLIENT_KEY}" "DLLPath"
    SetRegView lastused
    ${If} $2 == $1
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
    ${NSD_CreateLabel} 12u 94u -12u 40u "$(nailOptionMapiHint)"
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
