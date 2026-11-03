import { errorText } from "./lib/format";
import { usePolling } from "./hooks/usePolling";
import type { Recovery } from "./api/models/RecoveryPanel";
import { OreToggle } from "./OreToggle";
import { useState } from "react";
import { api, type Server } from "./api";

export function RecoveryPanel({ server }: { server: Server }) {
  const [data, setData] = useState<Recovery | null>(null);
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  usePolling(() => api("recovery", { id: server.id }), {
    interval: 3000,
    dependencies: [server.id],
    onReset: () => {
      setData(null);
      setError("");
    },
    onData: setData,
    onError: (error) => setError(errorText(error)),
  });
  async function save(enabled: boolean) {
    setPending(true);
    setError("");
    try {
      setData(await api("save_recovery", { id: server.id, enabled }));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  return (
    <details
      className="connection-card recovery-panel"
      open={data?.state.pending || undefined}
    >
      <summary>Recuperação automática {data?.enabled ? "· ativa" : ""}</summary>
      <p>
        Reinicia após uma falha, até três vezes, com esperas de 30, 60 e 120
        segundos. Falhas no primeiro arranque requerem intervenção manual.
      </p>
      {data && (
        <label className="checkbox-row">
          <OreToggle
            checked={data.enabled}
            disabled={pending}
            onChange={(e) => void save(e.target.checked)}
          />{" "}
          Tentar recuperar este servidor após uma falha
        </label>
      )}
      {data?.state.pending && (
        <p role="status">
          Tentativa {data.state.attempt} de {data.state.maximumAttempts} em
          cerca de {data.state.remainingSeconds} segundos. Usa Cancelar reinício
          para manter o servidor parado.
        </p>
      )}
      <p>
        As tentativas exigem que o motor continue ligado, mesmo com o painel
        fechado. Depois de recuperar, usa Partilhar servidor para voltar a ligar
        o túnel. Parar ou terminar o motor cancela as tentativas pendentes.
      </p>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </details>
  );
}
