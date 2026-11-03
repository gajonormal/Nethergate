import type { Server, BackupInfo, RestoreResult, LogLine } from "./models";
import type * as Auto from "./models/AutoBackupPanel";
import type * as Destination from "./models/BackupDestinationPanel";
import type * as Cloud from "./models/CloudBackupPanel";
import type * as Service from "./models/CloudProvidersPanel";
import type * as Connection from "./models/ConnectionPanel";
import type * as Ddns from "./models/DdnsPanel";
import type * as Energy from "./models/EnergyStatus";
import type * as Firewall from "./models/FirewallPanel";
import type * as Game from "./models/GameplayPanel";
import type * as Internet from "./models/InternetPanel";
import type * as Jar from "./models/ManualJarsPanel";
import type * as Migration from "./models/MigrationPanel";
import type * as Players from "./models/PlayersPanel";
import type * as Solver from "./models/PluginSolverPanel";
import type * as Plugins from "./models/PluginsPanel";
import type * as Public from "./models/PublicAccessPanel";
import type * as Recovery from "./models/RecoveryPanel";
import type * as Restore from "./models/RestoreRecoveryPanel";
import type * as Router from "./models/RouterPanel";
import type * as Metrics from "./models/ServerMetrics";
import type * as Transfer from "./models/ServerTransferPanel";
import type * as Sharing from "./models/SharingPanel";

type Id = { id: string };
type Ok = { ok: boolean };
type Started = { started: boolean };
type Call<P, R> = { request: P; response: R };
type Provider = Id & { provider: Service.Provider };
type CloudHistory = { backups: BackupInfo[]; limited: boolean };
export interface ServerSettings {
  name: string;
  memoryMb: number;
  port: number;
  javaPath: string;
}
export interface CreateServer extends ServerSettings {
  software: "paper" | "fabric";
  version: string;
  contact: string;
  javaMajor: number;
  eulaAccepted: boolean;
}
export interface SharingRequest extends Id {
  port: number;
  consent: boolean;
  temporary: boolean;
  recover: boolean;
  wan: string;
  allowFallback: boolean;
  expectedNetworkRevision?: number;
  expectedTarget?: string;
}
export interface RetentionPlan {
  keep: number;
  remove: BackupInfo[];
  fingerprint: string;
}

