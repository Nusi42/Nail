// The plug-in side of the host (nail-plugins, ADR 0051; M4a ADR 0079 /
// WP102). Evaluated as a script in every plug-in runtime before the
// plug-in's module. It builds the frozen `nail` global, `console` and
// timers on top of three native functions it keeps to itself:
// `hostCall(op, json) -> json`, `hostPutBytes(u8) -> id` (bytes the
// plug-in hands over, referenced as `{"$nailBytes": id}` in the JSON) and
// `hostTakeBytes(id) -> Uint8Array` (bytes for the plug-in). It returns
// `{ load, dispatch }`: the host calls `load(import(main))` to start the
// plug-in (its default export's `activate`) and `dispatch(kind, json)` for
// everything that happens later (resolved API calls, commands, dialog
// results, UI actions, panels, timers, settings, handler calls,
// deactivation).
(function (hostCall, info, hostPutBytes, hostTakeBytes) {
  "use strict";

  // Captured before plug-in code runs, so a plug-in that replaces them
  // cannot confuse its own host calls.
  const { parse, stringify } = JSON;
  const { isView } = ArrayBuffer;
  const NativeArrayBuffer = ArrayBuffer;
  const NativeUint8Array = Uint8Array;
  const pending = new Map(); // call id -> { resolve, reject }
  const commands = new Map(); // name -> handler
  const panels = new Map(); // name -> { render, onAction }
  const dialogActions = new Map(); // call id -> onAction
  const timers = new Map(); // id -> { fn, args }
  const handlers = new Map(); // "<registry>:<name>" -> handler (M4a)
  const settingsListeners = [];
  let settings = info.settings;
  let plugin = null;
  let nextTimer = 1;

  function isBytes(value) {
    return value instanceof NativeArrayBuffer || isView(value);
  }

  function toUint8(value) {
    if (value instanceof NativeUint8Array) return value;
    if (value instanceof NativeArrayBuffer) return new NativeUint8Array(value);
    return new NativeUint8Array(value.buffer, value.byteOffset, value.byteLength);
  }

  // Replaces typed arrays / ArrayBuffers in a value with byte markers the
  // host resolves (they cannot travel as JSON).
  function encodeBytes(value, depth) {
    const d = depth || 0;
    if (isBytes(value)) return { $nailBytes: hostPutBytes(toUint8(value)) };
    if (d > 32 || value === null || typeof value !== "object") return value;
    if (Array.isArray(value)) return value.map((v) => encodeBytes(v, d + 1));
    const out = {};
    for (const key of Object.keys(value)) out[key] = encodeBytes(value[key], d + 1);
    return out;
  }

  function call(op, payload) {
    const text = hostCall(op, stringify(payload === undefined ? null : payload));
    return parse(text);
  }

  function nailError(e) {
    const err = new Error(e.message);
    err.name = "NailError";
    err.code = e.code;
    return err;
  }

  function sync(op, payload) {
    const r = call(op, payload);
    if (r.error) throw nailError(r.error);
    return r.ok;
  }

  function request(op, payload) {
    let r;
    try {
      r = call(op, payload);
    } catch (e) {
      return Promise.reject(e);
    }
    if (r.error) return Promise.reject(nailError(r.error));
    if (r.pending !== undefined) {
      return new Promise((resolve, reject) => pending.set(r.pending, { resolve, reject }));
    }
    return Promise.resolve(r.ok);
  }

  // Registers a handler declared in the manifest (hooks, composer and
  // attachment extensions, link actions, jobs, MCP tools).
  function register(registry, name, handler) {
    if (typeof handler !== "function") throw new TypeError("a handler must be a function");
    sync("register", { registry, name });
    handlers.set(registry + ":" + name, handler);
  }

  function ids(value) {
    return Array.isArray(value) ? value : [value];
  }

  function composerEdit(op, text, options) {
    const o = options || {};
    return request("composer.edit", {
      op,
      text: String(text),
      format: o.format,
      draftId: o.draftId,
    });
  }

  function describe(value) {
    if (typeof value === "string") return value;
    if (value instanceof Error) {
      const head = value.name + ": " + value.message;
      return value.stack ? head + "\n" + value.stack : head;
    }
    try {
      const json = stringify(value);
      return json === undefined ? String(value) : json;
    } catch (_) {
      return String(value);
    }
  }

  function fatal(where, error) {
    hostCall("fatal", stringify({ where, message: describe(error) }));
  }

  // Runs plug-in code: synchronous throws and rejected promises are
  // uncaught errors that stop the plug-in; `then` gets the result.
  function run(where, fn, args, then) {
    let result;
    try {
      result = fn.apply(undefined, args);
    } catch (e) {
      fatal(where, e);
      return;
    }
    Promise.resolve(result).then(
      (value) => {
        if (then) then(value);
      },
      (e) => fatal(where, e),
    );
  }

  function log(level, args) {
    sync("log", { level, message: Array.prototype.map.call(args, describe).join(" ") });
  }

  function freeze(obj) {
    for (const key of Object.keys(obj)) {
      const value = obj[key];
      if (value && typeof value === "object") freeze(value);
    }
    return Object.freeze(obj);
  }

  function registerCommand(name, handler) {
    if (typeof handler !== "function") throw new TypeError("a command handler must be a function");
    sync("registerCommand", { name });
    commands.set(name, handler);
  }

  function registerPanel(name, panel) {
    const def = typeof panel === "function" ? { render: panel } : panel;
    if (!def || typeof def.render !== "function") {
      throw new TypeError("a panel needs a render function");
    }
    sync("registerPanel", { name });
    panels.set(name, def);
  }

  function response(answer) {
    const r = answer.value;
    const raw = answer.bytes || new NativeUint8Array(0);
    const headers = r.headers;
    const body = r.body;
    return Object.freeze({
      status: r.status,
      ok: r.status >= 200 && r.status < 300,
      headers,
      body,
      bytes: () => Promise.resolve(raw.slice()),
      arrayBuffer: () => Promise.resolve(raw.slice().buffer),
      text: () => Promise.resolve(body),
      json: () => {
        try {
          return Promise.resolve(parse(body));
        } catch (e) {
          return Promise.reject(e);
        }
      },
    });
  }

  const settingsApi = {
    get: (key) => settings[key],
    all: () => Object.assign({}, settings),
    onChange: (listener) => {
      if (typeof listener !== "function") throw new TypeError("a listener must be a function");
      settingsListeners.push(listener);
    },
  };

  const logApi = {
    debug: (...args) => log("debug", args),
    info: (...args) => log("info", args),
    warn: (...args) => log("warn", args),
    error: (...args) => log("error", args),
  };

  const nail = {
    plugin: { id: info.plugin.id, version: info.plugin.version },
    apiVersion: info.apiVersion,
    mail: {
      get: (id) => request("mail.get", { id }),
      getBody: (id) => request("mail.getBody", { id }),
      search: (query, options) =>
        request("mail.search", { query, limit: options && options.limit }),
      thread: (threadId) => request("mail.thread", { threadId }),
      // M4a writes (`mail.organize`, `mail.send`); each resolves with
      // `{ status: "done" | "denied" | "expired" | "vetoed", ... }`.
      update: (messageIds, changes) =>
        request("mail.update", { ids: ids(messageIds), options: changes }),
      delete: (messageIds, options) =>
        request("mail.delete", { ids: ids(messageIds), options }),
      createDraft: (draft) => request("mail.createDraft", { options: encodeBytes(draft) }),
      updateDraft: (draftId, changes) =>
        request("mail.updateDraft", { draftId, options: encodeBytes(changes) }),
      send: (draftId, options) => request("mail.send", { draftId, options }),
    },
    calendar: {
      events: (from, to, options) =>
        request("calendar.events", { from, to, limit: options && options.limit }),
      calendars: () => request("calendar.calendars", {}),
      createEvent: (event) => request("calendar.createEvent", { options: event }),
      updateEvent: (eventId, changes) =>
        request("calendar.updateEvent", { eventId, options: changes }),
      deleteEvent: (eventId, options) => request("calendar.deleteEvent", { eventId, options }),
      respond: (answer) => request("calendar.respond", { options: answer }),
    },
    contacts: {
      addressBooks: () => request("contacts.addressBooks", {}),
      find: (email, options) =>
        request("contacts.find", { email, limit: options && options.limit }),
      upsert: (contact) => request("contacts.upsert", { options: contact }),
      delete: (contactId) => request("contacts.delete", { contactId }),
    },
    tasks: {
      lists: () => request("tasks.lists", {}),
      create: (task) => request("tasks.create", { options: task }),
      update: (taskId, changes) => request("tasks.update", { taskId, options: changes }),
      delete: (taskId) => request("tasks.delete", { taskId }),
    },
    hooks: {
      register: (hook, handler) => register("hook", hook, handler),
      onBannerAction: (handler) => register("bannerAction", "default", handler),
    },
    composer: {
      registerAction: (id, handler) => register("composerAction", id, handler),
      registerBlock: (id, render) => register("markdownBlock", id, render),
      insert: (text, options) => composerEdit("insert", text, options),
      replaceSelection: (text, options) => composerEdit("replaceSelection", text, options),
      replaceAll: (text, options) => composerEdit("replaceAll", text, options),
    },
    attachments: {
      registerPreview: (id, render) => register("attachmentPreview", id, render),
      registerAction: (id, handler) => register("attachmentAction", id, handler),
    },
    links: {
      registerAction: (id, handler) => register("linkAction", id, handler),
    },
    jobs: {
      register: (id, handler) => register("job", id, handler),
    },
    mcp: {
      registerTool: (id, handler) => register("mcpTool", id, handler),
    },
    l10n: {
      locale: info.locale,
      t: (id, args) => sync("l10n.t", { id: String(id), args: args || {} }),
      has: (id) => sync("l10n.has", { id: String(id) }),
    },
    ui: {
      registerCommand,
      registerPanel,
      showDialog: (spec, options) => {
        let r;
        try {
          r = call("showDialog", spec);
        } catch (e) {
          return Promise.reject(e);
        }
        if (r.error) return Promise.reject(nailError(r.error));
        if (options && typeof options.onAction === "function") {
          dialogActions.set(r.pending, options.onAction);
        }
        return new Promise((resolve, reject) => pending.set(r.pending, { resolve, reject }));
      },
      notify: (spec) => request("notify", typeof spec === "string" ? { title: spec } : spec),
      updatePanel: (name, tree) => request("updatePanel", { name, tree }),
    },
    commands: { register: registerCommand },
    storage: {
      get: (key) => request("storage.get", { key }),
      set: (key, value) => request("storage.set", { key, value }),
      delete: (key) => request("storage.delete", { key }),
      keys: (prefix) => request("storage.keys", { prefix }),
      clear: () => request("storage.clear", {}),
      usage: () => request("storage.usage", {}),
    },
    http: {
      fetch: (url, init) => {
        const i = init || {};
        let body = null;
        try {
          if (i.body !== undefined && i.body !== null) {
            body = isBytes(i.body) ? encodeBytes(i.body) : String(i.body);
          }
        } catch (e) {
          return Promise.reject(e);
        }
        return request("http.fetch", {
          url: String(url),
          method: i.method || "GET",
          headers: i.headers || {},
          body,
        }).then(response);
      },
    },
    settings: settingsApi,
    log: logApi,
  };
  freeze(nail);

  const consoleApi = freeze({
    log: logApi.info,
    info: logApi.info,
    debug: logApi.debug,
    warn: logApi.warn,
    error: logApi.error,
    trace: logApi.debug,
  });

  function setTimeout(fn, ms, ...args) {
    if (typeof fn !== "function") throw new TypeError("setTimeout needs a function");
    const id = nextTimer++;
    sync("setTimer", { id, ms: Math.max(0, Number(ms) || 0) });
    timers.set(id, { fn, args });
    return id;
  }

  function clearTimeout(id) {
    if (timers.delete(id)) sync("clearTimer", { id });
  }

  function queueMicrotask(fn) {
    Promise.resolve().then(() => run("queueMicrotask", fn, []));
  }

  for (const [name, value] of [
    ["nail", nail],
    ["console", consoleApi],
    ["setTimeout", setTimeout],
    ["clearTimeout", clearTimeout],
    ["queueMicrotask", queueMicrotask],
  ]) {
    Object.defineProperty(globalThis, name, { value, writable: false, configurable: false });
  }

  const ctx = freeze({
    plugin: nail.plugin,
    commands: { register: registerCommand },
    panels: { register: registerPanel },
    settings: settingsApi,
    hooks: {
      register: nail.hooks.register,
      onBannerAction: nail.hooks.onBannerAction,
    },
    composer: {
      registerAction: nail.composer.registerAction,
      registerBlock: nail.composer.registerBlock,
    },
    attachments: {
      registerPreview: nail.attachments.registerPreview,
      registerAction: nail.attachments.registerAction,
    },
    links: { registerAction: nail.links.registerAction },
    jobs: { register: nail.jobs.register },
    mcp: { registerTool: nail.mcp.registerTool },
    l10n: nail.l10n,
  });

  function start(namespace) {
    let def = namespace.default !== undefined ? namespace.default : namespace;
    if (typeof def === "function") def = { activate: def };
    plugin = def || {};
    const activate = plugin.activate;
    if (typeof activate !== "function") {
      sync("activated", {});
      return;
    }
    run("activate", (c) => activate.call(plugin, c), [ctx], () => sync("activated", {}));
  }

  const dispatchers = {
    resolve(m) {
      const bytes = m.bytes === undefined ? undefined : hostTakeBytes(m.bytes);
      const p = pending.get(m.id);
      if (!p) return;
      pending.delete(m.id);
      dialogActions.delete(m.id);
      if (m.error) p.reject(nailError(m.error));
      else if (bytes !== undefined) p.resolve({ value: m.value, bytes });
      else p.resolve(m.value);
    },
    // A registered handler is called (M4a): its (awaited) result or error
    // goes back to the host. A throw never stops the plug-in.
    invoke(m) {
      const bytes = m.bytes === undefined ? undefined : hostTakeBytes(m.bytes);
      const handler = handlers.get(m.registry + ":" + m.name);
      const fail = (e) => {
        try {
          sync("invokeResult", {
            req: m.req,
            error: { message: describe(e), code: e && e.code ? String(e.code) : undefined },
          });
        } catch (_) {
          // The host stopped waiting.
        }
      };
      const finish = (value) => {
        try {
          sync("invokeResult", {
            req: m.req,
            value: encodeBytes(value === undefined ? null : value),
          });
        } catch (e) {
          fail(e);
        }
      };
      if (!handler) {
        fail(new Error("no handler"));
        return;
      }
      let result;
      try {
        result = bytes === undefined ? handler(m.input) : handler(m.input, bytes);
      } catch (e) {
        fail(e);
        return;
      }
      Promise.resolve(result).then(finish, fail);
    },
    command(m) {
      const handler = commands.get(m.name);
      if (!handler) return;
      run("command " + m.name, handler, [m.context]);
    },
    panel(m) {
      const panel = panels.get(m.name);
      if (!panel) return;
      run("panel " + m.name, (c) => panel.render(c), [m.context], (tree) =>
        sync("panelResult", { req: m.req, tree: tree === undefined ? [] : tree }),
      );
    },
    panelAction(m) {
      const panel = panels.get(m.name);
      if (!panel || typeof panel.onAction !== "function") return;
      run("panel action " + m.name, (a) => panel.onAction(a), [
        { componentId: m.componentId, values: m.values },
      ]);
    },
    dialogAction(m) {
      const onAction = dialogActions.get(m.id);
      if (!onAction) return;
      run("dialog action", onAction, [{ componentId: m.componentId, values: m.values }]);
    },
    timer(m) {
      const timer = timers.get(m.id);
      if (!timer) return;
      timers.delete(m.id);
      run("timer", timer.fn, timer.args);
    },
    settings(m) {
      settings = m.values;
      for (const listener of settingsListeners) run("settings listener", listener, [settings]);
    },
    deactivate() {
      if (plugin && typeof plugin.deactivate === "function") {
        try {
          plugin.deactivate();
        } catch (_) {
          // Stopping anyway.
        }
      }
    },
  };

  function dispatch(kind, json) {
    dispatchers[kind](parse(json));
  }

  function load(promise) {
    promise.then(
      (namespace) => {
        try {
          start(namespace);
        } catch (e) {
          fatal("activate", e);
        }
      },
      (e) => fatal("load", e),
    );
  }

  return { load, dispatch };
});
