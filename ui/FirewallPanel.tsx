import { errorText as message } from "./lib/format";
import type { Report } from "./api/models/FirewallPanel";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { useEffect, useState } from "react";
import { LoaderCircle, ShieldCheck, RefreshCw, X } from "lucide-react";
import { api, type Server } from "./api";

const titles: Record<string, string> = {
  aligned: "Regra LAN alinhada com o servidor",
  stale_ip: "A regra aponta para outro IP",
  missing: "Não foi encontrada uma regra LAN gerida para este servidor",
  different: "A regra precisa de ser revista",
  local_only: "O servidor está limitado a este PC",
  unsupported: "Verificação de firewall disponível apenas no Windows",
};

export function FirewallPanel({
  server,
  scope,
  address,
}: {
  server: Server;
  scope: string;
  address: string;
}) {
  const [report, setReport] = useState<Report | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [notice, setNotice] = useState("");
  useEffect(() => {
    setReport(null);
    setError("");
    setNotice("");
    setConfirm(false);
  }, [address, server.port]);
  async function inspect() {
    setWorking(true);
    setError("");
    setNotice("");
    try {
      setReport(await api("inspect_firewall", { id: server.id }));
    } catch (e) {
      setReport(null);
      setError(message(e));
    } finally {
      setWorking(false);
    }
  }
  async function repair() {
    if (!report) return;
    setWorking(true);
    setError("");
    try {
      setReport(
        await api("repair_firewall", {
          id: server.id,
          expected: report,
        }),
      );
      setConfirm(false);
      setNotice(
        "Regra lida novamente e confirmada. Volta a testar no outro dispositivo.",
      );
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking(false);
    }
  }
  const repairable =
    !!report && ["stale_ip", "missing", "different"].includes(report.state);
  return (
    <article className="connection-card firewall-card">
      <div className="players-heading">
        <h3>
          <ShieldCheck size={18} /> Firewall do Windows
        </h3>
        <button
          className="button secondary"
          disabled={working || scope !== "lan"}
          onClick={() => void inspect()}
        >
          {working ? (
            <LoaderCircle size={16} className="spin" />
          ) : (
            <RefreshCw size={16} />
          )}{" "}
          Verificar regra
        </button>
      </div>
      <p>
        {scope === "lan"
          ? "Deteta regras do Nethergate que ficaram com um IP antigo e prepara o acesso pela rede de casa."
          : "Seleciona primeiro um IP da rede de casa para verificar a regra LAN."}
      </p>
      {report && (
        <>
          <strong className="firewall-title">
            {titles[report.state] || "Estado não confirmado"}
          </strong>
          {report.previous?.localAddress && (
            <p>
              IP da regra: {report.previous.localAddress.join(", ")} · IP do
              servidor: {address}
            </p>
          )}
          {report.program && (
            <p className="firewall-path">Java: {report.program}</p>
          )}
          {repairable && (
            <button
              className="button primary"
              disabled={working}
              onClick={() => setConfirm(true)}
            >
              Preparar correção…
            </button>
          )}
        </>
      )}
      {working && (
        <p role="status">
          {confirm
            ? "A aguardar o Windows. Confirma o pedido de administrador para aplicar a correção."
            : "A ler a regra do Windows. Esta consulta não altera a firewall."}
        </p>
      )}
      <p className="players-hint">
        A verificação cobre a regra gerida pelo Nethergate. Outras regras ou
        políticas do Windows podem bloquear o acesso. A entrada a partir de
        outro dispositivo continua a ser o teste final.
      </p>
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
      <DialogPresence>
        {confirm && report && (
          <DialogBackdrop className="modal-backdrop">
            <section
              className="modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="firewall-confirm-title"
            >
              <button
                className="modal-close"
                aria-label="Cancelar correção de firewall"
                disabled={working}
                onClick={() => setConfirm(false)}
              >
                <X size={20} />
              </button>
              <h2 id="firewall-confirm-title">
                Permitir Minecraft na rede de casa?
              </h2>
              <p>
                Apenas TCP {report.port}, no IP {report.address}, pela interface{" "}
                {report.interface}, para a sub-rede local.
              </p>
              <p className="firewall-path">Executável: {report.program}</p>
              <p>
                A regra {report.ruleName} será criada ou atualizada. O estado
                anterior fica guardado. O router e as restantes regras não são
                alterados.
              </p>
              <div className="modal-actions">
                <button
                  className="button secondary"
                  disabled={working}
                  onClick={() => setConfirm(false)}
                >
                  Cancelar
                </button>
                <button
                  className="button primary"
                  disabled={working}
                  onClick={() => void repair()}
                >
                  {working ? (
                    <LoaderCircle size={16} className="spin" />
                  ) : (
                    <ShieldCheck size={16} />
                  )}{" "}
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
    </article>
  );
}