/** The wire contract is owned by the API, never by an individual caller. */
export interface ApiContract {
  list: Call<void, Server[]>;
  world_preview: Call<
    Id & { revision?: string },
    {
      revision: string | null;
      image: string | null;
      generating: boolean;
    }
  >;
  create: Call<CreateServer, Id>;
  versions: Call<{ contact: string; software: string }, string[]>;
  settings: Call<Id & { settings: ServerSettings }, Ok>;
  start: Call<Id, Ok>;
  stop: Call<Id, Ok>;
  install: Call<Id, Ok>;
  prepare_java: Call<Id, Ok>;
  java_status: Call<Id, { compatible: boolean; detail: string | null }>;
  logs: Call<Id & { after: number }, LogLine[]>;
  command: Call<Id & { command: string }, Ok>;
  energy: Call<void, Energy.Energy>;
  metrics: Call<Id, Metrics.Metrics>;
  connection: Call<Id, Connection.Connection>;
  diagnose_connection: Call<Id, Connection.Connection>;
  set_connection: Call<Id & { mode: string; address: string }, Ok>;
  gameplay: Call<Id, Game.Gameplay>;
  save_gameplay: Call<Id & { settings: Game.Gameplay }, Game.Gameplay>;
  players: Call<Id, Players.Access>;
  online_players: Call<Id, string[]>;
  player_operation: Call<
    Id & { operation: string; name: string },
    Players.Access
  >;
  inspect_firewall: Call<Id, Firewall.Report>;
  repair_firewall: Call<Id & { expected: Firewall.Report }, Firewall.Report>;
  public_access: Call<Id, Public.Report>;
  set_public_access: Call<
    Id & { operation: string; expected: Public.Report },
    Public.Report
  >;
  internet_ip: Call<Id & { consent: boolean }, Internet.PublicIp>;
  internet_test: Call<
    Id & { expectedIp: string; port: number; consent: boolean },
    Internet.ExternalTest
  >;
  internet_wan: Call<Id & { wan: string }, { state: string }>;
  ddns_status: Call<Id, Ddns.Status>;
  ddns_save: Call<
    Id & {
      settings: {
        hostname: string;
        token: string;
        externalPort: number;
        automatic: boolean;
        consent: boolean;
      };
      expectedHostname?: string | null;
    },
    Ddns.Status
  >;
  ddns_update: Call<
    Id & {
      expectedHostname?: string | null;
      settings?: ApiContract["ddns_save"]["request"]["settings"];
    },
    Ddns.Status
  >;
  ddns_pause: Call<
    Id & {
      expectedHostname?: string | null;
      settings?: ApiContract["ddns_save"]["request"]["settings"];
    },
    Ddns.Status
  >;
  ddns_test: Call<
    Id & { expectedHostname: string; consent: boolean },
    Ddns.ExternalTest
  >;
  recovery: Call<Id, Recovery.Recovery>;
  save_recovery: Call<Id & { enabled: boolean }, Recovery.Recovery>;
  backups: Call<Id, BackupInfo[]>;
  create_backup: Call<Id, BackupInfo>;
  restore_backup: Call<Id & { backupId: string }, RestoreResult>;
  restore_recovery_pending: Call<Id, boolean>;
  restore_recovery_plan: Call<Id, Restore.Plan | null>;
  recover_restore: Call<
    Id & { fingerprint: string; confirmed: boolean },
    { message: string }
  >;
  backup_schedule: Call<Id, Auto.Status>;
  save_backup_schedule: Call<Id & { policy: Auto.Policy }, Auto.Status>;
  backup_destination: Call<Id, Destination.Destination>;
  save_backup_destination: Call<
    Id & { path: string | null },
    Destination.Destination
  >;
  cloud_status: Call<Id, Cloud.Status>;
  cloud_save: Call<Id & { settings: Cloud.Settings }, Cloud.Status>;
  cloud_pause: Call<Id, Cloud.Status>;
  cloud_history: Call<Id, CloudHistory>;
  cloud_upload: Call<Id & { backupId?: string | null }, Started>;
  cloud_download: Call<Id & { backupId: string }, Started>;
  cloud_retention_preview: Call<Id & { keep: number }, RetentionPlan>;
  cloud_retention_apply: Call<
    Id & { keep: number; fingerprint: string; confirmed: boolean },
    { removed: number }
  >;
  cloud_account_begin: Call<
    Provider & { config: { clientId: string; clientSecret?: string } },
    { url: string }
  >;
  cloud_account_disconnect: Call<Provider, Service.Status["account"]>;
  service_cloud_status: Call<Provider, Service.Status>;
  service_cloud_save: Call<
    Provider & { settings: Service.Settings },
    Service.Status
  >;
  service_cloud_history: Call<Provider, CloudHistory>;
  service_cloud_upload: Call<Provider & { backupId?: string | null }, Started>;
  service_cloud_download: Call<Provider & { backupId: string }, Started>;
  router_status: Call<Id, Router.Status>;
  router_forget_expired: Call<Id, Router.Status>;
  router_preview: Call<
    Id & { provider: string; externalPort: number },
    Router.Plan
  >;
  router_create: Call<Id & { ticket?: string }, Router.Status>;
  router_remove: Call<Id & { ticket?: string }, Router.Status>;
  manual_jars: Call<Id, { files: Jar.Jar[]; disabled: Jar.Jar[] }>;
  manual_jar_file_preview: Call<{ path: string }, Jar.Jar>;
  manual_jar_preview: Call<
    Id & { filename: string; consent: boolean },
    Jar.Review
  >;
  manual_jar_add: Call<Id & { path: string; sha512: string }, unknown>;
  manual_jar_disable: Call<Id & { filename: string; sha512: string }, unknown>;
  manual_jar_enable: Call<Id & { disabledId?: string }, unknown>;
  manual_jar_adopt: Call<
    Id & { filename: string; fingerprint: string; consent: boolean },
    unknown
  >;
  migration_versions: Call<Id, string[]>;
  migration_preview: Call<Id & { target: string }, Migration.Plan>;
  migrate_server: Call<
    Id & {
      input: {
        target: string;
        name: string;
        port: number;
        fingerprint: string;
        acknowledged: boolean;
      };
    },
    unknown
  >;
  plugins: Call<Id, Plugins.Inventory>;
  plugin_search: Call<Id & { query: string }, { hits: Plugins.Hit[] }>;
  plugin_preview: Call<Id & { projectId: string }, Plugins.Plan>;
  plugin_install: Call<
    Id & { projectId: string; versionId: string; fingerprint: string },
    {
      installed: string;
      version: string;
      snapshot: string;
      worldBackup: unknown;
    }
  >;
  addon_set_preview: Call<Id & { projects: string[] }, Solver.Bundle>;
  addon_set_install: Call<
    Id & { projects: string[]; fingerprint: string },
    { changed: number; snapshot: string }
  >;
  export_modpack: Call<
    Id,
    { path: string; clientMods: number; serverOnlyExcluded: number }
  >;
  preview_import: Call<{ path: string }, Transfer.Preview>;
  export_server: Call<
    Id & { path: string },
    { path: string; fileCount: number }
  >;
  import_server: Call<
    {
      path: string;
      fingerprint: string;
      name: string;
      port: number;
      memoryMb: number;
      contact: string;
      javaPath: string;
      eulaAccepted: boolean;
    },
    unknown
  >;
  share_status: Call<Id, Sharing.Share>;
  playit_status: Call<Id, Sharing.Setup>;
  playit_prepare: Call<SharingRequest, Sharing.Setup>;
  playit_confirm: Call<SharingRequest, Sharing.Setup>;
  share_diagnose: Call<SharingRequest, Sharing.Advice>;
  share_start: Call<SharingRequest, Sharing.Share>;
  share_assisted_start: Call<SharingRequest, Sharing.Share>;
  share_stop: Call<SharingRequest, Sharing.Share>;
  share_cancel_check: Call<Id & { expectedOperation?: number }, Sharing.Share>;
  share_test: Call<Id & { consent: boolean }, Sharing.ExternalTest>;
}
export type ApiAction = keyof ApiContract;
export type ApiRequest<A extends ApiAction> = ApiContract[A]["request"];
export type ApiResponse<A extends ApiAction> = ApiContract[A]["response"];
export type ScopedArgs<A extends ApiAction, Scope extends object> =
  {} extends Omit<ApiRequest<A>, keyof Scope>
    ? [payload?: Omit<ApiRequest<A>, keyof Scope>]
    : [payload: Omit<ApiRequest<A>, keyof Scope>];
