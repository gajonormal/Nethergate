export interface PublicIp {
  ip: string;
  checkedAt: number;
  source: string;
}

export interface ExternalTest {
  state: string;
  address: string;
  checkedAt: number;
  version: string | null;
  observedAt: number | null;
  cacheHit: boolean | null;
  cacheExpiresAt: number | null;
  loginConfirmed: boolean;
}
