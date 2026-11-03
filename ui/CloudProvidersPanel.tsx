import { errorText } from "./lib/format";
import { formatDateTime } from "./lib/format";
import { scopedApi, type ScopedArgs } from "./api";
import type {
  Provider,
  Settings,
  Status,
} from "./api/models/CloudProvidersPanel";
import { useEffect, useState } from "react";
import { api, openCloudPage, type BackupInfo, type Server } from "./api";
import { CloudBackupPanel } from "./CloudBackupPanel";
import { OreToggle } from "./OreToggle";

export function CloudProvidersPanel({
  server,
  onHistoryChange,
}: {
  server: Server;
  onHistoryChange: () => void;
}) {
  const [selection, setSelection] = useState<Provider | "s3">("google_drive");
  useEffect(() => {
    let disposed = false;
    void api("cloud_status", { id: server.id })
      .then((s) => {
        if (!disposed && s.configured) setSelection("s3");
      })
      .catch(() => {});
    return () => {
      disposed = true;
    };
  }, [server.id]);
  return (
    <div className="cloud-providers">
      <nav className="cloud-provider-tabs" aria-label="Destino cloud">
        {(
          [
            ["google_drive", "Google Drive"],
            ["dropbox", "Dropbox"],
            ["s3", "S3 compatível"],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            className={`button ${selection === value ? "primary" : "secondary"}`}
            aria-pressed={selection === value}
            onClick={() => setSelection(value)}
          >
            {label}
          </button>
        ))}
      </nav>
      {selection === "s3" ? (
        <CloudBackupPanel server={server} onHistoryChange={onHistoryChange} />
      ) : (
        <ServicePanel
          key={`${server.id}-${selection}`}
          provider={selection}
          server={server}
          onHistoryChange={onHistoryChange}
        />
      )}
    </div>
  );
}
function ServicePanel({
  provider,
  server,
  onHistoryChange,
}: {
  provider: Provider;
  server: Server;
  onHistoryChange: () => void;
}) {
  const [status, setStatus] = useState<Status | null>(null);
  const [form, setForm] = useState<Settings>({
    namespace: server.id,
    compression: true,
    automatic: false,
  });
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [history, setHistory] = useState<BackupInfo[]>([]);
  const [limited, setLimited] = useState(false);
  const name = provider === "google_drive" ? "Google Drive" : "Dropbox";
  const busy = pending || !!status?.busy;
  const authPending = !!status?.account.pending;
  useEffect(() => {
    let disposed = false;
    let wasBusy = false;
    async function refresh(first: boolean) {
      try {
        const value = await api("service_cloud_status", {
          id: server.id,
          provider,
        });
        if (disposed) return;
        setStatus(value);
        if (first) {
          setForm(value.settings);
          setClientId(value.account.clientId ?? "");
        }
        if (wasBusy && !value.busy) onHistoryChange();
        wasBusy = value.busy;
      } catch (e) {
        if (!disposed) setError(errorText(e));
      }
    }
    void refresh(true);
    const timer = window.setInterval(() => void refresh(false), 2000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [server.id, provider]);
  async function task<
    A extends
      | "cloud_account_begin"
      | "cloud_account_disconnect"
      | "service_cloud_save"
      | "service_cloud_upload"
      | "service_cloud_download"
      | "service_cloud_history",
  >(action: A, ...args: ScopedArgs<A, { id: string; provider: Provider }>) {
    setPending(true);
    setError("");
    setNotice("");
    try {
      const value = await scopedApi({ id: server.id, provider })(
        action,
        ...args,
      );
      if (action === "cloud_account_begin") {
        setSecret("");
        await openCloudPage((value as { url: string }).url);
        setNotice(
          "Conclui a autorização no browser. A ligação será confirmada aqui.",
        );
      } else if (action === "service_cloud_history") {
        const result = value as { backups: BackupInfo[]; limited: boolean };
        setHistory(result.backups);
        setLimited(result.limited);
      } else if (action === "cloud_account_disconnect") {
        setHistory([]);
        setNotice(
          "Conta desligada neste PC. Os backups cloud foram preservados.",
        );
      } else if (action === "service_cloud_save") {
        setHistory([]);
        setNotice("Preferências cloud guardadas.");
      } else
        setNotice(
          "Transferência iniciada. Podes repetir o envio para retomar os blocos em falta.",
        );
      setStatus(await api("service_cloud_status", { id: server.id, provider }));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  return (
    <article className="connection-card service-cloud-panel">
      <p>
        {provider === "google_drive"
          ? "Backups na área privada do Nethergate no Drive."
          : "Backups na pasta da aplicação no Dropbox."}{" "}
        Com verificação, deduplicação e retoma por blocos.
      </p>
      <details open={!status?.account.connected || undefined}>
        <summary>
          {status?.account.connected ? `Conta ${name} ligada` : `Ligar ${name}`}
        </summary>
        <div className="cloud-account-controls">
          {!status?.account.connected && (
            <p>
              Configura os identificadores OAuth da aplicação e autoriza a conta
              no browser.
            </p>
          )}
          <label>
            {provider === "google_drive"
              ? "Client ID (aplicação Desktop)"
              : "App Key (aplicação App folder)"}
            <input
              autoComplete="off"
              disabled={busy || authPending}
              value={clientId}
              onChange={(e) => setClientId(e.target.value.trim())}
            />
          </label>
          {provider === "google_drive" && (
            <label>
              Client secret (opcional)
              <input
                type="password"
                autoComplete="new-password"
                disabled={busy || authPending}
                value={secret}
                onChange={(e) => setSecret(e.target.value)}
              />
            </label>
          )}
          <div className="modal-actions">
            <button
              className="button primary"
              disabled={busy || authPending || !clientId}
              onClick={() =>
                void task("cloud_account_begin", {
                  config: { clientId, clientSecret: secret },
                })
              }
            >
              {authPending
                ? "A aguardar autorização…"
                : status?.account.connected
                  ? "Ligar outra conta"
                  : "Ligar conta"}
            </button>
            {(status?.account.connected || authPending) && (
              <button
                className="button secondary"
                disabled={busy}
                onClick={() => void task("cloud_account_disconnect")}
              >
                {authPending ? "Cancelar autorização" : "Desligar neste PC"}
              </button>
            )}
          </div>
          <details className="backup-help">
            <summary>Configurar a aplicação OAuth</summary>
            <div className="cloud-account-controls">
              <p>
                {provider === "google_drive"
                  ? "No Google Cloud, ativa a Drive API e cria um cliente OAuth do tipo Desktop. Usa o âmbito drive.appdata e adiciona a tua conta como utilizador de teste, se necessário."
                  : "Na Dropbox App Console, cria uma aplicação Scoped access / App folder. Ativa files.content.read, files.content.write e files.metadata.read. Regista http://127.0.0.1:53682/oauth/callback como redirect URI."}
              </p>
              <p>
                A conta é partilhada pelos servidores desta biblioteca. Usa o
                mesmo registo OAuth noutros PCs para recuperar os mesmos
                backups.
              </p>
              <button
                className="button secondary"
                onClick={() =>
                  void openCloudPage(
                    provider === "google_drive"
                      ? "https://console.cloud.google.com/apis/credentials"
                      : "https://www.dropbox.com/developers/apps",
                  ).catch((e) => setError(errorText(e)))
                }
              >
                Abrir configuração {name}
              </button>
            </div>
          </details>
        </div>
      </details>
      {status?.account.connected && (
        <>
          <details>
            <summary>Preferências dos backups</summary>
            <fieldset disabled={busy}>
              <label>
                Identificador cloud deste servidor
                <input
                  value={form.namespace}
                  onChange={(e) =>
                    setForm({ ...form, namespace: e.target.value })
                  }
                />
              </label>
              <p>
                O mesmo identificador permite recuperar este histórico noutro
                PC. Alterá-lo abre um histórico separado.
              </p>
              <label className="eula-check">
                <OreToggle
                  checked={form.compression}
                  onChange={(e) =>
                    setForm({ ...form, compression: e.target.checked })
                  }
                />
                <span>Comprimir os blocos antes de enviar</span>
              </label>
              <label className="eula-check">
                <OreToggle
                  checked={form.automatic}
                  onChange={(e) =>
                    setForm({ ...form, automatic: e.target.checked })
                  }
                />
                <span>Enviar automaticamente com o motor ligado</span>
              </label>
              <button
                className="button primary"
                disabled={!/^[a-zA-Z0-9_-]{1,128}$/.test(form.namespace)}
                onClick={() =>
                  void task("service_cloud_save", { settings: form })
                }
              >
                Guardar preferências
              </button>
            </fieldset>
          </details>
          <div className="modal-actions">
            <button
              className="button primary"
              disabled={busy}
              onClick={() => void task("service_cloud_upload")}
            >
              Enviar backup mais recente
            </button>
            <button
              className="button secondary"
              disabled={busy}
              onClick={() => void task("service_cloud_history")}
            >
              Consultar histórico
            </button>
            {status.state.pendingBackup && !status.busy && (
              <button
                className="button secondary"
                disabled={busy}
                onClick={() =>
                  void task("service_cloud_upload", {
                    backupId: status.state.pendingBackup,
                  })
                }
              >
                Retomar envio interrompido
              </button>
            )}
          </div>
          <p role="status">
            {status.busy ? "Operação cloud em curso. " : ""}
            {status.state.sentBlocks} blocos enviados ·{" "}
            {status.state.reusedBlocks} reutilizados
            {status.state.lastSuccess
              ? ` · Último envio: ${formatDateTime(status.state.lastSuccess)}`
              : ""}
          </p>
        </>
      )}
      {history.map((copy) => (
        <div className="plugin-row" key={copy.id}>
          <div>
            <strong>{formatDateTime(copy.createdAt)}</strong>
            <p>
              Minecraft {copy.minecraftVersion} ·{" "}
              {(copy.sizeBytes / 1048576).toFixed(1)} MB
            </p>
          </div>
          <button
            className="button secondary"
            disabled={busy || !status?.account.connected}
            onClick={() =>
              void task("service_cloud_download", { backupId: copy.id })
            }
          >
            Transferir para backups
          </button>
        </div>
      ))}
      {limited && <p>São apresentados os 20 backups mais recentes.</p>}
      <details className="backup-help">
        <summary>Retoma e armazenamento</summary>
        <p>
          Repetir um envio reutiliza os blocos confirmados, mesmo após
          reiniciar. A deduplicação aplica-se ao mesmo identificador cloud.
          Transferir cria uma cópia local; restaurar continua a ser uma ação
          separada. Os blocos partilhados e incompletos não são eliminados
          automaticamente. Tokens protegidos pela conta Windows; podes revogar o
          acesso no fornecedor.
        </p>
      </details>
      {status?.account.error && (
        <p role="alert" className="inline-error">
          {status.account.error}
        </p>
      )}
      {status?.state.error && (
        <p role="alert" className="inline-error">
          {status.state.error}
        </p>
      )}
      {error && (
        <p role="alert" className="inline-error">
          {error}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
    </article>
  );
}
