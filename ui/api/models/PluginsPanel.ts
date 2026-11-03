export interface Installed {
  distribution?: { clientSide: string; serverSide: string };
  projectId: string;
  title: string;
  versionId: string;
  versionNumber: string;
  filename: string;
}

export interface Inventory {
  installed: { plugin: Installed; intact: boolean }[];
  external: string[];
  pending: boolean;
}

export interface Hit {
  projectId: string;
  title: string;
  description: string;
  author: string;
  downloads: number;
}

export interface Plan {
  missingProjects: string[];
  distribution: { clientSide: string; serverSide: string };
  projectId: string;
  title: string;
  versionId: string;
  versionNumber: string;
  minecraft: string;
  sourceFilename: string;
  size: number;
  sha512: string;
  previous: Installed | null;
  blockers: string[];
  fingerprint: string;
}
