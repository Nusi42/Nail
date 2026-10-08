/**
 * Accessibility of the Exchange UI (axe-core in jsdom, like
 * `src/a11y.test.tsx`): every widget, every mount point (with a room
 * search open, the setup parts with warnings) and every dialog with real
 * content. Contrast needs a real layout engine (Playwright).
 */
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "../../test/axe";
import { NodeView } from "../../layout/LayoutRenderer";
import {
  renderWithProviders,
  widget,
  type Env,
  settleQueries,
  shownDialog,
} from "../../test/harness";
import {
  dialogs,
  ExchangeAccountType,
  ExchangeOptionsFields,
  ExchangeRuleBadges,
  ExchangeServerReview,
  ExchangeSignInFields,
  ExchangeStatusPanel,
  FocusMoveButton,
  MailboxFolderBadge,
  MessageCategories,
  RoomFinder,
  TaskListBadge,
  TaskListChooser,
} from ".";
import { exchangeCandidate } from "./model";
import { EXCHANGE_ACCOUNT, EXCHANGE_CALENDARS, SHARED_ROOT } from "./mockSeed";
import {
  APPLY_CATEGORIES_DIALOG,
  AUTOMATIC_REPLIES_DIALOG,
  CATEGORY_DELETE_DIALOG,
  CATEGORY_EDITOR_DIALOG,
  CLASSIFY_DIALOG,
  DIALOGS,
  MAILBOX_EDITOR_DIALOG,
  MAILBOX_REMOVE_DIALOG,
} from "./names";
import { exchangeStoreFor } from "./store";
import { setup } from "./testing";
import { must } from "../../test/must";
import { messageWithSubject, cacheList, selectCached } from "../../test/env";

