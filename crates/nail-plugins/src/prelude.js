// The plug-in side of the host (nail-plugins, ADR 0051). Evaluated as
// a script in every plug-in runtime before the plug-in's module. It builds
// the frozen `nail` global, `console` and timers on top of one native
// function, `hostCall(op, json) -> json`, which it keeps to itself. It
// returns `{ load, dispatch }`: the host calls `load(import(main))` to
// start the plug-in (its default export's `activate`) and
// `dispatch(kind, json)` for everything that happens later (resolved API
// calls, commands, dialog results, UI actions, panels, timers, settings,
// deactivation).
(function (hostCall, info) {
  "use strict";

  // Captured before plug-in code runs, so a plug-in that replaces them
  // cannot confuse its own host calls.
  const { parse, stringify } = JSON;
  const pending = new Map(); // call id -> { resolve, reject }
  const commands = new Map(); // name -> handler
  const panels = new Map(); // name -> { render, onAction }
  const dialogActions = new Map(); // call id -> onAction
  const timers = new Map(); // id -> { fn, args }
  const settingsListeners = [];
  let settings = info.settings;
  let plugin = null;
  let nextTimer = 1;

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

  function response(r) {
    const headers = r.headers;
    const body = r.body;
    return Object.freeze({
      status: r.status,
      ok: r.status >= 200 && r.status < 300,
      headers,
      body,
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
    },
    calendar: {
      events: (from, to, options) =>
        request("calendar.events", { from, to, limit: options && options.limit }),
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
        return request("http.fetch", {
          url: String(url),
          method: i.method || "GET",
          headers: i.headers || {},
          body: i.body === undefined || i.body === null ? null : String(i.body),
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

  const handlers = {
    resolve(m) {
      const p = pending.get(m.id);
      if (!p) return;
      pending.delete(m.id);
      dialogActions.delete(m.id);
      if (m.error) p.reject(nailError(m.error));
      else p.resolve(m.value);
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
    handlers[kind](parse(json));
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
