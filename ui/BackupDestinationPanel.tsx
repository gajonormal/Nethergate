import { errorText } from "./lib/format";
import type { Destination } from "./api/models/BackupDestinationPanel";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, desktop, type Server } from "./api";

export function BackupDestinationPanel({
  server,
  onChange,
}: {
  server: Server;
  onChange: () => void;
}) {
  const [status, setStatus] = useState<Destination | null>(null);
  const [path, setPath] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let closed = false;
    api("backup_destination", { id: server.id })
      .then((v) => {
        if (!closed) {
          setStatus(v);
          setPath(v.base ?? "");
        }
      })
      .catch((e) => {
        if (!closed) setError(errorText(e));
      });
    return () => {
      closed = true;
    };
  }, [server.id, server.state]);
  async function pick() {
    try {
      const value = await invoke<string | null>("choose_data_folder");
      if (value) setPath(value);
    } catch (e) {
      setError(errorText(e));
    }
  }
  async function save(external: boolean) {
    setPending(true);
    setError("");
    try {
      setStatus(
        await api("save_backup_destination", {
          id: server.id,
          path: external ? path : null,
        }),
      );
      onChange();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  const disabled =
    pending || !["offline", "error"].includes(server.state) || !!server.pid;
  return (
    <article className="connection-card backup-destination-panel">
      <p>
        Guarda neste PC, num disco externo ou numa pasta sincronizada. A
        sincronização fica a cargo do serviço que utilizas.
      </p>
      {status && (
        <p className="backup-destination-current">
          <strong>
            {status.external ? "Destino externo" : "Armazenamento local"}
          </strong>
          <br />
          {status.path ?? status.base ?? status.localPath}
        </p>
      )}
      {status?.error && (
        <p className="inline-error" role="alert">
          {status.error}
        </p>
      )}
      <fieldset disabled={disabled}>
        <label>
          Pasta de destino
          <input
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="Pasta existente para guardar as cópias"
          />
        </label>
        <div className="modal-actions backup-destination-actions">
          {desktop && (
            <button className="button secondary" onClick={() => void pick()}>
              Escolher pasta…
            </button>
          )}
          <button
            className="button primary"
            disabled={!path.trim()}
            onClick={() => void save(true)}
          >
            Usar esta pasta
          </button>
          <button className="button secondary" onClick={() => void save(false)}>
            Repor destino local
          </button>
        </div>
      </fieldset>
      <details className="backup-help">
        <summary>O que acontece ao mudar de pasta?</summary>
        <p>
          Aplica-se aos próximos backups manuais, automáticos e cópias
          anteriores a alterações. O histórico não é movido nem apagado: volta
          ao armazenamento local para consultar as cópias locais. Voltar à mesma
          pasta externa após usar o armazenamento local recupera o seu
          histórico. Uma pasta externa diferente começa um histórico separado.
          Se o disco estiver desligado, o backup falha com um aviso. A retenção
          automática aplica-se apenas às cópias do destino ativo.
        </p>
      </details>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </article>
  );
}
