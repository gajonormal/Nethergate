import { errorText } from "./lib/format";
import { formatDateTime } from "./lib/format";
import { scopedApi, type ScopedArgs } from "./api";
import type { Settings, Status } from "./api/models/CloudBackupPanel";
import { OreToggle } from "./OreToggle";
import { useEffect, useState } from "react";
import { api, type BackupInfo, type Server } from "./api";

export function CloudBackupPanel({
  server,
  onHistoryChange,
}: {
  server: Server;
  onHistoryChange: () => void;
}) {
  const defaults: Settings = {
    endpoint: "",
    region: "us-east-1",
    bucket: "",
    prefix: `nethergate/${server.id}`,
    automatic: false,
    compression: false,
    deduplication: true,
    retentionKeep: 0,
    accessKey: "",
    secretKey: "",
    sessionToken: "",
  };
  const [form, setForm] = useState(defaults);
  const [status, setStatus] = useState<Status | null>(null);
  const [history, setHistory] = useState<BackupInfo[]>([]);
  const [limited, setLimited] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [retention, setRetention] = useState<{
    fingerprint: string;
    remove: BackupInfo[];
    keep: number;
  } | null>(null);
  const [retentionConfirmed, setRetentionConfirmed] = useState(false);
  const [keep, setKeep] = useState(7);
  useEffect(() => {
    let closed = false;
    let previousBusy = false;
    setForm(defaults);
    setStatus(null);
    setHistory([]);
    setRetention(null);
    setRetentionConfirmed(false);
    setError("");
    async function refresh(initial: boolean) {
      try {
        const value = await api("cloud_status", { id: server.id });
        if (closed) return;
        setStatus(value);
        if (initial && value.settings)
          setForm({ ...defaults, ...value.settings });
        if (previousBusy && !value.busy) onHistoryChange();
        previousBusy = value.busy;
      } catch (e) {
        if (!closed) setError(errorText(e));
      }
    }
    void refresh(true);
    const timer = window.setInterval(() => void refresh(false), 5000);
    return () => {
      closed = true;
      window.clearInterval(timer);
    };
  }, [server.id]);
  async function task<
    A extends
      | "cloud_save"
      | "cloud_upload"
      | "cloud_download"
      | "cloud_history"
      | "cloud_pause"
      | "cloud_retention_preview"
      | "cloud_retention_apply",
  >(action: A, ...args: ScopedArgs<A, { id: string }>) {
    setPending(true);
    setError("");
    setNotice("");
    try {
      const value = await scopedApi({ id: server.id })(action, ...args);
      if (action === "cloud_retention_preview") {
        setRetention(value as typeof retention);
        setRetentionConfirmed(false);
      } else if (action === "cloud_retention_apply") {
        setRetention(null);
        setRetentionConfirmed(false);
        setHistory([]);
        setNotice(
          `Limpeza concluída: ${(value as { removed: number }).removed} cópias automáticas removidas.`,
        );
      } else if (action === "cloud_history") {
        const v = value as { backups: BackupInfo[]; limited: boolean };
        setHistory(v.backups);
        setLimited(v.limited);
      } else if (action === "cloud_save" || action === "cloud_pause") {
        setRetention(null);
        setStatus(value as Status);
        setForm((v) => ({
          ...v,
          accessKey: "",
          secretKey: "",
          sessionToken: "",
          ...(action === "cloud_pause" ? { automatic: false } : {}),
        }));
        setNotice(
          "Destino cloud guardado. As credenciais ficam protegidas nesta conta Windows.",
        );
      } else {
        setNotice(
          "Transferência iniciada. O estado abaixo será atualizado; o restauro do mundo continua a ser uma ação separada.",
        );
        setStatus(await api("cloud_status", { id: server.id }));
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  const busy = pending || !!status?.busy;
  const field = (key: keyof Settings, value: string) =>
    setForm({ ...form, [key]: value });
  return (
    <article className="connection-card cloud-backup-panel">
      <p>
        Envia os mundos e dados dos jogadores para um bucket S3 privado (AWS,
        R2, MinIO ou compatível). O fornecedor pode cobrar armazenamento e
        tráfego.
      </p>
      <details>
        <summary>Configurar destino S3</summary>
        <fieldset disabled={busy}>
          <label>
            Endpoint HTTPS (sem o nome do bucket)
            <input
              value={form.endpoint}
              placeholder="https://s3.eu-west-1.amazonaws.com"
              onChange={(e) => field("endpoint", e.target.value)}
            />
          </label>
          <div className="form-row">
            <label>
              Região (auto para R2)
              <input
                value={form.region}
                onChange={(e) => field("region", e.target.value)}
              />
            </label>
            <label>
              Bucket
              <input
                value={form.bucket}
                onChange={(e) => field("bucket", e.target.value)}
              />
            </label>
          </div>
          <label>
            Prefixo exclusivo deste servidor
            <input
              value={form.prefix}
              onChange={(e) => field("prefix", e.target.value)}
            />
          </label>
          <p>
            Usa o mesmo bucket, prefixo e credenciais noutro PC para recuperar o
            histórico. Alterá-los abre outro histórico sem apagar o anterior.
          </p>
          <label>
            Access key
            <input
              autoComplete="off"
              value={form.accessKey}
              placeholder={status?.configured ? "Deixar vazio para manter" : ""}
              onChange={(e) => field("accessKey", e.target.value)}
            />
          </label>
          <label>
            Secret key
            <input
              type="password"
              autoComplete="new-password"
              value={form.secretKey}
              placeholder={status?.configured ? "Deixar vazio para manter" : ""}
              onChange={(e) => field("secretKey", e.target.value)}
            />
          </label>
          <label>
            Session token (opcional)
            <input
              type="password"
              autoComplete="new-password"
              value={form.sessionToken}
              onChange={(e) => field("sessionToken", e.target.value)}
            />
          </label>
          <p className="muted">
            Deixa ambas as chaves vazias para as manter ou preenche ambas para
            as substituir. Inclui o token de sessão, se necessário.
          </p>
          <label className="eula-check">
            <OreToggle
              checked={form.deduplication}
              onChange={(e) =>
                setForm({ ...form, deduplication: e.target.checked })
              }
            />
            <span>Deduplicação e retoma por blocos após reiniciar</span>
          </label>
          <label className="eula-check">
            <OreToggle
              checked={form.compression}
              onChange={(e) =>
                setForm({ ...form, compression: e.target.checked })
              }
            />
            <span>Comprimir novos envios com gzip</span>
          </label>
          <label>
            Retenção automática cloud
            <select
              aria-label="Retenção automática cloud"
              value={form.retentionKeep}
              onChange={(e) =>
                setForm({ ...form, retentionKeep: Number(e.target.value) })
              }
            >
              <option value={0}>Desligada</option>
              {[2, 3, 5, 7, 14, 30, 50].map((v) => (
                <option key={v} value={v}>
                  Manter {v} cópias automáticas
                </option>
              ))}
            </select>
          </label>
          <p className="muted">
            Após um envio bem-sucedido, remove cópias automáticas antigas deste
            prefixo. Backups manuais e anteriores a restauros são preservados.
          </p>
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
            disabled={!form.endpoint || !form.bucket || !form.prefix}
            onClick={() => void task("cloud_save", { settings: form })}
          >
            Guardar destino cloud
          </button>
        </fieldset>
      </details>
      {status?.configured && (
        <>
          <p>
            <strong>
              {status.settings?.bucket}/{status.settings?.prefix}
            </strong>
            <br />
            {status.state.lastSuccess
              ? `Último envio verificado: ${formatDateTime(status.state.lastSuccess)}`
              : "Ainda não existe um envio verificado."}
          </p>
          <div className="modal-actions">
            <button
              className="button secondary"
              disabled={busy}
              onClick={() => void task("cloud_upload")}
            >
              Enviar backup mais recente
            </button>
            <button
              className="button secondary"
              disabled={busy}
              onClick={() => void task("cloud_history")}
            >
              Consultar histórico cloud
            </button>
            {status.settings?.automatic && (
              <button
                className="button secondary"
                disabled={busy}
                onClick={() => void task("cloud_pause")}
              >
                Pausar envios automáticos
              </button>
            )}
          </div>
        </>
      )}
      {status?.busy && (
        <p role="status">
          Transferência em curso · {status.state.completedFiles}/
          {status.state.totalFiles || "…"} ficheiros. A paragem do motor cancela
          a transferência entre pedidos.
        </p>
      )}
      {status?.state.pendingBackup && !status.busy && (
        <button
          className="button secondary"
          disabled={busy}
          onClick={() =>
            void task("cloud_upload", { backupId: status.state.pendingBackup })
          }
        >
          Retomar envio interrompido
        </button>
      )}
      {status?.settings?.deduplication && (
        <p>
          {status.state.sentBlocks || 0} blocos enviados ·{" "}
          {status.state.reusedBlocks || 0} reutilizados
        </p>
      )}
      {status?.state.error && (
        <p className="inline-error" role="alert">
          {status.state.error}
        </p>
      )}
      {status?.state.retentionError && (
        <p className="inline-error" role="alert">
          {status.state.retentionError}
        </p>
      )}
      {status?.configured && (
        <details className="backup-help">
          <summary>Limpar cópias automáticas antigas</summary>
          <div className="cloud-retention-controls">
            <p>
              Conserva as cópias automáticas mais recentes. A limpeza afeta
              apenas as cópias deste perfil, bucket e prefixo; preserva os
              backups locais e manuais.
            </p>
            <label>
              Quantidade a conservar
              <input
                type="number"
                min={2}
                max={50}
                value={keep}
                disabled={busy}
                onChange={(e) => {
                  setKeep(Number(e.target.value));
                  setRetention(null);
                  setRetentionConfirmed(false);
                }}
              />
            </label>
            <div className="modal-actions">
              <button
                className="button secondary"
                disabled={busy || keep < 2 || keep > 50}
                onClick={() => void task("cloud_retention_preview", { keep })}
              >
                Rever limpeza
              </button>
            </div>
            {retention && (
              <>
                <p>
                  {retention.remove.length
                    ? retention.remove.length === 1
                      ? "1 cópia automática será removida da cloud."
                      : `${retention.remove.length} cópias automáticas serão removidas da cloud.`
                    : "Não há cópias antigas para remover."}
                </p>
                {retention.remove.length > 0 && (
                  <>
                    <ul>
                      {retention.remove.map((b) => (
                        <li key={b.id}>
                          {formatDateTime(b.createdAt)} ·{" "}
                          {(b.sizeBytes / 1048576).toFixed(1)} MB
                        </li>
                      ))}
                    </ul>
                    <label className="eula-check">
                      <OreToggle
                        disabled={busy}
                        checked={retentionConfirmed}
                        onChange={(e) =>
                          setRetentionConfirmed(e.target.checked)
                        }
                      />
                      <span>Remover estas cópias da cloud</span>
                    </label>
                    <div className="modal-actions">
                      <button
                        className="button primary"
                        disabled={busy || !retentionConfirmed}
                        onClick={() =>
                          void task("cloud_retention_apply", {
                            keep: retention.keep,
                            fingerprint: retention.fingerprint,
                            confirmed: true,
                          })
                        }
                      >
                        Aplicar limpeza
                      </button>
                    </div>
                  </>
                )}
              </>
            )}
          </div>
        </details>
      )}
      {history.map((copy) => (
        <div className="plugin-row" key={copy.id}>
          <div>
            <strong>{formatDateTime(copy.createdAt)}</strong>
            <p>
              Minecraft {copy.minecraftVersion} · {copy.fileCount} ficheiros ·{" "}
              {(copy.sizeBytes / 1048576).toFixed(1)} MB
            </p>
          </div>
          <button
            className="button secondary"
            disabled={busy}
            onClick={() => void task("cloud_download", { backupId: copy.id })}
          >
            Transferir para backups
          </button>
        </div>
      ))}
      {limited && (
        <p>
          A listagem atingiu o limite. Consulta também o fornecedor para
          arquivos mais antigos.
        </p>
      )}
      <details className="backup-help">
        <summary>Verificação, retoma e restauro</summary>
        <p>
          Com deduplicação ativa, a retoma reutiliza blocos completos de 4 MiB
          mesmo após reiniciar. A limpeza do histórico preserva blocos
          partilhados; estes continuam a ocupar espaço.
        </p>
        <p>
          Os ficheiros enviados são relidos e verificados por SHA-256; o
          manifesto só é publicado no fim. A cópia local é preservada. A
          retenção remota só atua quando a ativares ou confirmares uma limpeza.
          Se o envio falhar, repetir o mesmo backup reutiliza ficheiros
          completos após verificar o conteúdo remoto. Sem deduplicação, a retoma
          é por ficheiro; ficheiros grandes usam multipart com cancelamento das
          partes numa falha. A compressão aplica-se aos novos envios. O
          fornecedor pode cobrar armazenamento e tráfego, incluindo a
          verificação. Transferir não restaura o mundo: escolhe depois Restaurar
          no histórico local, na mesma versão Minecraft. A aplicação não altera
          as permissões do bucket.
        </p>
      </details>
      {notice && <p role="status">{notice}</p>}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </article>
  );
}
