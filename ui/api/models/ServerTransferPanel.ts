export interface Preview {
  path: string;
  profile: {
    name: string;
    minecraft: string;
    software: string;
    memoryMb: number;
    javaMajor: number;
  };
  fileCount: number;
  bytes: number;
  fingerprint: string;
  reconfigure: string[];
}
