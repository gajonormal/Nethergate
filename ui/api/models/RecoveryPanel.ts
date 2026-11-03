export interface Recovery {
  enabled: boolean;
  state: {
    attempt: number;
    maximumAttempts: number;
    remainingSeconds: number | null;
    pending: boolean;
  };
}
