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
//
// The JSDoc types document the host protocol and let `tsc` / ESLint check
// this file (ADR 0123). Values that come from plug-in code are `unknown`:
// the runtime checks (`typeof … === "function"`) are real guards.

/**
 * The plug-in facts the host passes in.
 *
 * @typedef {object} PreludeInfo
 * @property {{ id: string, version: string }} plugin
 * @property {string} apiVersion
 * @property {string} locale
 * @property {Record<string, unknown>} settings
 */

/**
 * An error the host reports for a call.
 *
 * @typedef {{ message: string, code?: string }} PreludeHostError
 */

/**
 * The host's answer to `hostCall`: a value, an error, or the id of a call
 * the host resolves later through `dispatch("resolve", …)`.
 *
 * @typedef {{ ok?: unknown, error?: PreludeHostError, pending?: number }} PreludeReply
 */

/**
 * Plug-in code the prelude calls (handlers, listeners, render functions).
 *
 * @typedef {(...args: unknown[]) => unknown} PreludeCallback
 */

/** @typedef {{ resolve: (value: unknown) => void, reject: (reason: unknown) => void }} PreludePending */
/** @typedef {{ render: PreludeCallback, onAction?: unknown }} PreludePanel */
/** @typedef {{ activate?: unknown, deactivate?: unknown }} PreludePlugin */
/** @typedef {{ load: (promise: Promise<unknown>) => void, dispatch: (kind: string, json: string) => void }} PreludeExports */

/**
 * @typedef {object} PreludeHttpReply
 * @property {number} status
 * @property {Record<string, string>} headers
 * @property {string} body
 */

