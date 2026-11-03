import { errorText } from "./lib/format";
import { formatDateTime } from "./lib/format";
import type { Plan } from "./api/models/RestoreRecoveryPanel";
import { useEffect, useState } from "react";
import { api, type Server } from "./api";
import { OreToggle } from "./OreToggle";

export function RestoreRecoveryPanel({
  server,
  onRecovered,
}: {
  server: Server;
  onRecovered: () => void;
}) {
  const [exists, setExists] = useState(false);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const stopped = ["offline", "error"].includes(server.state) && !server.pid;
  useEffect(() => {
    setPlan(null);
    setConfirmed(false);
    setNotice("");
    setError("");
  }, [server.id]);
  useEffect(() => {
    let closed = false;
    void api("restore_recovery_pending", { id: server.id })
      .then((v) => {
        if (!closed) setExists(v);
      })
      .catch((e) => {
        if (!closed) setError(errorText(e));
      });
    return () => {
      closed = true;
    };
  }, [server.id, server.state]);
  async function run(recover: boolean) {
    setBusy(true);
    setError("");
    try {
      if (recover && plan && confirmed) {
        const result = await api("recover_restore", {
          id: server.id,
          fingerprint: plan.fingerprint,
          confirmed,
        });
        setNotice(result.message);
        setExists(false);
        setPlan(null);
        onRecovered();
      } else {
        setPlan(await api("restore_recovery_plan", { id: server.id }));
        setConfirmed(false);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }
  if (!exists && !notice && !error) return null;
  return (
    <article className="connection-card restore-recovery-panel">
      <h3>{exists ? "Restauro interrompido" : "Recuperação do mundo"}</h3>
      {exists && (
        <>
          <p>
            O arranque está bloqueado. Podes repor a cópia anterior ao restauro;
            os ficheiros interrompidos serão preservados numa pasta separada. Se
            usavas outro disco ou pasta de backups, volta a ligá-lo e seleciona
            o destino original antes de verificar.
          </p>
          {!plan && (
            <button
              className="button secondary"
              disabled={busy || !stopped}
              onClick={() => void run(false)}
            >
              Verificar cópia de recuperação
            </button>
          )}
          {plan && (
            <>
              <p>
                Cópia de {formatDateTime(plan.safetyBackup.createdAt)} ·
                Minecraft {plan.safetyBackup.minecraftVersion} ·{" "}
                {plan.safetyBackup.fileCount} ficheiros verificados.
              </p>
              <label className="eula-check">
                <span>Repor o estado anterior ao restauro</span>
                <OreToggle
                  checked={confirmed}
                  onChange={(e) => setConfirmed(e.target.checked)}
                  disabled={busy}
                />
              </label>
              <div className="modal-actions">
                <button
                  className="button primary"
                  disabled={busy || !stopped || !confirmed}
                  onClick={() => void run(true)}
                >
                  Recuperar mundo
                </button>
              </div>
            </>
          )}
        </>
      )}
      {error && <p role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}
    </article>
  );
}
