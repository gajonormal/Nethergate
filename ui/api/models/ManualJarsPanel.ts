export interface Jar {
  filename: string;
  size: number;
  sha512: string;
  duplicates?: string[];
  id?: string;
}

export interface Review {
  title: string;
  versionNumber: string;
  projectId: string;
  fingerprint: string;
  sourceFilename: string;
}