describe("axe: Exchange widgets", () => {
  it("the folder-pane widgets, tabs, banner and rules have no violations", async () => {
    const env = setup();
    const replies = env.transport.data.features.exchange.automaticReplies[EXCHANGE_ACCOUNT];
    if (replies) replies.settings.status = "always-enabled";
    const { container } = renderWithProviders(
      <>
        <NodeView node={widget("automatic-replies-banner")} path="t/0" />
        <NodeView node={widget("focused-tabs")} path="t/1" />
        <NodeView node={widget("exchange-mailboxes")} path="t/2" />
        <NodeView node={widget("exchange-categories")} path="t/3" />
        <NodeView node={widget("exchange-rules")} path="t/4" />
      </>,
      env,
    );
    await settleQueries(env, 20);
    expect(await screen.findByRole("switch", { name: "Focused Inbox" })).toBeInTheDocument();
    expect(screen.getByText("Automatic replies are on for Contoso.")).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("the mount points have no violations", async () => {
    const env = setup();
    const user = userEvent.setup();
    const phoenix = messageWithSubject(env, "Phoenix launch checklist");
    cacheList(env, [phoenix]);
    const root = env.transport.data.folders.find((f) => f.id === SHARED_ROOT);
    const lists = env.transport.data.features.pim.calendars.filter((c) =>
      c.components.includes("task"),
    );
    const todo = lists.find((c) => c.id === EXCHANGE_CALENDARS.tasks);
    if (!root || !todo) throw new Error("demo data missing");
    const email = "ada@contoso.example";
    const redirected = exchangeCandidate(email, "ews", "mail.hoster.example");
    const { container } = renderWithProviders(
      <>
        <MessageCategories message={phoenix} />
        <FocusMoveButton message={phoenix} />
        <MailboxFolderBadge folder={root} />
        <ExchangeRuleBadges ruleId="rust-announcements" />
        <TaskListChooser lists={lists} value="" onChange={() => undefined} />
        <TaskListBadge list={todo} />
        <RoomFinder
          calendarId={EXCHANGE_CALENDARS.calendar}
          start={{ type: "zoned", local: "2026-10-01T09:00:00", tz: "UTC" }}
          end={{ type: "zoned", local: "2026-10-01T09:30:00", tz: "UTC" }}
          attendees={[
            {
              email: "room-ada@contoso.example",
              name: "Ada room",
              role: "non-participant",
              status: "accepted",
              kind: "room",
              rsvp: true,
              delegated_to: [],
              delegated_from: [],
            },
          ]}
          onAttendeesChange={() => undefined}
          location=""
          onLocationChange={() => undefined}
          tz="UTC"
        />
        <ExchangeAccountType choice="ews" host="mail.corp.example" onChange={() => undefined} />
        <ExchangeServerReview
          email={email}
          candidate={{ ...redirected, source: "autodiscover" }}
          onChange={() => undefined}
          confirmed={false}
          onConfirm={() => undefined}
        />
        <ExchangeSignInFields
          email={email}
          engine="ews"
          auth={{ method: "windows", username: "CONTOSO\\ada", secret: null, package: null }}
          onChange={() => undefined}
          password=""
          onPasswordChange={() => undefined}
        />
        <ExchangeOptionsFields
          options={{
            engine: "ews",
            url: null,
            poll_interval_secs: null,
            max_poll_interval_secs: null,
            streaming: null,
            server_rules: null,
          }}
          onChange={() => undefined}
        />
        <ExchangeStatusPanel accountId={EXCHANGE_ACCOUNT} />
      </>,
      env,
    );
    await user.click(await screen.findByRole("button", { name: "Find a room…" }));
    await screen.findByRole("list", { name: "Rooms for this time" });
    await settleQueries(env, 20);
    expect(await violations(container)).toEqual([]);
  }, 30_000);
});

describe("axe: Exchange dialogs", () => {
  function prepare(env: Env, name: string, existing: boolean): void {
    const ui = exchangeStoreFor(env.store).getState();
    if (name === MAILBOX_EDITOR_DIALOG) {
      ui.editMailbox({
        accountId: EXCHANGE_ACCOUNT,
        address: existing ? "support@contoso.example" : null,
      });
    }
    if (name === MAILBOX_REMOVE_DIALOG) {
      const mailbox = env.transport.data.features.exchange.mailboxes[EXCHANGE_ACCOUNT]?.[0];
      ui.removeMailbox(mailbox ?? null);
    }
    if (name === CATEGORY_EDITOR_DIALOG) {
      ui.editCategory({ accountId: EXCHANGE_ACCOUNT, categoryId: existing ? "{0004-cat}" : null });
    }
    if (name === CATEGORY_DELETE_DIALOG) {
      const category = env.transport.data.features.exchange.categories[EXCHANGE_ACCOUNT]?.[0];
      ui.deleteCategory(category ?? null);
    }
    if (name === AUTOMATIC_REPLIES_DIALOG) {
      const replies = env.transport.data.features.exchange.automaticReplies[EXCHANGE_ACCOUNT];
      if (replies && existing) {
        replies.settings = {
          ...replies.settings,
          status: "scheduled",
          start: "2026-10-05T07:00:00Z",
          end: "2026-10-09T16:00:00Z",
        };
      }
      ui.editReplies(EXCHANGE_ACCOUNT);
    }
    if (name === CLASSIFY_DIALOG) {
      const summary = messageWithSubject(env, "Your weekly digest");
      cacheList(env, [summary]);
      ui.classifyMessages({ messageIds: [summary.id], to: "focused" });
    }
    if (name === APPLY_CATEGORIES_DIALOG) {
      const summary = messageWithSubject(env, "Phoenix launch checklist");
      selectCached(env, summary);
    }
  }

  const cases = [
    ...DIALOGS.map((d) => [d, false] as const),
    [MAILBOX_EDITOR_DIALOG, true] as const,
    [CATEGORY_EDITOR_DIALOG, true] as const,
    [AUTOMATIC_REPLIES_DIALOG, true] as const,
  ];
  it.each(cases)(
    "dialog %s (existing: %s) has no violations",
    async (name, existing) => {
      const env = setup();
      prepare(env, name, existing);
      const container = await shownDialog(env, dialogs, name);
      expect(
        container.querySelector('[data-part="exchange.dialog"], [data-part="exchange.oof"]'),
      ).not.toBeNull();
      expect(container.querySelector('[data-state="empty"]')).toBeNull();
      expect(await violations(container)).toEqual([]);
    },
    30_000,
  );

  it("an empty dialog (nothing chosen) has no violations", async () => {
    const env = setup();
    const Dialog = must(dialogs[CATEGORY_EDITOR_DIALOG]);
    const { container } = renderWithProviders(
      <div role="dialog" aria-label={CATEGORY_EDITOR_DIALOG}>
        <Dialog name={CATEGORY_EDITOR_DIALOG} close={() => undefined} />
      </div>,
      env,
    );
    expect(container.querySelector('[data-state="empty"]')).not.toBeNull();
    expect(await violations(container)).toEqual([]);
  });
});
