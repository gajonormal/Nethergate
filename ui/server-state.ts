import type { Server, ServerState } from "./api";
export const stateLabels: Record<ServerState, string> = {
  offline: "Parado",
  starting: "A iniciar",
  online: "Online",
  stopping: "A guardar",
  installing: "A preparar",
  backing_up: "A criar backup",
  restoring: "A restaurar",
  managing_plugins: "A preparar plugins",
  exporting_server: "A exportar servidor",
  exporting_modpack: "A exportar pacote",
  migrating: "A preparar migração",
  checking_migration: "A verificar migração",
  error: "Precisa de atenção",
  restart_waiting: "A aguardar reinício",
};
export const active = (server: Server) =>
  !["offline", "error"].includes(server.state);
