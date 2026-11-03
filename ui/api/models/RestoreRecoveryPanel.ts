import type { BackupInfo } from "../models";

export interface Plan {
  fingerprint: string;
  safetyBackup: BackupInfo;
  sourceBackup: string;
  worlds: string[];
}
