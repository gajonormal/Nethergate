export interface Settings {
  endpoint: string;
  region: string;
  bucket: string;
  prefix: string;
  automatic: boolean;
  compression: boolean;
  deduplication: boolean;
  retentionKeep: number;
  accessKey: string;
  secretKey: string;
  sessionToken: string;
}

export interface Status {
  configured: boolean;
  busy: boolean;
  settings: Omit<Settings, "accessKey" | "secretKey" | "sessionToken"> | null;
  state: {
    lastSuccess: number | null;
    lastBackupId: string | null;
    error: string | null;
    completedFiles: number;
    totalFiles: number;
    retentionError: string | null;
    sentBlocks: number;
    reusedBlocks: number;
    pendingBackup: string | null;
  };
}
