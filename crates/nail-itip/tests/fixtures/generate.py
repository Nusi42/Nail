"""Writes the synthesized iMIP interop fixtures (see README.md).

Not needed at test time: the .eml files are checked in. Run it with
`python generate.py` in this folder after changing a fixture here; it
writes CRLF line endings and the base64 / quoted-printable bodies the
producers use.
"""

import base64
import quopri
from pathlib import Path

HERE = Path(__file__).parent


def crlf(text: str) -> str:
    return text.replace("\r\n", "\n").replace("\n", "\r\n")


def b64(text: str) -> str:
    data = base64.b64encode(crlf(text).encode("utf-8")).decode("ascii")
    return "\n".join(data[i : i + 76] for i in range(0, len(data), 76))


def qp(text: str) -> str:
    return quopri.encodestring(crlf(text).encode("utf-8")).decode("ascii")


def write(name: str, text: str) -> None:
    (HERE / name).write_bytes(crlf(text).encode("utf-8"))


# --------------------------------------------------------------------------
# Google Calendar: a weekly meeting with a Meet link (REQUEST).
# multipart/mixed { multipart/alternative { text/plain, text/html,
# text/calendar; method=REQUEST }, application/ics invite.ics (base64) }.

GOOGLE_REQUEST_ICS = """BEGIN:VCALENDAR
PRODID:-//Google Inc//Google Calendar 70.9054//EN
VERSION:2.0
CALSCALE:GREGORIAN
METHOD:REQUEST
BEGIN:VTIMEZONE
TZID:Europe/Berlin
X-LIC-LOCATION:Europe/Berlin
BEGIN:DAYLIGHT
TZOFFSETFROM:+0100
TZOFFSETTO:+0200
TZNAME:GMT+2
DTSTART:19700329T020000
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU
END:DAYLIGHT
BEGIN:STANDARD
TZOFFSETFROM:+0200
TZOFFSETTO:+0100
TZNAME:GMT+1
DTSTART:19701025T030000
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU
END:STANDARD
END:VTIMEZONE
BEGIN:VEVENT
DTSTART;TZID=Europe/Berlin:20261005T100000
DTEND;TZID=Europe/Berlin:20261005T103000
RRULE:FREQ=WEEKLY;BYDAY=MO
DTSTAMP:20261001T090512Z
ORGANIZER;CN=Ada Lovelace:mailto:ada@example.org
UID:5k1d9m3q7v0hb2c8e4f6g1j3l5@google.com
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=Ada Lov
 elace;X-NUM-GUESTS=0:mailto:ada@example.org
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=
 TRUE;CN=bob@example.org;X-NUM-GUESTS=0:mailto:bob@example.org
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=OPT-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=
 TRUE;CN=Charles Babbage;X-NUM-GUESTS=0:mailto:charles@example.net
X-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij
X-MICROSOFT-CDO-OWNERAPPTID:-1592648364
CREATED:20261001T090511Z
DESCRIPTION:Weekly sync on the engine.\\n\\n-::~:~::~:~:~:~:~:~:~:~:~:~:~:~:~:~
 :~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~::~:~::-\\nJoin with Google Meet: https://mee
 t.google.com/abc-defg-hij\\n\\nLearn more about Meet at: https://support.googl
 e.com/a/users/answer/9282720\\n\\n-::~:~::~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~
 :~:~:~:~:~:~:~:~:~:~:~::~:~::-
LAST-MODIFIED:20261001T090511Z
LOCATION:
SEQUENCE:0
STATUS:CONFIRMED
SUMMARY:Engine sync
TRANSP:OPAQUE
BEGIN:VALARM
ACTION:DISPLAY
DESCRIPTION:This is an event reminder
TRIGGER:-P0DT0H10M0S
END:VALARM
END:VEVENT
END:VCALENDAR
"""

