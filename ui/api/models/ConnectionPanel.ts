export interface Probe {
  status: string;
  address: string;
  version: string | null;
  elapsedMs: number | null;
  detail: string;
}

export interface Adapter {
  name: string;
  ip: string;
  gateway: string | null;
  address: string;
  configured: boolean;
  probe: Probe | null;
}

export interface Connection {
  network: {
    supported: boolean;
    revision: number;
    changedAt: number | null;
    checking: boolean;
    error: string | null;
    bindState: string;
    hasLocalNetwork: boolean | null;
  };
  scope: string;
  bindAddress: string;
  port: number;
  localAddress: string | null;
  managedProcess: boolean;
  state: string;
  adapters: Adapter[];
  discoveryError: string | null;
  probe: Probe | null;
  portState: string;
  authenticated: boolean;
  whitelistEnabled: boolean;
  testedAt: number | null;
}
