export type Audience = "local" | "lan" | "internet";

export interface Access {
  enabled: boolean;
  authenticated: boolean;
  players: { name: string; uuid: string }[];
}