write(
    "google-request.eml",
    f"""Return-Path: <ada@example.org>
Received: from mail.example.org (localhost [127.0.0.1])
	by mx.example.org with ESMTP id 1 for <bob@example.org>;
	Thu, 01 Oct 2026 09:05:13 +0000
MIME-Version: 1.0
Reply-To: ada@example.org
Sender: Google Calendar <calendar-notification@google.com>
Message-ID: <calendar-0f1e2d3c-4b5a-6978-8a9b-0c1d2e3f4a5b@google.com>
Date: Thu, 01 Oct 2026 09:05:12 +0000
Subject: Invitation: Engine sync @ Weekly from 10:00 to 10:30 on Monday (CEST) (bob@example.org)
From: Ada Lovelace <ada@example.org>
To: bob@example.org
Content-Type: multipart/mixed; boundary="000000000000a1b2c3d4e5f60718"

--000000000000a1b2c3d4e5f60718
Content-Type: multipart/alternative; boundary="000000000000a1b2c3d4e5f60717"

--000000000000a1b2c3d4e5f60717
Content-Type: text/plain; charset="UTF-8"; format=flowed; delsp=yes
Content-Transfer-Encoding: base64

{b64("You have been invited to the following event.\n\nEngine sync\nWeekly from 10:00 to 10:30 on Monday\nJoining info: meet.google.com/abc-defg-hij\n\nInvitation from Google Calendar\n")}
--000000000000a1b2c3d4e5f60717
Content-Type: text/html; charset="UTF-8"
Content-Transfer-Encoding: quoted-printable

{qp('<span itemscope itemtype="http://schema.org/InformAction"><div>You have been invited to <b>Engine sync</b>.</div></span>\n')}
--000000000000a1b2c3d4e5f60717
Content-Type: text/calendar; charset="UTF-8"; method=REQUEST
Content-Transfer-Encoding: 7bit

{GOOGLE_REQUEST_ICS}
--000000000000a1b2c3d4e5f60717--

--000000000000a1b2c3d4e5f60718
Content-Type: application/ics; name="invite.ics"
Content-Disposition: attachment; filename="invite.ics"
Content-Transfer-Encoding: base64

{b64(GOOGLE_REQUEST_ICS)}
--000000000000a1b2c3d4e5f60718--
""",
)

# Google: the organizer cancels one occurrence (CANCEL with RECURRENCE-ID).
GOOGLE_CANCEL_ICS = """BEGIN:VCALENDAR
PRODID:-//Google Inc//Google Calendar 70.9054//EN
VERSION:2.0
CALSCALE:GREGORIAN
METHOD:CANCEL
BEGIN:VTIMEZONE
TZID:Europe/Berlin
X-LIC-LOCATION:Europe/Berlin
BEGIN:DAYLIGHT
TZOFFSETFROM:+0100
TZOFFSETTO:+0200
TZNAME:GMT+2
DTSTART:19700329T020000
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU
END:DAYLIGHT
BEGIN:STANDARD
TZOFFSETFROM:+0200
TZOFFSETTO:+0100
TZNAME:GMT+1
DTSTART:19701025T030000
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU
END:STANDARD
END:VTIMEZONE
BEGIN:VEVENT
DTSTART;TZID=Europe/Berlin:20261012T100000
DTEND;TZID=Europe/Berlin:20261012T103000
DTSTAMP:20261008T141500Z
ORGANIZER;CN=Ada Lovelace:mailto:ada@example.org
UID:5k1d9m3q7v0hb2c8e4f6g1j3l5@google.com
RECURRENCE-ID;TZID=Europe/Berlin:20261012T100000
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;CN=bob
 @example.org;X-NUM-GUESTS=0:mailto:bob@example.org
CREATED:20261001T090511Z
DESCRIPTION:The holiday Monday.
LAST-MODIFIED:20261008T141459Z
LOCATION:
SEQUENCE:1
STATUS:CANCELLED
SUMMARY:Engine sync
TRANSP:OPAQUE
END:VEVENT
END:VCALENDAR
"""

