# Tools for AI agents

With the `mcp.expose` permission a plug-in can offer tools to the AI
agents (MCP clients) you connected to Nail ([connecting AI agents](../mcp.md)).
An agent sees them next to Nail's own tools, named
`<plugin-id>__<tool>` (at most 64 characters in all).

```json
"permissions": ["mcp.expose", "network:crm.acme.example"],
"contributes": {
  "mcpTools": [
    {
      "id": "lookup-customer",
      "description": "Finds a customer in the Acme CRM by email address and returns their open tickets.",
      "inputSchema": {
        "type": "object",
        "properties": { "email": { "type": "string" } },
        "required": ["email"]
      },
      "scopes": ["read"]
    }
  ]
}
```

```ts
export default definePlugin({
  mcpTools: {
    "lookup-customer": async ({ email }) => {
      const res = await nail.http.fetch(`https://crm.acme.example/api/customers?email=${encodeURIComponent(String(email))}`);
      return res.ok ? await res.json() : { error: `CRM answered ${res.status}` };
    },
  },
});
```

(or `ctx.mcp.registerTool(id, handler)` in `activate`).

- `description` is for the agent (English is fine; at most 1000
  characters). Say what the tool does and what it returns.
- `inputSchema` is the JSON Schema of the arguments: an object schema (at
  most 16 KB); without one the tool takes no arguments. Nail checks the
  agent's arguments against it before calling the plug-in.
- `scopes` are the MCP permissions a client needs to see and call the tool
  (default `read`): `read`, `organize`, `draft`, `send`, `calendar`,
  `pim`, `rules`, … as in the client's settings.
- The handler gets the arguments and answers JSON (or nothing). Nail hands
  the answer to the agent **marked as untrusted data**, like mail.

## Permissions and confirmations

A tool runs under two sets of permissions:

- the **client's** grant decides whether the agent may call it (`scopes`,
  and every call is in the client's activity log and counts against its
  rate limits);
- while the handler runs, it may do only what **both** the plug-in and the
  client may: the plug-in's permissions, narrowed to the client's grant.
  Mail in accounts the client may not read, or in folders it excludes, is
  "not found"; a write the client's scopes do not cover is refused
  (`forbidden`). The plug-in's own storage, network hosts, notifications
  and dialogs stay available.
- writes it makes while serving the call ask when the plug-in's **or** the
  client's confirmation setting asks, count against the client's limits
  (changes per minute, messages per hour), are in the client's activity
  log, and the confirmation names both: "Plug-in “Acme CRM” for the AI
  agent “Claude Desktop”".

So a tool cannot give an agent more than the plug-in may do, and cannot
give the plug-in more than you granted it — nor more than the agent may
do.

## Testing

```ts
expect(await host.callMcpTool("lookup-customer", { email: "ada@example.org" })).toMatchObject({ tickets: [] });
```
