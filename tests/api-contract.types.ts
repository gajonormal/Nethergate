import { api, scopedApi, type Server } from "../ui/api";

// Compiled by tsc, never executed: each expected error protects the wire contract.
export function verifyApiContract() {
  const servers: Promise<Server[]> = api("list");
  const command = api("command", { id: "server", command: "list" });
  const scoped = scopedApi({ id: "server" });
  void servers;
  void command;
  void scoped("cloud_download", { backupId: "backup" });
  // @ts-expect-error Unknown operation names must not reach the transport.
  void api("strat", { id: "server" });
  // @ts-expect-error Commands need their reviewed server and command text.
  void api("command", { id: "server" });
  void api("router_preview", {
    id: "server",
    provider: "upnp",
    // @ts-expect-error Ports are numbers, not strings from an unchecked input.
    externalPort: "25565",
  });
  // @ts-expect-error Scoping an id must not make a backup id optional.
  void scoped("cloud_download");
  // @ts-expect-error The response type is determined by the operation.
  const wrong: Promise<string[]> = api("list");
  void wrong;
}
