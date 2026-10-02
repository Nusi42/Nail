# Confirmations: what AI agents and plug-ins may change

AI agents connected to Nail (MCP clients) and plug-ins can do more than read: they can organise mail, write drafts, send messages, change events, contacts and tasks, and create filter rules — but only with the permissions you gave them, and only after you approved anything risky.

## The confirmation center

When an agent or a plug-in wants to do something that needs your approval, Nail shows a notice ("Waiting for your confirmation") and the confirmation indicator shows how many requests wait. Choose **Review** (or the indicator) to open the confirmation center.

Each request shows:

- **Who asks** — always "AI agent “…” (MCP client)" or "Plug-in “…”". When a plug-in acts for an agent, both are named.
- **What kind of change** it is. Deleting is marked **Deletes**, sending mail **Sends mail**.
- A sentence describing the change, the **affected items** (messages, recipients, events, …) and a **What changes** table with the values now and after.
- The accounts concerned, when it was asked and when the request expires.

Choose **Approve** or **Deny**. Nothing happens until you approve; a request that is not answered in time expires and changes nothing. The **History** lists earlier decisions and where they were made (in Nail, in the agent's own prompt, or by Nail when a request timed out).

### Always allow

For changes that do not delete anything you can tick **From now on, let … do this without asking** before approving. Deleting — moving mail to Trash, deleting mail permanently, deleting events, contacts or tasks — always asks, every time, and cannot be allowed without asking.

## Confirmation policies

Each MCP client (Settings → MCP clients) and each plug-in that may change data (Plug-ins → **Confirmations**) has a list of kinds of changes, each set to **Ask first** or **Without asking**. The defaults:

| Asks first | Runs without asking |
| --- | --- |
| Deleting (always), sending mail, invitations and answers, filter rules | Flags, tags, moving and archiving, drafts, calendar, contact and task changes |

Every change still goes through Nail like your own, appears in the activity log and can be undone.

## MCP clients: what they may change and how often

Under Settings → MCP clients, **What it may change** grants the writing permissions (organise mail, delete permanently, write drafts, send mail, change calendars, change contacts and tasks, filter rules). **Limits** caps how many calls, changes and sent messages a client may make; calls over a limit are refused and shown as denied in the activity.

## Plug-ins in the reader and composer

- Plug-ins that check messages you open can show a **note above the message** or a **badge** in its header. Each one names the plug-in; plug-ins never change the message itself.
- Plug-ins can **preview attachments** (for example a CSV file as a table); the preview names the plug-in that drew it.
- Attachments and links can have **plug-in actions** in their menus.
- In the composer, **Plug-in actions** insert or change text, and **Insert block** adds a block a plug-in turns into formatted content when the message is sent. When a plug-in or an agent edits your draft, Nail says who did it.

## When a plug-in stops a message

A plug-in that checks outgoing mail can stop a message, for example when it mentions an attachment but nothing is attached. Nail then shows which plug-in stopped it and why. Choose **Keep editing** to fix the message, or **Send anyway** to send it as it is. If you wait too long, send the message again so it is checked once more.

## Plug-in activity

In the plug-in manager, **Activity** shows a plug-in's mail hooks (how often they ran, failed or were too slow), its background jobs (schedule, last and next run, **Run now**) and how much of its hourly CPU time it used. A plug-in that uses all its CPU time has its jobs skipped until the hour is over; a plug-in that is too slow is skipped and can never hold up your mail.
