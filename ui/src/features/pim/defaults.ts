/**
 * Empty items for the editors and new items (no title, no times beyond the
 * given ones, every list empty).
 */
import type { ContactFields, EventFields, PimTime, TaskFields } from "../../api/generated";

export function emptyEventFields(start: PimTime, end: PimTime): EventFields {
  return {
    title: "",
    start,
    end,
    location: null,
    description: null,
    status: "confirmed",
    transparency: "busy",
    visibility: "public",
    priority: null,
    categories: [],
    color: null,
    url: null,
    organizer: null,
    attendees: [],
    alarms: [],
    conferences: [],
    attachments: [],
    links: [],
  };
}

export function emptyTaskFields(title = ""): TaskFields {
  return {
    title,
    description: null,
    start: null,
    due: null,
    status: "needs-action",
    percent_complete: null,
    priority: null,
    completed_at: null,
    categories: [],
    location: null,
    url: null,
    visibility: "public",
    alarms: [],
    parent_id: null,
    order: null,
    links: [],
  };
}

export function emptyContactFields(displayName = ""): ContactFields {
  return {
    kind: "individual",
    display_name: displayName,
    name: null,
    nicknames: [],
    emails: [],
    phones: [],
    addresses: [],
    urls: [],
    impp: [],
    organization: null,
    title: null,
    role: null,
    birthday: null,
    anniversary: null,
    notes: null,
    categories: [],
    photo: null,
    related: [],
    custom: [],
  };
}