write(
    "google-cancel-occurrence.eml",
    f"""MIME-Version: 1.0
Sender: Google Calendar <calendar-notification@google.com>
Message-ID: <calendar-7a6b5c4d-3e2f-1a0b-9c8d-7e6f5a4b3c2d@google.com>
Date: Thu, 08 Oct 2026 14:15:00 +0000
Subject: Cancelled event: Engine sync @ Mon 12 Oct 2026 10:00 - 10:30 (CEST) (bob@example.org)
From: Ada Lovelace <ada@example.org>
To: bob@example.org
Content-Type: multipart/mixed; boundary="000000000000b1b2c3d4e5f60718"

--000000000000b1b2c3d4e5f60718
Content-Type: multipart/alternative; boundary="000000000000b1b2c3d4e5f60717"

--000000000000b1b2c3d4e5f60717
Content-Type: text/plain; charset="UTF-8"; format=flowed; delsp=yes

This event has been cancelled and removed from your calendar.

--000000000000b1b2c3d4e5f60717
Content-Type: text/calendar; charset="UTF-8"; method=CANCEL
Content-Transfer-Encoding: 7bit

{GOOGLE_CANCEL_ICS}
--000000000000b1b2c3d4e5f60717--

--000000000000b1b2c3d4e5f60718
Content-Type: application/ics; name="invite.ics"
Content-Disposition: attachment; filename="invite.ics"
Content-Transfer-Encoding: base64

{b64(GOOGLE_CANCEL_ICS)}
--000000000000b1b2c3d4e5f60718--
""",
)

# --------------------------------------------------------------------------
# Outlook / Exchange: a single meeting with a Windows zone; the calendar
# part base64 encoded inside multipart/alternative, no attachment.

OUTLOOK_REQUEST_ICS = """BEGIN:VCALENDAR
METHOD:REQUEST
PRODID:Microsoft Exchange Server 2010
VERSION:2.0
BEGIN:VTIMEZONE
TZID:W. Europe Standard Time
BEGIN:STANDARD
DTSTART:16010101T030000
TZOFFSETFROM:+0200
TZOFFSETTO:+0100
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10
END:STANDARD
BEGIN:DAYLIGHT
DTSTART:16010101T020000
TZOFFSETFROM:+0100
TZOFFSETTO:+0200
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3
END:DAYLIGHT
END:VTIMEZONE
BEGIN:VEVENT
ORGANIZER;CN=Grace Hopper:mailto:grace@contoso.example
ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=Bob Builde
 r:mailto:Bob@Example.org
ATTENDEE;ROLE=OPT-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=Alan Turin
 g:mailto:alan@contoso.example
ATTENDEE;CUTYPE=RESOURCE;ROLE=NON-PARTICIPANT;PARTSTAT=ACCEPTED;CN=Room Ada
 :mailto:room-ada@contoso.example
DESCRIPTION;LANGUAGE=en-US:Quarterly review of the compiler roadmap.\\n\\n____
 ____________________________________________________________________________
 \\nMicrosoft Teams meeting\\nJoin: https://teams.microsoft.com/l/meetup-join/1
 9%3ameeting_abc%40thread.v2/0\\n
UID:040000008200E00074C5B7101A82E00800000000A0B1C2D3E4F5A601000000000000000010
 000000F1E2D3C4B5A69788A9B0C1D2E3F4A5B6
SUMMARY;LANGUAGE=en-US:Compiler roadmap Q4
DTSTART;TZID=W. Europe Standard Time:20261006T140000
DTEND;TZID=W. Europe Standard Time:20261006T150000
CLASS:PUBLIC
PRIORITY:5
DTSTAMP:20261001T101500Z
TRANSP:OPAQUE
STATUS:CONFIRMED
SEQUENCE:0
LOCATION;LANGUAGE=en-US:Room Ada
X-MICROSOFT-CDO-APPT-SEQUENCE:0
X-MICROSOFT-CDO-OWNERAPPTID:2124321212
X-MICROSOFT-CDO-BUSYSTATUS:TENTATIVE
X-MICROSOFT-CDO-INTENDEDSTATUS:BUSY
X-MICROSOFT-CDO-ALLDAYEVENT:FALSE
X-MICROSOFT-CDO-IMPORTANCE:1
X-MICROSOFT-CDO-INSTTYPE:0
X-MICROSOFT-SKYPETEAMSMEETINGURL:https://teams.microsoft.com/l/meetup-join/19
 %3ameeting_abc%40thread.v2/0
X-MICROSOFT-DONOTFORWARDMEETING:FALSE
X-MICROSOFT-DISALLOW-COUNTER:FALSE
X-MICROSOFT-LOCATIONS:[{"DisplayName":"Room Ada","LocationAnnotation":"","Lo
 cationUri":"room-ada@contoso.example","LocationSource":"None"}]
BEGIN:VALARM
DESCRIPTION:REMINDER
TRIGGER;RELATED=START:-PT15M
ACTION:DISPLAY
END:VALARM
END:VEVENT
END:VCALENDAR
"""

