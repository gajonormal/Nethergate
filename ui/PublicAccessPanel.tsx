import type { Report } from "./api/models/PublicAccessPanel";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { useEffect, useState } from "react";
import { LoaderCircle, ShieldCheck, X } from "lucide-react";
import { api, type Server } from "./api";

const titles: Record<string, string> = {
  enabled: "Permissão pública do Windows ativa",
  disabled: "Permissão pública do Windows desativada",
  missing: "Sem permissão pública gerida para este servidor",
  unrecognized: "Existe uma regra que precisa de revisão manual",
  unsupported: "Gestão disponível apenas no Windows",
};
export function PublicAccessPanel({
  server,
  scope,
  address,
  authenticated,
  whitelist,
}: {
  server: Server;
  scope: string;
  address: string;
  authenticated: boolean;
  whitelist: boolean;
}) {
  const [report, setReport] = useState<Report | null>(null);
  const [working, setWorking] = useState(false);
  const [confirmation, setConfirmation] = useState<"enable" | "disable" | null>(
    null,
  );
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  useEffect(() => {
    setReport(null);
    setConfirmation(null);
    setNotice("");
    setError("");
  }, [server.id, server.port, address, authenticated, whitelist]);
  async function inspect() {
    setWorking(true);
    setError("");
    setNotice("");
    try {
      setReport(await api("public_access", { id: server.id }));
    } catch (e) {
      setReport(null);
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setWorking(false);
    }
  }
  async function apply() {
    if (!report || !confirmation) return;
    setWorking(true);
    setError("");
    try {
      setReport(
        await api("set_public_access", {
          id: server.id,
          operation: confirmation,
          expected: report,
        }),
      );
      setNotice(
        confirmation === "disable"
          ? "Permissões públicas geridas desativadas e verificadas. O encaminhamento no router mantém a configuração que escolheste."
          : "Permissão do Windows confirmada. Testa a entrada a partir de outra rede.",
      );
      setConfirmation(null);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setWorking(false);
    }
  }
  const recognized =
    report && ["enabled", "disabled", "missing"].includes(report.state);
  const canEnable =
    recognized &&
    !report.activeCount &&
    scope === "lan" &&
    authenticated &&
    whitelist &&
    !!report.desired.program &&
    !!report.desired.interface;
  return (
    <section className="connection-card firewall-card">
      <div className="players-heading">
        <h3>
          <ShieldCheck size={18} /> Gerir acesso público
        </h3>
        <button
          className="button secondary"
          disabled={working}
          onClick={() => void inspect()}
        >
          {working && <LoaderCircle size={16} className="spin" />} Verificar
          permissão
        </button>
      </div>
      <p>
        A permissão do Windows e o encaminhamento no router são configurações
        separadas. O estado do router não é consultado por este painel.
      </p>
      {report && (
        <>
          <strong className="firewall-title">
            {titles[report.state] || "Estado não confirmado"}
          </strong>
          {!!report.activeCount && !report.aligned && (
            <p className="inline-error">
              Há uma permissão ativa com outro destino. Desativa-a antes de
              mudar ou voltar a abrir o acesso.
            </p>
          )}
          {report.rules.map((rule) => (
            <p className="firewall-path" key={rule.name}>
              {rule.name} · {rule.enabled ? "Ativa" : "Desativada"} ·{" "}
              {rule.addresses.join(", ")}:{rule.ports.join(", ")}
            </p>
          ))}
          <div className="modal-actions">
            <button
              className="button primary"
              disabled={working || !canEnable}
              onClick={() => setConfirmation("enable")}
            >
              Permitir acesso público
            </button>
            <button
              className="button secondary"
              disabled={working || !recognized || !report.activeCount}
              onClick={() => setConfirmation("disable")}
            >
              Desativar permissão pública
            </button>
          </div>
          {!report.activeCount &&
            (!authenticated || !whitelist || scope !== "lan") && (
              <p>
                Para permitir acesso, escolhe um IP da rede de casa, mantém a
                autenticação oficial e ativa a whitelist com os jogadores
                autorizados.
              </p>
            )}
        </>
      )}
      <p className="field-hint">
        Desativar retira apenas as permissões públicas reconhecidas do
        Nethergate. Outras regras do Windows podem permitir ligações. Para
        retirar também o encaminhamento, desativa a entrada Minecraft no router.
      </p>
      {notice && (
        <p className="gameplay-success" role="status">
          {notice}
        </p>
      )}
      {error && !confirmation && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      <DialogPresence>
        {confirmation && report && (
          <DialogBackdrop className="modal-backdrop">
            <section
              className="modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="public-confirm-title"
            >
              <button
                className="modal-close"
                disabled={working}
                aria-label="Cancelar"
                onClick={() => setConfirmation(null)}
              >
                <X size={20} />
              </button>
              <h2 id="public-confirm-title">
                {confirmation === "enable"
                  ? "Permitir ligações da internet no Windows?"
                  : "Desativar a permissão pública?"}
              </h2>
              {confirmation === "enable" ? (
                <>
                  <p>
                    TCP {report.desired.port} → {report.desired.address},
                    interface {report.desired.interface}. Serão permitidas
                    origens de qualquer rede para este Java.
                  </p>
                  <p className="firewall-path">{report.desired.program}</p>
                  <p>
                    Será criada ou atualizada a regra NetherGate-Internet-
                    {server.id}.
                  </p>
                </>
              ) : (
                <>
                  <p>Serão desativadas estas regras:</p>
                  {report.rules
                    .filter((r) => r.enabled && r.owned)
                    .map((r) => (
                      <p className="firewall-path" key={r.name}>
                        {r.name}
                      </p>
                    ))}
                  <p>A regra da rede de casa mantém-se.</p>
                </>
              )}
              <p>
                O router mantém a configuração atual. O Windows pedirá
                autorização de administrador; o resultado será lido novamente.
              </p>
              <div className="modal-actions">
                <button
                  className="button secondary"
                  disabled={working}
                  onClick={() => setConfirmation(null)}
                >
                  Cancelar
                </button>
                <button
                  className="button primary"
                  disabled={working}
                  onClick={() => void apply()}
                >
                  {working && <LoaderCircle size={16} className="spin" />}{" "}
                  Confirmar e pedir ao Windows
                </button>
              </div>
              {error && (
                <p className="inline-error" role="alert">
                  {error}
                </p>
              )}
            </section>
          </DialogBackdrop>
        )}
      </DialogPresence>
    </section>
  );
}
