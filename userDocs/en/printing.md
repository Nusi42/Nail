# Printing and saving as PDF

Nail prints a message, a whole conversation, an event or an agenda of your
calendars, and saves any of them as a PDF file.

## Printing

Use **Print** in the reader's toolbar or menu, or the command palette
(**Print the message…**, **Print the conversation…**, **Print the
event…**, **Print an agenda…**). The print dialog shows what will be
printed and lets you choose:

| Option | What it does |
| --- | --- |
| Paper size | A4, A5, Letter or Legal |
| Orientation | Portrait or landscape |
| Headers | Basic (from, to, cc, date, subject) or all (also reply-to and the Message-ID) |
| Include quoted text | Off replaces quoted parts of replies (lines starting with `>`) with a short note |
| List attachments | Names and sizes of the attachments at the end |
| Load remote images | Only where remote content is allowed for the sender anyway |
| Print background colours | Off saves ink |

**Print** opens a print preview with Nail's printing style and then the
Windows print dialog. When the print dialog closes, you are back where you
were. **Print** and **Close** at the top of the preview are not printed.

The printout always uses light colours, also when Nail is dark. A theme can
style printouts with its `print.*` parts and `print=` colours (see the
[theme reference](theme-reference.md)).

Messages are printed as text, in the order of the conversation for a
conversation, each message on a new page.

## Agendas

**Print an agenda…** prints your events between two days (at most one
year):

- **List** — only the days with events, one after another;
- **Day** — every day on its own page, also free days;
- **Week** — a page per week;
- **Month** — a month grid per page.

Tick the calendars to print; with none ticked, the calendars shown in your
calendar are printed.

## Saving as PDF

In the desktop app, **Save as PDF…** asks where to save the file (type the
path or choose **Browse…**) and writes the PDF directly, with the paper size
and options you chose. Nail says where the file went and how big it is.

In a browser (Nail's web mode), the browser cannot write files for Nail:
use **Print** and choose **Save as PDF** (or **Microsoft Print to PDF**) as
the printer in the print dialog.
