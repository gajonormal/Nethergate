export interface Plan {
  from: string;
  target: string;
  javaMajor: number;
  fingerprint: string;
  warnings: string[];
  addons: {
    items: { projectId: string; title: string; versionNumber: string }[];
  };
}
