; Nail's installer strings (English). Used by hooks.nsh and installer.nsi
; as $(name). Every string used there must be defined here
; (app/src-tauri/tests/installer.rs checks it). Tauri's own installer
; strings come from the Tauri CLI.

LangString nailOptionsTitle ${LANG_ENGLISH} "Options"
LangString nailOptionsSubtitle ${LANG_ENGLISH} "Choose how Nail fits into Windows."
LangString nailOptionAutostart ${LANG_ENGLISH} "Start Nail when I sign in to Windows"
LangString nailOptionAutostartHint ${LANG_ENGLISH} "Nail starts minimized and keeps syncing mail, running rules and serving MCP clients."
LangString nailOptionPath ${LANG_ENGLISH} "Add the nail command-line tool to PATH"
LangString nailOptionPathHint ${LANG_ENGLISH} "Lets you run commands such as nail mcp and nail serve from any terminal."
LangString nailDeleteData ${LANG_ENGLISH} "Also delete my mail data, settings and stored passwords"
LangString nailAppDescription ${LANG_ENGLISH} "Mail, calendar and contacts"
LangString nailMailtoName ${LANG_ENGLISH} "Email link (Nail)"
LangString nailWebcalName ${LANG_ENGLISH} "Calendar subscription (Nail)"
LangString nailEmlName ${LANG_ENGLISH} "Email message"
LangString nailIcsName ${LANG_ENGLISH} "Calendar file"
LangString nailVcfName ${LANG_ENGLISH} "Contact card"
LangString nailPathFailed ${LANG_ENGLISH} "Could not add the nail command-line tool to PATH."
LangString nailCredentialsFailed ${LANG_ENGLISH} "Could not remove all stored passwords from the Windows Credential Manager."
