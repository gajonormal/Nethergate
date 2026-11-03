export interface Report {
  state: string;
  activeCount: number;
  aligned: boolean;
  rulesSignature: string;
  planSignature: string;
  rules: {
    name: string;
    owned: boolean;
    enabled: boolean;
    addresses: string[];
    ports: string[];
  }[];
  desired: {
    address: string;
    port: number;
    interface: string | null;
    program: string | null;
  };
}
