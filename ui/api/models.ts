export type ServerState =
  | "offline"
  | "starting"
  | "online"
  | "stopping"
  | "installing"
  | "backing_up"
  | "restoring"
  | "managing_plugins"
  | "exporting_server"
  | "exporting_modpack"
  | "migrating"
  | "checking_migration"
  | "error"
  | "restart_waiting";

export interface Server {
  software: "paper" | "fabric";
  id: string;
  name: string;
  directory: string;
  version: string;
  memoryMb: number;
  port: number;
  contact: string;
  javaMajor: number;
  javaPath: string;
  state: ServerState;
  installed: boolean;
  eulaAccepted: boolean;
  error: string | null;
  pid: number | null;
  localAddress: string | null;
}
export interface LogLine {
  seq: number;
  text: string;
}

export interface BackupInfo {
  id: string;
  createdAt: number;
  sizeBytes: number;
  fileCount: number;
  worlds: string[];
  minecraftVersion: string;
  reason: string;
}

export interface RestoreResult {
  safetyBackup: BackupInfo;
  restoredBackup: string;
  cleanupWarning: string | null;
}
