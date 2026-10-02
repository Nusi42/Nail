/**
 * Demo confirmations for the mock core (demo mode, screenshots, manual
 * testing): what an MCP client and plug-ins would ask while Nail runs —
 * an outgoing mail, a move to Trash, a permanent delete, a plug-in task
 * write on behalf of an agent — plus a short decided history.
 */
import type { Confirmation } from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { requestConfirmation } from "./mock";

const CLAUDE = {
  actor: { kind: "mcp-client", client_id: "mcp-1" },
  name: "Claude Desktop",
} as const;

/** Adds the demo confirmations; returns the pending ones in request order. */
export function seedAgentsDemo(core: MockCore): Confirmation[] {
  const decided = (
    fields: Parameters<typeof requestConfirmation>[1],
    state: Confirmation["state"],
  ) => {
    const c = requestConfirmation(core, fields);
    c.state = state;
    c.decided_at = new Date().toISOString();
    c.decided_via = state === "expired" ? "system" : "nail";
    return c;
  };
  decided(
    {
      requested_by: CLAUDE,
      kind: "permanent-delete",
      operation: "delete_messages",
      summary: {
        id: "confirmation-summary-permanent-delete",
        args: { actor: CLAUDE.name, count: 12 },
      },
      targets: [{ kind: "message", id: "acc-work/INBOX/3", label: "Weekly newsletter" }],
      more_targets: 11,
      account_ids: ["acc-work"],
    },
    "denied",
  );
  decided(
    {
      requested_by: CLAUDE,
      kind: "rule",
      operation: "create_rule",
      summary: { id: "confirmation-summary-rule", args: { actor: CLAUDE.name, name: "Invoices" } },
      targets: [{ kind: "rule", id: "invoices", label: "Invoices" }],
      changes: [
        {
          field: { id: "action-field-rule" },
          before: null,
          after: "from:billing@acme.example → move to Invoices",
        },
      ],
    },
    "approved",
  );
  const send = requestConfirmation(core, {
    requested_by: CLAUDE,
    kind: "send",
    operation: "send_message",
    summary: {
      id: "confirmation-summary-send",
      args: { actor: CLAUDE.name, subject: "Re: Quarterly report", count: 2 },
    },
    targets: [
      { kind: "draft", id: "draft-7", label: "Re: Quarterly report" },
      { kind: "recipient", id: "grace@example.org", label: "Grace Hopper <grace@example.org>" },
      { kind: "recipient", id: "alan@example.org", label: "alan@example.org" },
    ],
    changes: [
      { field: { id: "action-field-to" }, before: null, after: "Grace Hopper, alan@example.org" },
      { field: { id: "action-field-subject" }, before: null, after: "Re: Quarterly report" },
      { field: { id: "action-field-attachments" }, before: null, after: "q3-figures.csv" },
    ],
    account_ids: ["acc-work"],
  });
  const trash = requestConfirmation(core, {
    requested_by: CLAUDE,
    kind: "trash",
    operation: "delete_messages",
    summary: { id: "confirmation-summary-trash", args: { actor: CLAUDE.name, count: 3 } },
    targets: [
      { kind: "message", id: "acc-work/INBOX/1", label: "Lunch on Friday?" },
      { kind: "message", id: "acc-work/INBOX/2", label: "Your parcel is on its way" },
      { kind: "message", id: "acc-work/INBOX/4", label: "Reminder: dentist" },
    ],
    changes: [{ field: { id: "action-field-folder" }, before: "Inbox", after: "Trash" }],
    account_ids: ["acc-work"],
  });
  const task = requestConfirmation(core, {
    requested_by: {
      actor: { kind: "plugin", plugin_id: "follow-up" },
      name: "Follow-up",
      on_behalf_of: { client_id: "mcp-1", name: CLAUDE.name },
    },
    kind: "invite",
    operation: "nail.calendar.create",
    summary: {
      id: "confirmation-summary-invite",
      args: { actor: "Follow-up", title: "Report review", count: 3 },
    },
    targets: [{ kind: "event", id: "ev-review", label: "Report review" }],
    changes: [
      { field: { id: "action-field-title" }, before: null, after: "Report review" },
      { field: { id: "action-field-start" }, before: null, after: "2026-10-05 10:00" },
      { field: { id: "action-field-attendees" }, before: null, after: "Grace, Alan, Ada" },
    ],
    account_ids: ["acc-work"],
  });
  return [send, trash, task];
}
