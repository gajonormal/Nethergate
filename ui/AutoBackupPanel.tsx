import { errorText } from "./lib/format";
import { formatDateTime } from "./lib/format";
import { usePolling } from "./hooks/usePolling";
import type { Policy, Status } from "./api/models/AutoBackupPanel";
import { OreToggle } from "./OreToggle";
import { useRef, useState } from "react";
import { LoaderCircle } from "lucide-react";
import { api, type Server } from "./api";

const phases: Record<string, string> = {
  disabled: "Backups automáticos desligados",
  running: "A criar uma cópia com o mundo parado",
  waiting_world: "À espera de o servidor criar o primeiro mundo",
  waiting_stop:
    "Prazo atingido: a cópia será feita após a próxima paragem normal",
  blocked: "Resolve o erro do servidor e volta a iniciar/parar para retomar",
  due: "À espera de uma oportunidade para criar a cópia",
  scheduled: "Agendamento ativo",
};
export function AutoBackupPanel({
  server,
  onHistoryChange,
}: {
  server: Server;
  onHistoryChange: () => void;
}) {
  const [status, setStatus] = useState<Status | null>(null);
  const [policy, setPolicy] = useState<Policy>({
    enabled: false,
    intervalMinutes: 60,
    keep: 7,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const lastCopy = useRef<string | null>(null);
  usePolling(() => api("backup_schedule", { id: server.id }), {
    interval: 10000,
    dependencies: [server.id],
    onReset: () => {
      lastCopy.current = null;
    },
    onData: (value, initial) => {
      setStatus(value);
      if (initial) setPolicy(value.schedule.policy);
      if (value.schedule.lastBackupId !== lastCopy.current) {
        lastCopy.current = value.schedule.lastBackupId;
        onHistoryChange();
      }
    },
    onError: (error) => setError(errorText(error)),
  });
  async function save() {
    setSaving(true);
    setError("");
    setNotice("");
    try {
      setStatus(await api("save_backup_schedule", { id: server.id, policy }));
      setNotice(
        policy.enabled
          ? "Agendamento guardado. As cópias são feitas com o mundo parado."
          : "Agendamento desligado. Uma cópia que já esteja em curso termina normalmente.",
      );
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  }
  return (
    <article className="auto-backup-panel connection-card">
      <p>
        Cria cópias após cada paragem e por intervalo com o mundo parado. Requer
        o motor ligado; nunca desliga o servidor para fazer um backup.
      </p>
      {status && (
        <>
          <strong>{phases[status.phase] || "Estado por confirmar"}</strong>
          <p>
            {status.schedule.lastSuccess
              ? `Última cópia automática: ${formatDateTime(status.schedule.lastSuccess)}`
              : "Ainda não foi criada uma cópia automática."}
          </p>
          {status.schedule.policy.enabled &&
            status.schedule.nextDue != null && (
              <p>
                Próximo prazo: {formatDateTime(status.schedule.nextDue)} · Com o
                mundo parado.
              </p>
            )}
          {status.schedule.error && (
            <p className="inline-error" role="status">
              {status.schedule.error} · Nova tentativa após cinco minutos, com o
              servidor parado.
            </p>
          )}
          {status.schedule.cleanupWarning && (
            <p className="inline-error" role="status">
              {status.schedule.cleanupWarning}
            </p>
          )}
        </>
      )}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <fieldset disabled={saving || !status}>
          <label className="eula-check">
            <OreToggle
              checked={policy.enabled}
              onChange={(e) =>
                setPolicy({ ...policy, enabled: e.target.checked })
              }
            />
            <span>Ativar backups automáticos</span>
          </label>
          <div className="form-row">
            <label>
              Intervalo (minutos)
              <input
                type="number"
                min={15}
                max={1440}
                required
                value={policy.intervalMinutes}
                onChange={(e) =>
                  setPolicy({
                    ...policy,
                    intervalMinutes: Number(e.target.value),
                  })
                }
              />
            </label>
            <label>
              Cópias automáticas a manter
              <input
                type="number"
                min={2}
                max={50}
                required
                value={policy.keep}
                onChange={(e) =>
                  setPolicy({ ...policy, keep: Number(e.target.value) })
                }
              />
            </label>
          </div>
          <p>
            Mantém {policy.keep} cópias automáticas após o próximo backup
            bem-sucedido. Backups manuais e anteriores a um restauro são
            preservados.
          </p>
          <button className="button primary">
            {saving && <LoaderCircle size={16} className="spin" />} Guardar
            agendamento
          </button>
        </fieldset>
      </form>
      {notice && (
        <p className="gameplay-success" role="status">
          {notice}
        </p>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </article>
  );
}