write(
    "outlook-request.eml",
    f"""Received: from AM0PR01MB0001.eurprd01.prod.outlook.example (localhost
 [127.0.0.1]) by AM0PR01MB0002.eurprd01.prod.outlook.example; Thu, 1 Oct 2026
 10:15:00 +0000
From: Grace Hopper <grace@contoso.example>
To: Bob Builder <Bob@Example.org>, Alan Turing <alan@contoso.example>
Subject: Compiler roadmap Q4
Thread-Topic: Compiler roadmap Q4
Thread-Index: AdcBAgMEBQYHCAkKCwwNDg8QERITFA==
Date: Thu, 1 Oct 2026 10:15:00 +0000
Message-ID: <AM0PR01MB0001F1E2D3C4B5A6978@AM0PR01MB0001.eurprd01.prod.outlook.example>
Accept-Language: en-US
Content-Language: en-US
X-MS-Has-Attach:
X-MS-TNEF-Correlator:
Content-Type: multipart/alternative;
	boundary="_000_AM0PR01MB0001F1E2D3C4B5A6978AM0PR01MB0001eurp_"
MIME-Version: 1.0

--_000_AM0PR01MB0001F1E2D3C4B5A6978AM0PR01MB0001eurp_
Content-Type: text/plain; charset="iso-8859-1"
Content-Transfer-Encoding: quoted-printable

{qp("Quarterly review of the compiler roadmap.")}
--_000_AM0PR01MB0001F1E2D3C4B5A6978AM0PR01MB0001eurp_
Content-Type: text/html; charset="iso-8859-1"
Content-Transfer-Encoding: quoted-printable

{qp("<html><body><p>Quarterly review of the compiler roadmap.</p></body></html>")}
--_000_AM0PR01MB0001F1E2D3C4B5A6978AM0PR01MB0001eurp_
Content-Type: text/calendar; charset="utf-8"; method=REQUEST
Content-Transfer-Encoding: base64

{b64(OUTLOOK_REQUEST_ICS)}
--_000_AM0PR01MB0001F1E2D3C4B5A6978AM0PR01MB0001eurp_--
""",
)

# Outlook: "Propose new time" (COUNTER) from Alan to Grace, listing every
# attendee like Outlook does; the proposer is the sender.
OUTLOOK_COUNTER_ICS = """BEGIN:VCALENDAR
METHOD:COUNTER
PRODID:Microsoft Exchange Server 2010
VERSION:2.0
BEGIN:VTIMEZONE
TZID:W. Europe Standard Time
BEGIN:STANDARD
DTSTART:16010101T030000
TZOFFSETFROM:+0200
TZOFFSETTO:+0100
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10
END:STANDARD
BEGIN:DAYLIGHT
DTSTART:16010101T020000
TZOFFSETFROM:+0100
TZOFFSETTO:+0200
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3
END:DAYLIGHT
END:VTIMEZONE
BEGIN:VEVENT
ORGANIZER;CN=Grace Hopper:mailto:grace@contoso.example
ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=Bob Builde
 r:mailto:Bob@Example.org
ATTENDEE;ROLE=OPT-PARTICIPANT;PARTSTAT=TENTATIVE;RSVP=TRUE;CN=Alan Turing:m
 ailto:alan@contoso.example
COMMENT;LANGUAGE=en-US:Could we do Wednesday instead?\\n
UID:040000008200E00074C5B7101A82E00800000000A0B1C2D3E4F5A601000000000000000010
 000000F1E2D3C4B5A69788A9B0C1D2E3F4A5B6
SUMMARY;LANGUAGE=en-US:New Time Proposed: Compiler roadmap Q4
DTSTART;TZID=W. Europe Standard Time:20261007T160000
DTEND;TZID=W. Europe Standard Time:20261007T170000
CLASS:PUBLIC
PRIORITY:5
DTSTAMP:20261002T081000Z
TRANSP:OPAQUE
STATUS:CONFIRMED
SEQUENCE:0
LOCATION;LANGUAGE=en-US:Room Ada
X-MICROSOFT-CDO-APPT-SEQUENCE:0
X-MICROSOFT-CDO-BUSYSTATUS:TENTATIVE
X-MS-OLK-ORIGINALSTART:20261006T140000
X-MS-OLK-ORIGINALEND:20261006T150000
END:VEVENT
END:VCALENDAR
"""

