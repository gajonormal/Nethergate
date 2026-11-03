export interface Policy {
  enabled: boolean;
  intervalMinutes: number;
  keep: number;
}

export interface Status {
  phase: string;
  schedule: {
    policy: Policy;
    nextDue: number | null;
    lastSuccess: number | null;
    lastBackupId: string | null;
    error: string | null;
    cleanupWarning: string | null;
  };
}
