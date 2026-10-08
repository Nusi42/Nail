/** Test components around the action dispatcher. */
import { useActions } from "../actions/ActionsProvider";

/** A button that dispatches the action `id` (named "run <id>"). */
export function RunAction({ id }: { id: string }) {
  const actions = useActions();
  return (
    <button type="button" onClick={() => void actions.dispatch(id)}>
      {`run ${id}`}
    </button>
  );
}