write(
    "outlook-counter.eml",
    f"""From: Alan Turing <alan@contoso.example>
To: Grace Hopper <grace@contoso.example>
Subject: New Time Proposed: Compiler roadmap Q4
Thread-Topic: Compiler roadmap Q4
Date: Fri, 2 Oct 2026 08:10:00 +0000
Message-ID: <AM0PR01MB0003A1B2C3D4E5F6071@AM0PR01MB0003.eurprd01.prod.outlook.example>
Content-Type: multipart/alternative;
	boundary="_000_AM0PR01MB0003A1B2C3D4E5F6071AM0PR01MB0003eurp_"
MIME-Version: 1.0

--_000_AM0PR01MB0003A1B2C3D4E5F6071AM0PR01MB0003eurp_
Content-Type: text/plain; charset="us-ascii"

Could we do Wednesday instead?

--_000_AM0PR01MB0003A1B2C3D4E5F6071AM0PR01MB0003eurp_
Content-Type: text/calendar; charset="utf-8"; method=COUNTER
Content-Transfer-Encoding: base64

{b64(OUTLOOK_COUNTER_ICS)}
--_000_AM0PR01MB0003A1B2C3D4E5F6071AM0PR01MB0003eurp_--
""",
)

# --------------------------------------------------------------------------
# Apple Calendar (macOS): Bob accepts one occurrence of Ada's Google series.
# Apple sends the calendar part as a named attachment with method=REPLY.

APPLE_REPLY_ICS = """BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Apple Inc.//macOS 15.0//EN
METHOD:REPLY
CALSCALE:GREGORIAN
BEGIN:VTIMEZONE
TZID:Europe/Berlin
BEGIN:DAYLIGHT
TZOFFSETFROM:+0100
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU
DTSTART:19810329T020000
TZNAME:CEST
TZOFFSETTO:+0200
END:DAYLIGHT
BEGIN:STANDARD
TZOFFSETFROM:+0200
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU
DTSTART:19961027T030000
TZNAME:CET
TZOFFSETTO:+0100
END:STANDARD
END:VTIMEZONE
BEGIN:VEVENT
ATTENDEE;CN="Bob Builder";CUTYPE=INDIVIDUAL;EMAIL="bob@example.org";PARTSTAT
 =DECLINED;ROLE=REQ-PARTICIPANT:mailto:bob@example.org
DTEND;TZID=Europe/Berlin:20261019T103000
DTSTAMP:20261002T071233Z
DTSTART;TZID=Europe/Berlin:20261019T100000
LAST-MODIFIED:20261002T071233Z
ORGANIZER;CN="Ada Lovelace":mailto:ada@example.org
RECURRENCE-ID;TZID=Europe/Berlin:20261019T100000
REQUEST-STATUS:2.0;Success
SEQUENCE:0
SUMMARY:Engine sync
UID:5k1d9m3q7v0hb2c8e4f6g1j3l5@google.com
X-APPLE-TRAVEL-ADVISORY-BEHAVIOR:AUTOMATIC
END:VEVENT
END:VCALENDAR
"""

write(
    "apple-reply-occurrence.eml",
    f"""Content-Type: multipart/mixed;
	boundary="Apple-Mail=_0A1B2C3D-4E5F-6A7B-8C9D-0E1F2A3B4C5D"
Subject: Invitation Declined: Engine sync
Mime-Version: 1.0 (Mac OS X Mail 16.0 \\(3774.600.62\\))
From: Bob Builder <bob@example.org>
Date: Fri, 2 Oct 2026 09:12:33 +0200
Message-Id: <0A1B2C3D-4E5F-6A7B-8C9D-0E1F2A3B4C5D@example.org>
To: Ada Lovelace <ada@example.org>

--Apple-Mail=_0A1B2C3D-4E5F-6A7B-8C9D-0E1F2A3B4C5D
Content-Transfer-Encoding: 7bit
Content-Type: text/plain;
	charset=us-ascii

Bob Builder has declined your event.

--Apple-Mail=_0A1B2C3D-4E5F-6A7B-8C9D-0E1F2A3B4C5D
Content-Disposition: attachment;
	filename=iCal-20261002-091233.ics
Content-Type: text/calendar;
	x-unix-mode=0644;
	name="iCal-20261002-091233.ics";
	method=REPLY
Content-Transfer-Encoding: quoted-printable

{qp(APPLE_REPLY_ICS)}
--Apple-Mail=_0A1B2C3D-4E5F-6A7B-8C9D-0E1F2A3B4C5D--
""",
)

