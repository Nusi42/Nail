# Nail — English texts of folder management named by ADR 0196: why creating,
# renaming, moving or deleting a folder was refused.
#
# The ids named by nail_api::folders are pre-filled;
# nail-api/tests/catalog.rs checks them. The UI's own texts (menus, dialogs)
# are in folders.ftl.

folder-issue-missing-name = A folder needs a name.
folder-issue-name-too-long = The folder name is too long.
folder-issue-invalid-name = This name cannot be used for a folder on this server (it contains the folder separator or control characters).
folder-issue-name-taken = A folder with this name exists there already.
folder-issue-system-folder = Inbox, Sent, Drafts, Trash and the other special folders cannot be renamed, moved or deleted.
folder-issue-into-itself = A folder cannot be moved into itself or one of its subfolders.
folder-issue-other-account = Folders can only be moved within their account.
folder-issue-no-hierarchy = This server has no subfolders.
folder-issue-no-trash = This folder can only be deleted for good.
folder-issue-gmail-labels = Gmail accounts have labels instead of folders.
folder-issue-unsupported = Nail cannot change folders of this account yet.