// The script's completion value is the function below; the host calls it
// with the native functions, so the expression is used.
// eslint-disable-next-line @typescript-eslint/no-unused-expressions -- QuickJS evaluates this classic script and the host takes its completion value (the function)
/** @type {(hostCall: (op: string, json: string) => string, info: PreludeInfo, hostPutBytes: (bytes: Uint8Array) => number, hostTakeBytes: (id: number) => Uint8Array) => PreludeExports} */ (
  function (hostCall, info, hostPutBytes, hostTakeBytes) {
    "use strict";

    // Captured before plug-in code runs, so a plug-in that replaces them
    // cannot confuse its own host calls.
    const nativeParse = JSON.parse.bind(JSON);
    const nativeStringify = JSON.stringify.bind(JSON);
    const isView = ArrayBuffer.isView.bind(ArrayBuffer);
    const NativeArrayBuffer = ArrayBuffer;
    const NativeUint8Array = Uint8Array;
    /** @type {Map<number, PreludePending>} call id -> { resolve, reject } */
    const pending = new Map();
    /** @type {Map<string, PreludeCallback>} name -> handler */
    const commands = new Map();
    /** @type {Map<string, PreludePanel>} name -> { render, onAction } */
    const panels = new Map();
    /** @type {Map<number, PreludeCallback>} call id -> onAction */
    const dialogActions = new Map();
    /** @type {Map<number, { fn: PreludeCallback, args: unknown[] }>} id -> { fn, args } */
    const timers = new Map();
    /** @type {Map<string, PreludeCallback>} "<registry>:<name>" -> handler (M4a) */
    const handlers = new Map();
    /** @type {PreludeCallback[]} */
    const settingsListeners = [];
    let settings = info.settings;
    /** @type {PreludePlugin | null} */
    let plugin = null;
    let nextTimer = 1;

    /**
     * @param {string} text
     * @returns {unknown}
     */
    function parse(text) {
      return /** @type {unknown} */ (nativeParse(text));
    }

    /**
     * `JSON.stringify`, which yields `undefined` for functions and
     * `undefined` itself.
     *
     * @param {unknown} value
     * @returns {string | undefined}
     */
    function stringify(value) {
      return nativeStringify(value);
    }

    /**
     * @param {unknown} value
     * @returns {value is PreludeCallback}
     */
    function isCallable(value) {
      return typeof value === "function";
    }

    /**
     * @param {unknown} value
     * @returns {value is ArrayBuffer | ArrayBufferView}
     */
    function isBytes(value) {
      return value instanceof NativeArrayBuffer || isView(value);
    }

    /**
     * @param {ArrayBuffer | ArrayBufferView} value
     * @returns {Uint8Array}
     */
    function toUint8(value) {
      if (value instanceof NativeUint8Array) return value;
      if (value instanceof NativeArrayBuffer) return new NativeUint8Array(value);
      return new NativeUint8Array(value.buffer, value.byteOffset, value.byteLength);
    }

    /**
     * Replaces typed arrays / ArrayBuffers in a value with byte markers the
     * host resolves (they cannot travel as JSON).
     *
     * @param {unknown} value
     * @param {number} [depth]
     * @returns {unknown}
     */
    function encodeBytes(value, depth) {
      const d = depth ?? 0;
      if (isBytes(value)) return { $nailBytes: hostPutBytes(toUint8(value)) };
      if (d > 32 || value === null || typeof value !== "object") return value;
      if (Array.isArray(value)) {
        return value.map((/** @type {unknown} */ v) => encodeBytes(v, d + 1));
      }
      const object = /** @type {Record<string, unknown>} */ (value);
      /** @type {Record<string, unknown>} */
      const out = {};
      for (const key of Object.keys(object)) out[key] = encodeBytes(object[key], d + 1);
      return out;
    }

    /**
     * @param {string} op
     * @param {unknown} payload
     * @returns {PreludeReply}
     */
    function call(op, payload) {
      const json = stringify(payload === undefined ? null : payload);
      if (json === undefined) throw new TypeError("the value cannot be sent as JSON");
      return /** @type {PreludeReply} */ (parse(hostCall(op, json)));
    }

    /**
     * @param {PreludeHostError} e
     * @returns {Error}
     */
    function nailError(e) {
      const err = /** @type {Error & { code?: string }} */ (new Error(e.message));
      err.name = "NailError";
      err.code = e.code;
      return err;
    }

    /**
     * @param {string} op
     * @param {unknown} payload
     * @returns {unknown}
     */
    function sync(op, payload) {
      const r = call(op, payload);
      if (r.error) throw nailError(r.error);
      return r.ok;
    }

    /**
     * A host call as a promise: a throw (also from `call`) rejects it with
     * the thrown value, a pending call settles when the host resolves it.
     *
     * @param {string} op
     * @param {unknown} payload
     * @returns {Promise<unknown>}
     */
    function request(op, payload) {
      return new Promise((resolve, reject) => {
        const r = call(op, payload);
        if (r.error) {
          reject(nailError(r.error));
        } else if (r.pending !== undefined) {
          pending.set(r.pending, { resolve, reject });
        } else {
          resolve(r.ok);
        }
      });
    }

    /**
     * Registers a handler declared in the manifest (hooks, composer and
     * attachment extensions, link actions, jobs, MCP tools).
     *
     * @param {string} registry
     * @param {string} name
     * @param {unknown} handler
     */
    function register(registry, name, handler) {
      if (!isCallable(handler)) throw new TypeError("a handler must be a function");
      sync("register", { registry, name });
      handlers.set(registry + ":" + name, handler);
    }

    /**
     * @param {unknown} value
     * @returns {unknown[]}
     */
    function ids(value) {
      return Array.isArray(value) ? value : [value];
    }

    /**
     * @param {string} op
     * @param {unknown} text
     * @param {{ format?: unknown, draftId?: unknown } | undefined} options
     * @returns {Promise<unknown>}
     */
    function composerEdit(op, text, options) {
      const o = options ?? {};
      return request("composer.edit", {
        op,
        text: String(text),
        format: o.format,
        draftId: o.draftId,
      });
    }

    /**
     * @param {unknown} value
     * @returns {string}
     */
    function describe(value) {
      if (typeof value === "string") return value;
      if (value instanceof Error) {
        const head = value.name + ": " + value.message;
        return value.stack ? head + "\n" + value.stack : head;
      }
      try {
        return stringify(value) ?? String(value);
      } catch {
        return String(value);
      }
    }

    /**
     * The `code` of a thrown value, if it has one.
     *
     * @param {unknown} e
     * @returns {string | undefined}
     */
    function errorCode(e) {
      if (e === null || (typeof e !== "object" && typeof e !== "function")) return undefined;
      const code = /** @type {{ code?: unknown }} */ (e).code;
      if (!code) return undefined;
      return typeof code === "string" ? code : describe(code);
    }

    /**
     * @param {string} where
     * @param {unknown} error
     */
    function fatal(where, error) {
      const json = stringify({ where, message: describe(error) }) ?? "null";
      hostCall("fatal", json);
    }

    /**
     * Runs plug-in code: synchronous throws and rejected promises are
     * uncaught errors that stop the plug-in; `then` gets the result.
     *
     * @param {string} where
     * @param {PreludeCallback} fn
     * @param {unknown[]} args
     * @param {(value: unknown) => void} [then]
     */
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
        (/** @type {unknown} */ e) => {
          fatal(where, e);
        },
      );
    }

    /**
     * @param {string} level
     * @param {unknown[]} args
     */
    function log(level, args) {
      sync("log", { level, message: args.map((arg) => describe(arg)).join(" ") });
    }

    /**
     * @template {object} T
     * @param {T} obj
     * @returns {Readonly<T>}
     */
    function freeze(obj) {
      const record = /** @type {Record<string, unknown>} */ (obj);
      for (const key of Object.keys(record)) {
        const value = record[key];
        if (value && typeof value === "object") freeze(value);
      }
      return Object.freeze(obj);
    }

    /**
     * @param {string} name
     * @param {unknown} handler
     */
    function registerCommand(name, handler) {
      if (!isCallable(handler)) throw new TypeError("a command handler must be a function");
      sync("registerCommand", { name });
      commands.set(name, handler);
    }

    /**
     * @param {string} name
     * @param {unknown} panel
     */
    function registerPanel(name, panel) {
      const def = /** @type {{ render?: unknown, onAction?: unknown } | null | undefined} */ (
        isCallable(panel) ? { render: panel } : panel
      );
      if (!def || !isCallable(def.render)) {
        throw new TypeError("a panel needs a render function");
      }
      sync("registerPanel", { name });
      // The plug-in's own object: `render` / `onAction` are called as its
      // methods.
      panels.set(name, /** @type {PreludePanel} */ (def));
    }

    /**
     * @param {unknown} reply
     */
    function response(reply) {
      const answer = /** @type {{ value: PreludeHttpReply, bytes?: Uint8Array }} */ (reply);
      const r = answer.value;
      const raw = answer.bytes ?? new NativeUint8Array(0);
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
        json: () =>
          new Promise((resolve) => {
            resolve(parse(body));
          }),
      });
    }

    const settingsApi = {
      get: (/** @type {string} */ key) => settings[key],
      all: () => ({ ...settings }),
      onChange: (/** @type {unknown} */ listener) => {
        if (!isCallable(listener)) throw new TypeError("a listener must be a function");
        settingsListeners.push(listener);
      },
    };

    const logApi = {
      debug: (/** @type {unknown[]} */ ...args) => {
        log("debug", args);
      },
      info: (/** @type {unknown[]} */ ...args) => {
        log("info", args);
      },
      warn: (/** @type {unknown[]} */ ...args) => {
        log("warn", args);
      },
      error: (/** @type {unknown[]} */ ...args) => {
        log("error", args);
      },
    };

    /** @typedef {{ limit?: unknown } | null | undefined} PreludeLimit */

    /**
     * The `limit` of a plug-in's options bag.
     *
     * @param {PreludeLimit} options
     * @returns {unknown}
     */
    function limitOf(options) {
      return options ? options.limit : options;
    }

    /**
     * @param {string} registry
     * @returns {(id: string, handler: unknown) => void}
     */
    function registrar(registry) {
      return (id, handler) => {
        register(registry, id, handler);
      };
    }

    const nail = {
      plugin: { id: info.plugin.id, version: info.plugin.version },
      apiVersion: info.apiVersion,
      mail: {
        get: (/** @type {unknown} */ id) => request("mail.get", { id }),
        getBody: (/** @type {unknown} */ id) => request("mail.getBody", { id }),
        search: (/** @type {unknown} */ query, /** @type {PreludeLimit} */ options) =>
          request("mail.search", { query, limit: limitOf(options) }),
        thread: (/** @type {unknown} */ threadId) => request("mail.thread", { threadId }),
        // M4a writes (`mail.organize`, `mail.send`); each resolves with
        // `{ status: "done" | "denied" | "expired" | "vetoed", ... }`.
        update: (/** @type {unknown} */ messageIds, /** @type {unknown} */ changes) =>
          request("mail.update", { ids: ids(messageIds), options: changes }),
        delete: (/** @type {unknown} */ messageIds, /** @type {unknown} */ options) =>
          request("mail.delete", { ids: ids(messageIds), options }),
        createDraft: (/** @type {unknown} */ draft) =>
          request("mail.createDraft", { options: encodeBytes(draft) }),
        updateDraft: (/** @type {unknown} */ draftId, /** @type {unknown} */ changes) =>
          request("mail.updateDraft", { draftId, options: encodeBytes(changes) }),
        send: (/** @type {unknown} */ draftId, /** @type {unknown} */ options) =>
          request("mail.send", { draftId, options }),
      },
      calendar: {
        events: (
          /** @type {unknown} */ from,
          /** @type {unknown} */ to,
          /** @type {PreludeLimit} */ options,
        ) => request("calendar.events", { from, to, limit: limitOf(options) }),
        calendars: () => request("calendar.calendars", {}),
        createEvent: (/** @type {unknown} */ event) =>
          request("calendar.createEvent", { options: event }),
        updateEvent: (/** @type {unknown} */ eventId, /** @type {unknown} */ changes) =>
          request("calendar.updateEvent", { eventId, options: changes }),
        deleteEvent: (/** @type {unknown} */ eventId, /** @type {unknown} */ options) =>
          request("calendar.deleteEvent", { eventId, options }),
        respond: (/** @type {unknown} */ answer) =>
          request("calendar.respond", { options: answer }),
      },
      contacts: {
        addressBooks: () => request("contacts.addressBooks", {}),
        find: (/** @type {unknown} */ email, /** @type {PreludeLimit} */ options) =>
          request("contacts.find", { email, limit: limitOf(options) }),
        upsert: (/** @type {unknown} */ contact) =>
          request("contacts.upsert", { options: contact }),
        delete: (/** @type {unknown} */ contactId) => request("contacts.delete", { contactId }),
      },
      tasks: {
        lists: () => request("tasks.lists", {}),
        create: (/** @type {unknown} */ task) => request("tasks.create", { options: task }),
        update: (/** @type {unknown} */ taskId, /** @type {unknown} */ changes) =>
          request("tasks.update", { taskId, options: changes }),
        delete: (/** @type {unknown} */ taskId) => request("tasks.delete", { taskId }),
      },
      hooks: {
        register: registrar("hook"),
        onBannerAction: (/** @type {unknown} */ handler) => {
          register("bannerAction", "default", handler);
        },
      },
      composer: {
        registerAction: registrar("composerAction"),
        registerBlock: registrar("markdownBlock"),
        insert: (
          /** @type {unknown} */ text,
          /** @type {{ format?: unknown, draftId?: unknown } | undefined} */ options,
        ) => composerEdit("insert", text, options),
        replaceSelection: (
          /** @type {unknown} */ text,
          /** @type {{ format?: unknown, draftId?: unknown } | undefined} */ options,
        ) => composerEdit("replaceSelection", text, options),
        replaceAll: (
          /** @type {unknown} */ text,
          /** @type {{ format?: unknown, draftId?: unknown } | undefined} */ options,
        ) => composerEdit("replaceAll", text, options),
      },
      attachments: {
        registerPreview: registrar("attachmentPreview"),
        registerAction: registrar("attachmentAction"),
      },
      links: {
        registerAction: registrar("linkAction"),
      },
      jobs: {
        register: registrar("job"),
      },
      mcp: {
        registerTool: registrar("mcpTool"),
      },
      l10n: {
        locale: info.locale,
        t: (/** @type {unknown} */ id, /** @type {unknown} */ args) =>
          sync("l10n.t", { id: String(id), args: args ?? {} }),
        has: (/** @type {unknown} */ id) => sync("l10n.has", { id: String(id) }),
      },
      ui: {
        registerCommand,
        registerPanel,
        showDialog: (
          /** @type {unknown} */ spec,
          /** @type {{ onAction?: unknown } | null | undefined} */ options,
        ) =>
          new Promise((resolve, reject) => {
            const r = call("showDialog", spec);
            if (r.error) {
              reject(nailError(r.error));
              return;
            }
            const id = /** @type {number} */ (r.pending);
            const onAction = options ? options.onAction : undefined;
            if (isCallable(onAction)) dialogActions.set(id, onAction);
            pending.set(id, { resolve, reject });
          }),
        notify: (/** @type {unknown} */ spec) =>
          request("notify", typeof spec === "string" ? { title: spec } : spec),
        updatePanel: (/** @type {unknown} */ name, /** @type {unknown} */ tree) =>
          request("updatePanel", { name, tree }),
      },
      commands: { register: registerCommand },
      storage: {
        get: (/** @type {unknown} */ key) => request("storage.get", { key }),
        set: (/** @type {unknown} */ key, /** @type {unknown} */ value) =>
          request("storage.set", { key, value }),
        delete: (/** @type {unknown} */ key) => request("storage.delete", { key }),
        keys: (/** @type {unknown} */ prefix) => request("storage.keys", { prefix }),
        clear: () => request("storage.clear", {}),
        usage: () => request("storage.usage", {}),
      },
      http: {
        fetch: (
          /** @type {unknown} */ url,
          /** @type {{ method?: unknown, headers?: unknown, body?: unknown } | null | undefined} */ init,
        ) =>
          new Promise((resolve) => {
            const i = init ?? {};
            /** @type {unknown} */
            let body = null;
            if (i.body !== undefined && i.body !== null) {
              body = isBytes(i.body)
                ? encodeBytes(i.body)
                : // eslint-disable-next-line @typescript-eslint/no-base-to-string -- any other body is sent as its string form, like `fetch` does with `toString()`
                  String(i.body);
            }
            resolve(
              request("http.fetch", {
                url: String(url),
                method: i.method ?? "GET",
                headers: i.headers ?? {},
                body,
              }),
            );
          }).then(response),
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

    /**
     * @param {unknown} fn
     * @param {unknown} ms
     * @param {unknown[]} args
     * @returns {number}
     */
    function setTimeout(fn, ms, ...args) {
      if (!isCallable(fn)) throw new TypeError("setTimeout needs a function");
      const id = nextTimer++;
      sync("setTimer", { id, ms: Math.max(0, Number(ms) || 0) });
      timers.set(id, { fn, args });
      return id;
    }

    /**
     * @param {number} id
     */
    function clearTimeout(id) {
      if (timers.delete(id)) sync("clearTimer", { id });
    }

    /**
     * @param {unknown} fn
     */
    function queueMicrotask(fn) {
      void Promise.resolve().then(() => {
        run("queueMicrotask", /** @type {PreludeCallback} */ (fn), []);
      });
    }

    /** @type {[string, unknown][]} */
    const globals = [
      ["nail", nail],
      ["console", consoleApi],
      ["setTimeout", setTimeout],
      ["clearTimeout", clearTimeout],
      ["queueMicrotask", queueMicrotask],
    ];
    for (const [name, value] of globals) {
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

    /**
     * @param {unknown} namespace
     */
    function start(namespace) {
      const exported = /** @type {{ default?: unknown }} */ (namespace).default;
      /** @type {unknown} */
      let def = exported !== undefined ? exported : namespace;
      if (isCallable(def)) def = { activate: def };
      // Anything but an object (`null`, a number) has no `activate`.
      /** @type {PreludePlugin} */
      const current = def !== null && typeof def === "object" ? def : {};
      plugin = current;
      const activate = current.activate;
      if (!isCallable(activate)) {
        sync("activated", {});
        return;
      }
      run("activate", (c) => activate.call(current, c), [ctx], () => sync("activated", {}));
    }

    /**
     * @typedef {object} PreludeMessage
     * @property {number} id
     * @property {number} [bytes]
     * @property {PreludeHostError} [error]
     * @property {unknown} value
     * @property {number} req
     * @property {string} registry
     * @property {string} name
     * @property {unknown} input
     * @property {unknown} context
     * @property {string} componentId
     * @property {unknown} values
     */

    /** @type {Record<string, (m: PreludeMessage) => void>} */
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
        /** @param {unknown} e */
        const fail = (e) => {
          try {
            sync("invokeResult", {
              req: m.req,
              error: { message: describe(e), code: errorCode(e) },
            });
          } catch {
            // The host stopped waiting.
          }
        };
        /** @param {unknown} value */
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
        run(
          "panel " + m.name,
          (c) => panel.render(c),
          [m.context],
          (tree) => sync("panelResult", { req: m.req, tree: tree === undefined ? [] : tree }),
        );
      },
      panelAction(m) {
        const panel = panels.get(m.name);
        if (!panel) return;
        const onAction = panel.onAction;
        if (!isCallable(onAction)) return;
        run("panel action " + m.name, (a) => onAction.call(panel, a), [
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
        settings = /** @type {Record<string, unknown>} */ (m.values);
        for (const listener of settingsListeners) run("settings listener", listener, [settings]);
      },
      deactivate() {
        if (plugin && isCallable(plugin.deactivate)) {
          try {
            plugin.deactivate();
          } catch {
            // Stopping anyway.
          }
        }
      },
    };

    /**
     * @param {string} kind
     * @param {string} json
     */
    function dispatch(kind, json) {
      const dispatcher = dispatchers[kind];
      if (dispatcher === undefined) throw new TypeError("unknown dispatch kind " + kind);
      dispatcher(/** @type {PreludeMessage} */ (parse(json)));
    }

    /**
     * @param {Promise<unknown>} promise
     */
    function load(promise) {
      promise.then(
        (namespace) => {
          try {
            start(namespace);
          } catch (e) {
            fatal("activate", e);
          }
        },
        (/** @type {unknown} */ e) => {
          fatal("load", e);
        },
      );
    }

    return { load, dispatch };
  }
);
