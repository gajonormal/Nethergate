import type { Server } from "../models";

export interface Status {
  keepRunning: boolean;
  independent?: boolean;
  busy?: boolean;
  error: string | null;
  closing: boolean;
  servers?: Server[];
}
