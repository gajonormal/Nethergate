export type Provider = "google_drive" | "dropbox";

export interface Settings {
  namespace: string;
  compression: boolean;
  automatic: boolean;
}

export interface Status {
  busy: boolean;
  account: {
    connected: boolean;
    clientId: string | null;
    pending: boolean;
    error: string | null;
  };
  settings: Settings;
  state: {
    error: string | null;
    lastSuccess: number | null;
    sentBlocks: number;
    reusedBlocks: number;
    pendingBackup: string | null;
  };
}
