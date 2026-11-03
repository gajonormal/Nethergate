export interface Status {
  supported: boolean;
  configured: boolean;
  hostname: string | null;
  externalPort: number | null;
  automatic: boolean;
  hasToken: boolean;
  lastPublishedIp: string | null;
  lastSuccess: number | null;
  error: string | null;
  needsNetworkUpdate: boolean;
  wanChangedAt: number | null;
}

export interface ExternalTest {
  state: string;
  checkedAt: number;
  observedAt: number | null;
  address: string;
}
