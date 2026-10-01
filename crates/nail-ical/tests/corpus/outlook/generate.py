"""Writes contacts-vcard21.vcf: an Outlook-style vCard 2.1 export in
Windows-1252 with quoted-printable values (synthesized; see ../README.md).

Run from this folder: python generate.py
"""

LINES = [
    "BEGIN:VCARD",
    "VERSION:2.1",
    "N;LANGUAGE=de-de:Müller;Jürgen",
    "FN:Jürgen Müller",
    "ORG:Rechenmaschinen GmbH;Entwicklung",
    "TITLE:Ingenieur",
    "TEL;WORK;VOICE:+49 30 1234567",
    "TEL;CELL;VOICE:+49 170 1234567",
    "ADR;WORK;PREF:;;Hauptstraße 1;Berlin;;10115;Deutschland",
    "LABEL;WORK;PREF;ENCODING=QUOTED-PRINTABLE:Hauptstra=DFe 1=0D=0A=",
    "Berlin 10115=0D=0A=",
    "Deutschland",
    "EMAIL;PREF;INTERNET:juergen.mueller@rechen.example",
    "NOTE;ENCODING=QUOTED-PRINTABLE;CHARSET=Windows-1252:Gr=FC=DFe aus der Entwi=",
    "cklung.",
    "X-MS-OL-DEFAULT-POSTAL-ADDRESS:2",
    "X-MS-OL-DESIGN;CHARSET=utf-8:<card xmlns=\"http://schemas.microsoft.com/office/outlook/12/electronicbusinesscards\"/>",
    "REV:20260915T081500Z",
    "END:VCARD",
]

with open("contacts-vcard21.vcf", "wb") as out:
    out.write(("\r\n".join(LINES) + "\r\n").encode("cp1252"))
