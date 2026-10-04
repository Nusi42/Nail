/**
 * Exchange settings UI against the mock core: the automatic replies editor
 * and banner, rule placement badges and the inbox rules widget, and the
 * Exchange parts of account setup / settings.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { AuthConfig, DiscoveredAccount, ExchangeOptions } from "../../api/generated";
import { NodeView } from "../../layout/LayoutRenderer";
import { renderWithProviders, widget } from "../../test/harness";
import {
  actions,
  ExchangeAccountType,
  ExchangeOptionsFields,
  ExchangeRuleBadges,
  ExchangeServerReview,
  ExchangeSignInFields,
  ExchangeStatusPanel,
  handleExchangeEvent,
  needsRedirectConfirmation,
  type ExchangeSetupChoice,
} from ".";
import { defaultSchedule, utcOf, wallOf } from "./AutomaticReplies";
import { exchangeCandidate } from "./model";
import { EXCHANGE_ACCOUNT } from "./mockSeed";
import { AUTOMATIC_REPLIES_DIALOG } from "./names";
import { exchangeStoreFor } from "./store";
import { actionContext, dialog, requests, setup } from "./testing";

describe("automatic replies", () => {
  it("converts schedule times between UTC and the viewer's zone", () => {
    const wall = wallOf("2026-10-01T22:30:00Z", "Europe/Berlin");
    expect(wall.day.toString()).toBe("2026-10-02");
    expect(wall.time).toEqual({ hour: 0, minute: 30 });
    expect(utcOf(wall.day, wall.time, "Europe/Berlin")).toBe("2026-10-01T22:30:00Z");
    expect(defaultSchedule(Date.parse("2026-10-01T08:10:00Z"))).toEqual({
      start: "2026-10-01T09:00:00Z",
      end: "2026-10-08T09:00:00Z",
    });
  });

  it("turns replies on for a schedule and shows that the server is not updated yet", async () => {
    const env = setup();
    const user = userEvent.setup();
    env.transport.data.behaviour.workMs = 60_000;
    const Replies = dialog(AUTOMATIC_REPLIES_DIALOG);
    renderWithProviders(<Replies name={AUTOMATIC_REPLIES_DIALOG} close={vi.fn()} />, env);
    await user.click(
      await screen.findByRole("radio", { name: "Send automatic replies only during this time" }),
    );
    // Locale-following pickers (React Aria segments), not native inputs.
    const start = screen.getByRole("group", { name: "Start" });
    expect(within(start).getAllByRole("spinbutton").length).toBeGreaterThan(3);
    expect(screen.getByRole("group", { name: "End" })).toBeInTheDocument();
    const internal = screen.getByLabelText("Reply to people in my organization");
    expect(internal).toHaveValue("I am out of the office. Grace Hopper stands in for me.");
    expect(
      screen.getByLabelText("Reply to people outside my organization", { selector: "select" }),
    ).toHaveValue("contacts-only");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved in Nail; not on the server yet.")).toBeInTheDocument();
    const sent = requests(env, "SetAutomaticReplies")[0];
    expect(sent?.settings.status).toBe("scheduled");
    expect(sent?.settings.start).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:00:00Z$/);
    expect(Date.parse(sent?.settings.end ?? "") - Date.parse(sent?.settings.start ?? "")).toBe(
      7 * 86_400_000,
    );
    env.transport.close();
  });

  it("checks the replies before saving", async () => {
    const env = setup();
    const user = userEvent.setup();
    const Replies = dialog(AUTOMATIC_REPLIES_DIALOG);
    renderWithProviders(<Replies name={AUTOMATIC_REPLIES_DIALOG} close={vi.fn()} />, env);
    await user.click(await screen.findByRole("radio", { name: "Send automatic replies" }));
    await user.clear(screen.getByLabelText("Reply to people in my organization"));
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Write the reply for people in your organization.",
    );
    expect(requests(env, "SetAutomaticReplies")).toHaveLength(0);
    await user.selectOptions(
      screen.getByLabelText("Reply to people outside my organization", { selector: "select" }),
      "none",
    );
    expect(
      screen.queryByLabelText("Reply to people outside my organization", { selector: "textarea" }),
    ).toBeNull();
  });

  it("shows a banner while replies are on, with turn off and edit", async () => {
    const env = setup();
    const user = userEvent.setup();
    const replies = env.transport.data.features.exchange.automaticReplies[EXCHANGE_ACCOUNT];
    if (!replies) throw new Error("demo replies missing");
    replies.settings.status = "always-enabled";
    env.transport.onEvent((event) => {
      handleExchangeEvent(event, {
        client: env.client,
        queryClient: env.queryClient,
        store: env.store,
      });
    });
    renderWithProviders(<NodeView node={widget("automatic-replies-banner")} path="t/0" />, env);
    expect(await screen.findByText("Automatic replies are on for Contoso.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Edit…" }));
    expect(env.store.getState().windows).toContain(AUTOMATIC_REPLIES_DIALOG);
    expect(exchangeStoreFor(env.store).getState().repliesAccount).toBe(EXCHANGE_ACCOUNT);
    await user.click(screen.getByRole("button", { name: "Turn off" }));
    await waitFor(() => {
      expect(screen.queryByText("Automatic replies are on for Contoso.")).toBeNull();
    });
    expect(requests(env, "SetAutomaticReplies")[0]?.settings.status).toBe("disabled");
  });

  it("announces an upcoming schedule and stays empty while off", async () => {
    const env = setup();
    const replies = env.transport.data.features.exchange.automaticReplies[EXCHANGE_ACCOUNT];
    if (!replies) throw new Error("demo replies missing");
    const { container } = renderWithProviders(
      <NodeView node={widget("automatic-replies-banner")} path="t/0" />,
      env,
    );
    await waitFor(() => {
      expect(env.queryClient.isFetching()).toBe(0);
    });
    expect(container.querySelector('[data-part="automatic-replies-banner.item"]')).toBeNull();
    const env2 = setup();
    const r2 = env2.transport.data.features.exchange.automaticReplies[EXCHANGE_ACCOUNT];
    if (!r2) throw new Error("demo replies missing");
    r2.settings = {
      ...r2.settings,
      status: "scheduled",
      start: "2099-01-01T08:00:00Z",
      end: "2099-01-08T08:00:00Z",
    };
    renderWithProviders(<NodeView node={widget("automatic-replies-banner")} path="t/1" />, env2);
    expect(await screen.findByText(/Automatic replies for Contoso start/)).toBeInTheDocument();
  });

  it("opens the editor from the action when there is an Exchange account", async () => {
    const env = setup("acc-work");
    const ctx = actionContext(env);
    const handler = actions["exchange.automatic-replies"];
    expect(handler?.enabled?.(ctx, [])).toBe(false);
    await env.queryClient.query({
      queryKey: ["accounts"],
      queryFn: async () =>
        (await env.client.request({ type: "ListAccounts" }, "Accounts")).accounts,
    });
    expect(handler?.enabled?.(ctx, [])).toBe(true);
    await handler?.run(ctx, [EXCHANGE_ACCOUNT]);
    expect(env.store.getState().windows).toContain(AUTOMATIC_REPLIES_DIALOG);
  });
});

describe("rules", () => {
  it("badges where a Nail rule runs on each Exchange account, with the reasons", async () => {
    const env = setup();
    renderWithProviders(<ExchangeRuleBadges ruleId="rust-announcements" />, env);
    const badges = await screen.findByRole("list", {
      name: "Where this rule runs on Exchange accounts",
    });
    await waitFor(() => expect(badges).toHaveTextContent("Contoso: in Nail"));
    expect(badges).toHaveTextContent("Condition 2 uses a search query");
    const badge = badges.querySelector('[data-part="exchange.rule-badge"]');
    expect(badge?.getAttribute("data-placement")).toBe("client");
  });

  it("lists placements and server rules; switches and deletes the user's own", async () => {
    const env = setup();
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("exchange-rules")} path="t/0" />, env);
    const table = await screen.findByRole("table", { name: "Where your Nail rules run" });
    expect(
      await within(table).findByRole("rowheader", { name: "Rust announcements" }),
    ).toBeInTheDocument();
    expect(within(table).getByText("In Nail")).toBeInTheDocument();
    expect(
      screen.getByText("The server's rules are up to date.", { exact: false }),
    ).toBeInTheDocument();
    const outlook = screen.getByRole("checkbox", { name: "Move newsletters (created in Outlook)" });
    expect(outlook).not.toBeChecked();
    expect(screen.getByText(/Nail cannot show all of this rule/)).toBeInTheDocument();
    await user.click(outlook);
    await waitFor(() => {
      expect(requests(env, "SetExchangeInboxRuleEnabled")).toHaveLength(1);
    });
    expect(requests(env, "SetExchangeInboxRuleEnabled")[0]).toMatchObject({
      server_rule_id: "inbox-rule-3",
      enabled: true,
    });
    await user.click(
      screen.getByRole("button", { name: "Delete inbox rule Invoices to accounting" }),
    );
    expect(
      screen.getByText("Delete the inbox rule “Invoices to accounting” from the server?"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(requests(env, "DeleteExchangeInboxRule")).toHaveLength(1);
    });
    await user.click(screen.getByRole("button", { name: "Sync now" }));
    await waitFor(() => {
      expect(requests(env, "SyncExchangeRules")).toHaveLength(1);
    });
  });

  it("switches running rules on the server through the account settings", async () => {
    const env = setup();
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("exchange-rules")} path="t/0" />, env);
    const toggle = await screen.findByRole("checkbox", {
      name: "Run rules on the server where Exchange can",
    });
    expect(toggle).toBeChecked();
    await user.click(toggle);
    await waitFor(() => {
      expect(requests(env, "UpdateAccount")).toHaveLength(1);
    });
    const config = requests(env, "UpdateAccount")[0]?.config;
    expect(config?.incoming).toMatchObject({
      protocol: "exchange",
      options: { server_rules: false },
    });
  });

  it("says when there is no Exchange account", async () => {
    const env = setup("acc-work");
    env.transport.data.accounts = env.transport.data.accounts.filter((a) => a.kind !== "exchange");
    renderWithProviders(<NodeView node={widget("exchange-rules")} path="t/0" />, env);
    expect(await screen.findByText("There is no Exchange account.")).toBeInTheDocument();
  });
});

describe("account setup and settings", () => {
  it("chooses the kind of account and the server of an Exchange Server", async () => {
    const env = setup();
    const user = userEvent.setup();
    const seen: [ExchangeSetupChoice, string][] = [];
    function Host() {
      const [choice, setChoice] = useState<ExchangeSetupChoice>("auto");
      const [host, setHost] = useState("");
      return (
        <ExchangeAccountType
          choice={choice}
          host={host}
          onChange={(c, h) => {
            seen.push([c, h]);
            setChoice(c);
            setHost(h);
          }}
        />
      );
    }
    renderWithProviders(<Host />, env);
    expect(screen.queryByLabelText("Exchange server")).toBeNull();
    await user.click(screen.getByRole("radio", { name: "Exchange Server in my organization" }));
    await user.type(screen.getByLabelText("Exchange server"), "mail.corp.example");
    expect(seen.at(-1)).toEqual(["ews", "mail.corp.example"]);
    await user.click(screen.getByRole("radio", { name: "Microsoft 365 or Exchange Online" }));
    expect(seen.at(-1)?.[0]).toBe("graph");
  });

  it("reviews an Autodiscover answer: redirect warning, confirmation, engine switch", async () => {
    const env = setup();
    const user = userEvent.setup();
    const email = "ada@contoso.example";
    const found: DiscoveredAccount = (() => {
      const c = exchangeCandidate(email, "ews", "mail.hoster.example");
      return {
        ...c,
        source: "autodiscover",
        incoming:
          c.incoming.protocol === "exchange"
            ? {
                ...c.incoming,
                options: {
                  ...c.incoming.options,
                  url: "https://mail.hoster.example/EWS/Exchange.asmx",
                },
              }
            : c.incoming,
      };
    })();
    expect(needsRedirectConfirmation(email, found)).toBe(true);
    const latest: { candidate: DiscoveredAccount; confirmed: boolean } = {
      candidate: found,
      confirmed: false,
    };
    function Host() {
      const [candidate, setCandidate] = useState(found);
      const [confirmed, setConfirmed] = useState(false);
      latest.candidate = candidate;
      latest.confirmed = confirmed;
      return (
        <ExchangeServerReview
          email={email}
          candidate={candidate}
          onChange={setCandidate}
          confirmed={confirmed}
          onConfirm={setConfirmed}
        />
      );
    }
    renderWithProviders(<Host />, env);
    expect(screen.getByText("Microsoft Autodiscover")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "The settings for contoso.example point to mail.hoster.example",
    );
    await user.click(screen.getByRole("checkbox", { name: "I trust mail.hoster.example" }));
    expect(latest.confirmed).toBe(true);
    expect(screen.getByLabelText("EWS address")).toHaveValue(
      "https://mail.hoster.example/EWS/Exchange.asmx",
    );
    await user.click(screen.getByRole("radio", { name: "Microsoft Graph (Exchange Online)" }));
    expect(latest.candidate.incoming).toMatchObject({
      server: { host: "graph.microsoft.com" },
      options: { engine: "graph" },
    });
    expect(screen.queryByLabelText("EWS address")).toBeNull();
  });

  it("warns about EWS against Exchange Online", () => {
    const env = setup();
    const c = exchangeCandidate("ada@contoso.example", "ews", "outlook.office365.com");
    renderWithProviders(
      <ExchangeServerReview
        email="ada@contoso.example"
        candidate={{ ...c, source: "autodiscover" }}
        onChange={vi.fn()}
        confirmed={false}
        onConfirm={vi.fn()}
      />,
      env,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Microsoft is retiring Exchange Web Services in Exchange Online.",
    );
  });

  it("signs in as the Windows user, another Windows account, or with OAuth for Graph", async () => {
    const env = setup();
    const user = userEvent.setup();
    const state: { auth: AuthConfig; password: string } = {
      auth: { method: "windows", username: null, secret: null, package: null },
      password: "",
    };
    function Host({ engine }: { engine: "graph" | "ews" }) {
      const [auth, setAuth] = useState<AuthConfig>(state.auth);
      const [password, setPassword] = useState("");
      state.auth = auth;
      state.password = password;
      return (
        <ExchangeSignInFields
          email="ada@corp.example"
          engine={engine}
          auth={auth}
          onChange={setAuth}
          password={password}
          onPasswordChange={setPassword}
        />
      );
    }
    const { unmount } = renderWithProviders(<Host engine="ews" />, env);
    expect(screen.getByRole("radio", { name: "As the user signed in to Windows" })).toBeChecked();
    expect(
      screen.getByText("Nail signs in like Outlook does and stores no password."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).toBeNull();
    await user.click(screen.getByRole("radio", { name: "With another Windows account" }));
    expect(screen.getByLabelText("Windows account (DOMAIN\\user)")).toHaveValue("CORP\\ada");
    await user.selectOptions(screen.getByLabelText("Sign-in protocol"), "NTLM only");
    await user.type(screen.getByLabelText("Password"), "secret");
    expect(state.auth).toEqual({
      method: "windows",
      username: "CORP\\ada",
      secret: null,
      package: "ntlm",
    });
    expect(state.password).toBe("secret");
    unmount();
    state.auth = exchangeCandidate("ada@corp.example", "graph").incoming.server.auth;
    renderWithProviders(<Host engine="graph" />, env);
    expect(screen.queryByRole("radio")).toBeNull();
    expect(
      screen.getByText("Microsoft 365 accounts sign in with Microsoft in your browser."),
    ).toBeInTheDocument();
  });

  it("edits the account options and shows the status", async () => {
    const env = setup();
    const user = userEvent.setup();
    const changes: ExchangeOptions[] = [];
    function Host() {
      const [options, setOptions] = useState<ExchangeOptions>({
        engine: "ews",
        url: null,
        poll_interval_secs: null,
        max_poll_interval_secs: null,
        streaming: null,
        server_rules: null,
      });
      return (
        <ExchangeOptionsFields
          options={options}
          onChange={(o) => {
            changes.push(o);
            setOptions(o);
          }}
        />
      );
    }
    renderWithProviders(
      <>
        <Host />
        <ExchangeStatusPanel accountId={EXCHANGE_ACCOUNT} />
      </>,
      env,
    );
    await user.type(screen.getByLabelText("Check for changes every (seconds)"), "60");
    await user.click(
      screen.getByRole("checkbox", { name: "Get changes at once (streaming notifications)" }),
    );
    expect(changes.at(-1)).toMatchObject({ poll_interval_secs: 60, streaming: false });
    const status = await screen.findByRole("region", { name: "Exchange status" });
    await waitFor(() => expect(status).toHaveTextContent("by checking regularly"));
    expect(status).toHaveTextContent("8 of 8");
    await act(() => Promise.resolve());
  });
});
