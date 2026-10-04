/**
 * Picks the transport for the current environment:
 * `?mock=1` → mock (dev), Tauri webview → Tauri IPC, otherwise WebSocket.
 */
import { TransportClient, type NailClient, type Transport } from "./client";
import { ReauthTransport } from "./reauth";

export type TransportKind = "mock" | "tauri" | "websocket";

/** Decides the transport kind from the page environment. */
export function selectTransportKind(env: { search: string; hasTauri: boolean }): TransportKind {
  const mock = new URLSearchParams(env.search).get("mock");
  if (mock === "1" || mock === "true") {
    return "mock";
  }
  return env.hasTauri ? "tauri" : "websocket";
}

/** Creates a client for the current window. Transports load lazily. */
export async function createClient(win: Window = window): Promise<NailClient> {
  const kind = selectTransportKind({
    search: win.location.search,
    hasTauri: "__TAURI_INTERNALS__" in win,
  });
  let transport: Transport;
  switch (kind) {
    case "mock": {
      const { MockTransport, securityDemoBySender } = await import("./mock");
      const mock = new MockTransport({ latencyMs: 80 });
      // The dev mailbox shows every security badge and warning kind.
      mock.data.features.security.bySender = securityDemoBySender();
      // Invitations (M3b) and a Gmail account with labels and categories.
      const { seedSchedulingDemo } = await import("../features/scheduling/mockSeed");
      const { seedGmailDemo } = await import("../features/gmail/mockSeed");
      seedSchedulingDemo(mock.data);
      seedGmailDemo(mock.data);
      // An Exchange Online account (M3c): Focused Inbox, categories, shared mailboxes, rooms.
      const { seedExchangeDemo } = await import("../features/exchange/mockSeed");
      seedExchangeDemo(mock.data);
      transport = mock;
      break;
    }
    case "tauri": {
      const { TauriTransport } = await import("./tauri");
      transport = new TauriTransport();
      break;
    }
    case "websocket": {
      const { WebSocketTransport } = await import("./websocket");
      // The remote UI asks to confirm the user before admin requests.
      transport = new ReauthTransport(new WebSocketTransport());
      break;
    }
  }
  return new TransportClient(transport);
}
