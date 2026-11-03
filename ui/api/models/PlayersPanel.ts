export interface Player {
  name: string;
  uuid: string;
}

export interface Access {
  enabled: boolean;
  authenticated: boolean;
  players: Player[];
}
