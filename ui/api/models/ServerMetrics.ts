export interface Metrics {
  supported: boolean;
  pid: number | null;
  cpuPercent: number | null;
  memoryBytes: number | null;
  privateBytes: number | null;
  error: string | null;
}
