import { stateLabels } from "./server-state";

export function Badge({ state }: { state: import("./api").ServerState }) {
  return (
    <span className={`badge ${state}`}>
      <span />
      {stateLabels[state] || state}
    </span>
  );
}
