export interface Energy {
  supported: boolean;
  activeServers: number;
  protectedServers: number;
  supply: {
    source: "ac" | "battery" | "unknown";
    batteryPercent: number | null;
    hasBattery: boolean | null;
  } | null;
  error: string | null;
}
