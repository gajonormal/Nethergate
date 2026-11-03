export interface Status {
  state: string;
  provider?: string;
  address?: string;
  externalPort?: number;
  internalPort?: number;
  gateway?: string;
  wan?: string;
  publicIp?: boolean;
  expiresAt?: number;
  error?: string | null;
  canForget?: boolean;
}

export interface Plan {
  ticket: string;
  provider: string;
  localAddress: string;
  internalPort: number;
  externalPort: number;
  gateway: string;
  wan: string;
  publicIp: boolean;
  leaseSeconds: number;
  expiresAt: number;
}
