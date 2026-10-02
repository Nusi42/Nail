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
LangString nailOptionMapi ${LANG_ENGLISH} "Use Nail for $\"Send to > Mail recipient$\" (needs administrator rights)"
LangString nailOptionMapiHint ${LANG_ENGLISH} "Registers Nail as a Simple MAPI mail program. Once Nail is your mail program, other programs' $\"email this$\" commands open a Nail message with the file attached. Windows asks for administrator rights for this step only: the component is copied to Program Files, where only administrators can change it."
LangString nailMapiElevating ${LANG_ENGLISH} "Asking Windows for administrator rights to change the Simple MAPI registration..."
LangString nailMapiFailed ${LANG_ENGLISH} "Nail was not registered for $\"Send to > Mail recipient$\" (administrator rights were not granted). You can run the setup program again to try once more."
LangString nailMapiRemoveFailed ${LANG_ENGLISH} "Could not remove Nail's $\"Send to > Mail recipient$\" registration (administrator rights were not granted)."
LangString nailMapiMissing ${LANG_ENGLISH} "This build has no Simple MAPI component; $\"Send to > Mail recipient$\" was not set up."
LangString nailMapiPathUnsupported ${LANG_ENGLISH} "$\"Send to > Mail recipient$\" was not set up: the installation folder contains a percent sign (%) or its path is very long. Choose another folder to use it."
