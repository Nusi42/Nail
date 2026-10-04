/**
 * The mock core's undo histories (M5d, ADR 0131): `GetEditHistory`,
 * `UndoEdit`, `RedoEdit` and `EditHistoryChanged`, like
 * `nail-config/src/edit_history.rs`.
 *
 * Instead of file texts the mock keeps snapshots of what a scope covers;
 * the feature mocks register how to take and restore one
 * ({@link registerMockHistoryScope}: the customization mock for `layout`,
 * the appearance mock for `appearance`) and record every write with
 * {@link recordMockEdit}. A scope's revision is a counter (`"none"` before
 * the first write). {@link simulateExternalEdit} plays a change on disk:
 * the revision moves and undo / redo answer `conflict` until `force`.
 */
import type {
  EditEntry,
  EditHistory,
  EditKind,
  HistoryScope,
  Request,
  Response,
} from "./generated";
import type { MockCore } from "./mockMail";

/** The requests answered here. */
export const EDIT_HISTORY_REQUESTS = [
  "GetEditHistory",
  "UndoEdit",
  "RedoEdit",
] as const satisfies readonly Request["type"][];

export type EditHistoryRequest = Extract<Request, { type: (typeof EDIT_HISTORY_REQUESTS)[number] }>;

export function isEditHistoryRequest(request: Request): request is EditHistoryRequest {
  return (EDIT_HISTORY_REQUESTS as readonly string[]).includes(request.type);
}

/**
 * What a feature mock's patch function answers for a patch it cannot apply
 * yet (M5d contract stubs); answered as `unsupported` with this Fluent id.
 */
export const MOCK_NOT_READY = "edit-issue-not-ready";

/** `nail_api::editing::MAX_EDIT_HISTORY`. */
export const MAX_EDIT_HISTORY = 50;

/** How a feature mock takes and restores what a scope covers. */
export interface MockHistoryScope {
  snapshot: (core: MockCore) => unknown;
  /** Puts a snapshot back and announces it (`ConfigChanged`, …). */
  restore: (core: MockCore, snapshot: unknown) => void;
}

const scopes = new Map<HistoryScope, MockHistoryScope>();

/** Registers a scope's snapshot functions (once per feature mock module). */
export function registerMockHistoryScope(scope: HistoryScope, impl: MockHistoryScope): void {
  scopes.set(scope, impl);
}

interface Recorded {
  entry: EditEntry;
  before: unknown;
  after: unknown;
}

interface ScopeState {
  revision: number;
  /** A change "on disk" since the last write by the mock. */
  external: boolean;
  undo: Recorded[];
  redo: Recorded[];
}

interface State {
  nextId: number;
  layout: ScopeState;
  appearance: ScopeState;
}

const states = new WeakMap<MockCore, State>();

function stateOf(core: MockCore): State {
  let state = states.get(core);
  if (state === undefined) {
    const scope = (): ScopeState => ({ revision: 0, external: false, undo: [], redo: [] });
    state = { nextId: 0, layout: scope(), appearance: scope() };
    states.set(core, state);
  }
  return state;
}

function revisionOf(scope: ScopeState): string {
  return scope.revision === 0 ? "none" : `mock-${String(scope.revision)}`;
}

/** The scope's history as the core answers it. */
export function mockEditHistory(core: MockCore, scope: HistoryScope): EditHistory {
  const s = stateOf(core)[scope];
  return {
    scope,
    revision: revisionOf(s),
    undo: s.undo.map((r) => structuredClone(r.entry)),
    redo: s.redo.map((r) => structuredClone(r.entry)),
  };
}

/** The scope's current revision (for `base_revision` checks). */
export function mockRevision(core: MockCore, scope: HistoryScope): string {
  return revisionOf(stateOf(core)[scope]);
}

/**
 * `conflict` (`edit-issue-conflict`) when `base` is given and the scope's
 * revision moved since; else `null`.
 */
export function checkMockBaseRevision(
  core: MockCore,
  scope: HistoryScope,
  base: string | null | undefined,
): Response | null {
  if (base === undefined || base === null || base === mockRevision(core, scope)) return null;
  return conflict(scope);
}

function conflict(scope: HistoryScope): Response {
  const file = scope === "layout" ? "layout.kdl" : "theme.kdl";
  return {
    type: "Error",
    code: "conflict",
    message: `${file} changed on disk`,
    localized: { id: "edit-issue-conflict", args: { file } },
  };
}

function emit(core: MockCore, scope: HistoryScope): void {
  core.emit({ type: "EditHistoryChanged", history: mockEditHistory(core, scope) });
}

/**
 * Records a write: `before` is the scope's snapshot taken before the change
 * (the snapshot after it is taken now). Clears the redo list, moves the
 * revision and sends `EditHistoryChanged`.
 */
export function recordMockEdit(
  core: MockCore,
  scope: HistoryScope,
  kind: EditKind,
  before: unknown,
): void {
  const impl = scopes.get(scope);
  if (!impl) return;
  const state = stateOf(core);
  const s = state[scope];
  state.nextId += 1;
  s.undo.push({
    entry: {
      id: `e${String(state.nextId)}`,
      kind,
      label: { id: `edit-kind-${kind}` },
      at: new Date().toISOString(),
      files: [scope === "layout" ? "layout.kdl" : "theme.kdl"],
    },
    before: structuredClone(before),
    after: structuredClone(impl.snapshot(core)),
  });
  if (s.undo.length > MAX_EDIT_HISTORY) s.undo.shift();
  s.redo = [];
  s.revision += 1;
  s.external = false;
  emit(core, scope);
}

/** Plays a change of the scope's file on disk (tests). */
export function simulateExternalEdit(core: MockCore, scope: HistoryScope): void {
  const s = stateOf(core)[scope];
  s.revision += 1;
  s.external = true;
  emit(core, scope);
}

function step(core: MockCore, scope: HistoryScope, undo: boolean, force: boolean): Response {
  const impl = scopes.get(scope);
  const s = stateOf(core)[scope];
  const from = undo ? s.undo : s.redo;
  const recorded = from.at(-1);
  if (!impl || recorded === undefined) {
    const id = undo ? "edit-issue-nothing-to-undo" : "edit-issue-nothing-to-redo";
    return { type: "Error", code: "invalid_request", message: id, localized: { id } };
  }
  if (s.external && !force) return conflict(scope);
  from.pop();
  (undo ? s.redo : s.undo).push(recorded);
  impl.restore(core, structuredClone(undo ? recorded.before : recorded.after));
  s.revision += 1;
  s.external = false;
  emit(core, scope);
  return { type: "EditHistory", history: mockEditHistory(core, scope) };
}

export function answerEditHistory(core: MockCore, request: EditHistoryRequest): Response {
  switch (request.type) {
    case "GetEditHistory":
      return { type: "EditHistory", history: mockEditHistory(core, request.scope) };
    case "UndoEdit":
      return step(core, request.scope, true, request.force === true);
    case "RedoEdit":
      return step(core, request.scope, false, request.force === true);
  }
}
