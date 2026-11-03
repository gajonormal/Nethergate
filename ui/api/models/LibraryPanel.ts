export interface Info {
  root: string;
  serverCount: number;
  busy: boolean;
  warning: string | null;
}

export interface Preview {
  root: string;
  fingerprint: string;
  externalDirectories: boolean;
  servers: { name: string; software: string; version: string; port: number }[];
}
