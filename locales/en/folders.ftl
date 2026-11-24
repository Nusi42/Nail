# Nail — English UI strings: folder management (ADR 0196).
#
# Message ids start with the prefixes listed in
# ui/src/features/folders/messages.ts. The core's reasons for refusing a
# change are in core-folders.ftl.

folders-action-new = New folder…
folders-action-new-subfolder = New subfolder…
folders-action-rename = Rename folder…
folders-action-move = Move folder to…
folders-action-delete = Delete folder
folders-action-keywords = folder create new add rename move delete remove subfolder mailbox
window-folder-name = Folder
window-folder-move = Move folder
window-folder-delete = { folders-action-delete }
folders-create-intro = New folder in “{ $parent }”
folders-create-top-intro = New folder in { $account }
folders-rename-intro = Rename “{ $name }”
folders-move-intro = Move “{ $name }” with its subfolders to:
folders-move-top = The top of the account
folders-delete-question = Delete the folder “{ $name }” for good?
folders-delete-intro = { $subfolders ->
    [0] { $messages ->
        [one] It holds 1 message.
       *[other] It holds { $messages } messages.
    }
    [one] With its subfolder it holds { $messages } messages.
   *[other] With its { $subfolders } subfolders it holds { $messages } messages.
} It is deleted on the server too. This cannot be undone.
folders-delete-empty-intro = The folder is empty. It is deleted on the server too. This cannot be undone.
folders-delete-confirm = { folders-action-delete }
folders-waiting = The server cannot be reached right now. Nail makes the change as soon as it can.
folders-gone = This folder is no longer there.
folders-searched-here = The search runs in this folder
