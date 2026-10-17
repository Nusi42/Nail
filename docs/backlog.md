# Backlog

Items that are agreed but not scheduled in a milestone yet. Move an item into `docs/plan.md` when it gets scheduled.

## Real-account and real-server testing

Decided: the tests against real Google and Microsoft services wait in the backlog; until then the automated suites use mock servers and recorded fixtures.

| Item | What it covers | Notes |
| --- | --- | --- |
| Google test account | Gmail API engine, Google CalDAV/CardDAV, OAuth sign-in | The owner creates the account (phone verification, Google's terms). Then: Google Cloud project, Gmail API enabled, OAuth consent screen in "Testing" mode with the account as test user (full Gmail scope without app review), desktop OAuth client id into `accounts.kdl` `oauth_providers`. In Testing mode refresh tokens expire after about 7 days (check), so a longer trial needs re-sign-in. |
| Outlook.com account (free) | Graph engine for personal accounts: mail, calendar, contacts, To Do | Free Entra app registration. Does not cover shared mailboxes, rooms, business inbox rules / out of office, or EWS. |
| Microsoft 365 Business tenant | All of Exchange Online: shared mailboxes, rooms, rules, out of office, invitations between users | Business Basic trial (about one month free, then a small per-user fee; check current prices) with the free `onmicrosoft.com` domain, 2–3 users. Alternative: Microsoft 365 Developer Program sandbox, if eligible (eligibility was restricted, e.g. to Visual Studio subscribers — check). |
| grommunio in the interop run | EWS engine against a real EWS implementation | Open-source groupware with EWS, runs as a container next to Dovecot, Stalwart and Radicale. Complements, does not replace, a Microsoft Exchange Server. |
| Exchange Server lab (optional) | EWS + Windows integrated sign-in (SSPI) against Microsoft's server | Hyper-V on Windows 11 Pro: Windows Server evaluation, Active Directory, Exchange Server Subscription Edition in trial mode; about 16 GB RAM for the VMs. Only if on-premises customers matter. |
