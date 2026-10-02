# Background jobs

With the `background` permission a plug-in can run jobs on a schedule or
when something happens. A job is declared in the manifest
(`contributes.jobs`, at most 10) and its handler registered in code:

```json
"permissions": ["background", "notifications", "storage"],
"contributes": {
  "jobs": [
    { "id": "digest", "title": "%digest-title%", "schedule": "0 8 * * 1-5" },
    { "id": "on-mail", "events": ["message.received"] }
  ]
}
```

```ts
export default definePlugin({
  jobs: {
    digest: async (trigger) => { /* trigger.kind === "schedule" */ },
    "on-mail": async (trigger) => {
      if (trigger.kind === "event") for (const id of trigger.messageIds) { /* … */ }
    },
  },
});
```

(or `ctx.jobs.register(id, handler)` in `activate`). A job needs a
`schedule`, `events`, or both. The plug-in manager lists jobs under
**Activity** with their last and next run, and **Run now** runs one by
hand (`trigger.kind === "manual"`).

## Schedules

Five fields in local time — minute, hour, day of month, month, day of
week (0–7; 0 and 7 are Sunday) — each `*`, a number, a range (`1-5`), a
step (`*/15`, `9-17/2`, or `5/20` = 5, 25, 45) or a list (`0,30`); or
`@hourly`, `@daily`, `@weekly`. When both day fields are restricted, a day
matching either runs the job (as in cron).

| Schedule | Runs |
| --- | --- |
| `*/15 * * * *` | every 15 minutes |
| `0 8 * * 1-5` | at 8:00 on weekdays |
| `30 6 1 * *` | at 6:30 on the first of every month |
| `@daily` | at midnight |

A schedule that would run more often than every 5 minutes is refused. A
run missed because Nail was not running runs once when it starts.

## Events

| Event | When | The trigger carries |
| --- | --- | --- |
| `message.received` | New mail arrived (after rules and arrival hooks) | `messageIds` (readable with `mail.read`) |
| `message.sent` | A message was sent | `messageIds` |
| `selection.changed` | The selection in a window changed | `messageIds` |
| `event.reminder` | A reminder of an event or task fired (needs `calendar.read` for its data) | — |
| `app.started` | Nail (or the plug-in) started | — |

Events in quick succession are combined: many `message.received` within
2 seconds make one run with all their message ids (at most 500). Message
events only carry messages of accounts the plug-in's `mail.read` grant
covers.

## Limits

- **One run at a time** per job: a run that starts while the previous one
  is still going is skipped.
- **60 seconds** of wall-clock time per run, waiting included; then the
  run is stopped and counted as a failure.
- **CPU quota**: a plug-in may use 120 seconds of CPU time per hour across
  everything it does. Over the quota, its jobs are skipped until the hour
  is over (**Activity** shows "The plug-in used all its CPU time for this
  hour").
- A job that throws is counted as failed; the plug-in keeps running.

Writes a job makes follow the plug-in's [confirmations](writing-data.md):
a job that deletes or sends waits for the user like any other write.

## Testing

```ts
expect(await host.runJob("digest", { kind: "schedule", scheduledFor: "2026-10-05T06:00:00Z" })).toEqual({ outcome: "ok" });
```

`runJob` applies the wall-clock limit (`jobTimeoutMs` in the test host's
options); `parseCron`, `nextCronRun` and `cronMatches` from
`@nail/plugin-api/tools` check schedules the way Nail reads them.
