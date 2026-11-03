import type {
  ApiAction,
  ApiRequest,
  ApiResponse,
  ScopedArgs,
} from "./api/contract";
import { invoke, isTauri } from "@tauri-apps/api/core";

export const desktop = isTauri();
export async function openCloudPage(url: string) {
  const parsed = new URL(url);
  if (
    parsed.protocol !== "https:" ||
    parsed.username ||
    parsed.password ||
    parsed.hash ||
    ![
      "accounts.google.com/o/oauth2/v2/auth",
      "www.dropbox.com/oauth2/authorize",
      "console.cloud.google.com/apis/credentials",
      "www.dropbox.com/developers/apps",
    ].includes(parsed.host + parsed.pathname)
  )
    throw new Error("Ligação cloud inválida");
  if (desktop) await invoke("open_cloud_page", { url });
  else window.open(url, "_blank", "noopener,noreferrer");
}
export async function openEula() {
  if (desktop) await invoke("open_eula");
}
export async function openDuckDns() {
  if (desktop) await invoke("open_duckdns");
  else window.open("https://www.duckdns.org/", "_blank", "noopener,noreferrer");
}
export async function openPlayit(url: string) {
  if (
    !/^https:\/\/playit\.gg\/(claim\/[a-f0-9]{10}|account\/tunnels)$/.test(url)
  )
    throw new Error("Ligação playit inválida");
  if (desktop) await invoke("open_playit", { url });
  else window.open(url, "_blank", "noopener,noreferrer");
}
let session: Promise<string> | undefined;
export async function api<A extends ApiAction>(
  action: A,
  ...args: ApiRequest<A> extends void
    ? [payload?: undefined]
    : [payload: ApiRequest<A>]
): Promise<ApiResponse<A>> {
  const payload = args[0] ?? {};
  if (desktop) return invoke<ApiResponse<A>>("hub_call", { action, payload });
  session ??= fetch("/api/session", {
    headers: { "X-NetherGate-Client": "hub" },
  })
    .then(async (r) => {
      if (!r.ok) throw new Error("Não foi possível ligar ao motor local.");
      return (await r.json()).token as string;
    })
    .catch((error) => {
      session = undefined;
      throw error;
    });
  const token = await session;
  const response = await fetch("/api/call", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-NetherGate-Token": token,
    },
    body: JSON.stringify({ action, payload }),
  });
  const result = await response.json();
  if (!response.ok) {
    if (response.status === 403) session = undefined;
    throw new Error(result.error || "O pedido não foi concluído.");
  }
  return result;
}

export type * from "./api/models";

export type {
  ApiAction,
  ApiRequest,
  ApiResponse,
  ScopedArgs,
} from "./api/contract";

/** Bind a server/provider scope once while keeping each action's fields checked. */
export function scopedApi<S extends object>(scope: S) {
  return <A extends ApiAction>(
    action: A,
    ...args: ScopedArgs<A, S>
  ): Promise<ApiResponse<A>> => {
    const payload = { ...scope, ...args[0] } as ApiRequest<A>;
    // The conditional argument tuple cannot be reduced for a generic action.
    const send = api as (
      action: A,
      payload: ApiRequest<A>,
    ) => Promise<ApiResponse<A>>;
    return send(action, payload);
  };
}
