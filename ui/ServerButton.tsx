import { Download, LoaderCircle, Power, Square } from "lucide-react";
import { type Server } from "./api";
import { stateLabels } from "./server-state";

export function ServerButton({
  server,
  pending,
  onAction,
}: {
  server: Server;
  pending: boolean;
  onAction: (action: "start" | "stop" | "install") => void;
}) {
  const busy = [
    "starting",
    "installing",
    "stopping",
    "backing_up",
    "restoring",
    "managing_plugins",
    "exporting_modpack",
  ].includes(server.state);
  if (busy)
    return (
      <button className="button secondary" disabled>
        <LoaderCircle size={16} className="spin" />
        {stateLabels[server.state]}
      </button>
    );
  const type = ["online", "restart_waiting"].includes(server.state)
    ? "stop"
    : server.installed
      ? "start"
      : "install";
  return (
    <button
      className={`button ${type === "stop" ? "danger" : "primary"}`}
      disabled={pending}
      onClick={() => onAction(type)}
    >
      {pending ? (
        <LoaderCircle className="spin" size={16} />
      ) : type === "stop" ? (
        <Square size={14} />
      ) : type === "install" ? (
        <Download size={16} />
      ) : (
        <Power size={16} />
      )}
      <span>
        {type === "stop"
          ? server.state === "restart_waiting"
            ? "Cancelar reinício"
            : "Parar"
          : type === "install"
            ? "Preparar"
            : "Iniciar"}
      </span>
    </button>
  );
}
