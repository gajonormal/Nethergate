export interface Item {
  projectId: string;
  title: string;
  versionNumber: string;
  size: number;
  previous: { versionNumber: string } | null;
  distribution: { requiredProjects: string[] };
}

export interface Bundle {
  projects: string[];
  minecraft: string;
  items: Item[];
  warnings: string[];
  fingerprint: string;
}