# --------------------------------------------------------------------------
# Thunderbird: Bob's reply to the whole Google series (inline, QP-free).

THUNDERBIRD_REPLY_ICS = """BEGIN:VCALENDAR
PRODID:-//Mozilla.org/NONSGML Mozilla Calendar V1.1//EN
VERSION:2.0
METHOD:REPLY
BEGIN:VTIMEZONE
TZID:Europe/Berlin
X-TZINFO:Europe/Berlin[2026b]
BEGIN:DAYLIGHT
TZOFFSETTO:+020000
TZOFFSETFROM:+010000
TZNAME:CEST
DTSTART:20260329T020000
RDATE:20260329T020000
END:DAYLIGHT
BEGIN:STANDARD
TZOFFSETTO:+010000
TZOFFSETFROM:+020000
TZNAME:CET
DTSTART:20261025T030000
RDATE:20261025T030000
END:STANDARD
END:VTIMEZONE
BEGIN:VEVENT
CREATED:20261001T090511Z
LAST-MODIFIED:20261001T120000Z
DTSTAMP:20261001T120000Z
UID:5k1d9m3q7v0hb2c8e4f6g1j3l5@google.com
SUMMARY:Engine sync
ORGANIZER;CN=Ada Lovelace:mailto:ada@example.org
ATTENDEE;RSVP=TRUE;CN=bob@example.org;PARTSTAT=ACCEPTED;ROLE=REQ-PARTICIPA
 NT:mailto:bob@example.org
RRULE:FREQ=WEEKLY;BYDAY=MO
DTSTART;TZID=Europe/Berlin:20261005T100000
DTEND;TZID=Europe/Berlin:20261005T103000
SEQUENCE:0
TRANSP:OPAQUE
X-MOZ-SEND-INVITATIONS:TRUE
COMMENT:See you Monday.
END:VEVENT
END:VCALENDAR
"""

write(
    "thunderbird-reply.eml",
    f"""Message-ID: <a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d@example.org>
Date: Thu, 1 Oct 2026 14:00:00 +0200
MIME-Version: 1.0
User-Agent: Mozilla Thunderbird
From: bob@example.org
To: ada@example.org
Subject: Accepted: Engine sync
Content-Type: multipart/mixed; boundary="------------0a1b2c3d4e5f6a7b8c9d0e1f"

This is a multi-part message in MIME format.
--------------0a1b2c3d4e5f6a7b8c9d0e1f
Content-Type: text/plain; charset=UTF-8
Content-Transfer-Encoding: 8bit

bob@example.org has accepted this invitation.

--------------0a1b2c3d4e5f6a7b8c9d0e1f
Content-Type: text/calendar; charset=UTF-8; method=REPLY
Content-Transfer-Encoding: 8bit

{THUNDERBIRD_REPLY_ICS}
--------------0a1b2c3d4e5f6a7b8c9d0e1f
Content-Type: application/ics; name="invite.ics"
Content-Disposition: attachment; filename="invite.ics"
Content-Transfer-Encoding: base64

{b64(THUNDERBIRD_REPLY_ICS)}
--------------0a1b2c3d4e5f6a7b8c9d0e1f--
""",
)

# --------------------------------------------------------------------------
# A spoofed "invitation": claims Ada as organizer, sent by someone else.

write(
    "spoofed-request.eml",
    f"""From: "Ada Lovelace" <ada@evil.example>
To: bob@example.org
Subject: Invitation: Engine sync
Date: Thu, 01 Oct 2026 09:30:00 +0000
Message-ID: <spoof-1@evil.example>
MIME-Version: 1.0
Content-Type: text/calendar; charset=utf-8; method=REQUEST

{GOOGLE_REQUEST_ICS.replace("SEQUENCE:0", "SEQUENCE:5")}""",
)
