export interface Report {
  state: string;
  address?: string;
  port?: number;
  interface?: string;
  program?: string;
  ruleName?: string;
  previous?: { localAddress: string[] } | null;
}
