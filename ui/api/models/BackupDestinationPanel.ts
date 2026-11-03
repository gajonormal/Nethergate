export interface Destination {
  external: boolean;
  base: string | null;
  path: string | null;
  localPath: string;
  error: string | null;
}
