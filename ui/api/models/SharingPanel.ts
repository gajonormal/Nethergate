export interface Share {
  state: string;
  address: string | null;
  detail?: string;
  startedAt?: number;
  recoveryEnabled?: boolean;
  attempt?: number;
  maximumAttempts?: number;
  remainingSeconds?: number | null;
  fallbackUsed?: boolean;
  directTest?: { state: string; error?: string } | null;
  operation?: number;
  cancelled?: boolean;
}

export interface Advice {
  recommendation: "review" | "direct" | "playit";
  reason: string;
  blocked: boolean;
  target: string;
  networkRevision: number;
  checkedAt: number;
  wanHint: string;
  directTest: {
    state: string;
    error?: string;
    observedAt?: number | null;
  } | null;
}

export interface ExternalTest {
  state: string;
  address: string;
  checkedAt: number;
  observedAt: number | null;
  version: string | null;
  cacheHit: boolean | null;
}

export interface Setup {
  associated: boolean;
  ready: boolean;
  claimUrl: string | null;
  address: string | null;
  target: string | null;
  error?: string | null;
}
