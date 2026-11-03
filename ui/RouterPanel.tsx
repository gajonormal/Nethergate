import { errorText } from "./lib/format";
import { formatTime } from "./lib/format";
import type { Status, Plan } from "./api/models/RouterPanel";
import { OreToggle } from "./OreToggle";
import { useEffect, useState } from "react";
import { api, type Server } from "./api";

export function RouterPanel({
  server,
  onMapped,
}: {
  server: Server;
  onMapped?: (value: { externalPort: number; wan: string }) => void;
}) {
  const [status, setStatus] = useState<Status | null>(null);
  const [provider, setProvider] = useState("upnp");
  const [port, setPort] = useState(server.port);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [accepted, setAccepted] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  useEffect(() => {
    let closed = false;
    setPlan(null);
    setAccepted(false);
    setPort(server.port);
    async function refresh() {
      try {
        const v = await api("router_status", { id: server.id });
        if (!closed) setStatus(v);
      } catch (e) {
        if (!closed) setError(errorText(e));
      }
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => {
      closed = true;
      window.clearInterval(timer);
    };
  }, [server.id, server.port]);
  async function preview() {
    setPending(true);
    setError("");
    setPlan(null);
    setAccepted(false);
    try {
      setPlan(
        await api("router_preview", {
          id: server.id,
          provider,
          externalPort: port,
        }),
      );
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  async function change(remove: boolean) {
    setPending(true);
    setError("");
    setNotice("");
    try {
      const result = await api(remove ? "router_remove" : "router_create", {
        id: server.id,
        ticket: plan?.ticket,
      });
      setStatus(result);
      setPlan(null);
      setAccepted(false);
      setNotice(
        remove
          ? "Remoção confirmada. A firewall e o DNS mantêm as suas configurações."
          : "Mapeamento confirmado no router. Testa a ligação a partir de outra rede.",
      );
      if (!remove && result.externalPort && result.wan)
        onMapped?.({ externalPort: result.externalPort, wan: result.wan });
    } catch (e) {
      setError(errorText(e));
      setStatus(
        await api("router_status", { id: server.id }).catch(() => null),
      );
    } finally {
      setPending(false);
    }
  }
  const current = status && status.state !== "idle";
  return (
    <article className="connection-card router-panel">
      <h3>Encaminhar porta no router</h3>
      <p>
        UPnP ou NAT-PMP pode criar uma regra TCP temporária no gateway desta
        rede. Precisa do servidor a escutar no IP LAN, autenticação e whitelist
        com jogadores. A firewall Windows e o acesso externo são verificados
        separadamente.
      </p>
      {current ? (
        <>
          <p>
            <strong>
              {status.state === "mapped"
                ? "Mapeamento confirmado"
                : status.state === "review_required"
                  ? "Registo de sessão anterior — verificar antes de usar"
                  : status.state === "expired"
                    ? "Lease expirado"
                    : "Limpeza por confirmar"}
            </strong>
            <br />
            {status.provider} · porta externa {status.externalPort} →{" "}
            {status.internalPort} · gateway {status.gateway}
          </p>
          {status.address && (
            <p>
              Endereço indicado pelo router: <strong>{status.address}</strong>.
              Usa esta porta no DuckDNS e nos testes.
            </p>
          )}
          {!status.publicIp && (
            <p className="form-info">
              O router não indicou um IPv4 público utilizável. Este mapeamento
              pode não ultrapassar CGNAT/duplo NAT; usa o teste externo ou um
              túnel.
            </p>
          )}
          {status.expiresAt && (
            <p>
              Expiração indicada: {formatTime(status.expiresAt)}. A sessão ativa
              renova o lease enquanto as condições se mantêm.
            </p>
          )}
          {status.error && <p className="inline-error">{status.error}</p>}
          <button
            className="button secondary"
            disabled={pending}
            onClick={() => void change(true)}
          >
            Remover mapeamento Nethergate
          </button>
          {status.canForget && (
            <button
              className="button secondary"
              disabled={pending}
              onClick={() => {
                setPending(true);
                void api("router_forget_expired", { id: server.id })
                  .then((v) => {
                    setStatus(v);
                    setNotice(
                      "Registo expirado removido localmente. Não foi enviada uma ordem ao router.",
                    );
                  })
                  .catch((e) => setError(errorText(e)))
                  .finally(() => setPending(false));
              }}
            >
              Limpar registo expirado
            </button>
          )}
        </>
      ) : (
        <fieldset disabled={pending}>
          <div className="form-row">
            <label>
              Método
              <select
                value={provider}
                onChange={(e) => {
                  setProvider(e.target.value);
                  setPlan(null);
                }}
              >
                <option value="upnp">UPnP</option>
                <option value="nat_pmp">NAT-PMP</option>
              </select>
            </label>
            <label>
              Porta externa pretendida
              <input
                type="number"
                min={1}
                max={65535}
                value={port}
                onChange={(e) => {
                  setPort(Number(e.target.value));
                  setPlan(null);
                }}
              />
            </label>
          </div>
          <button
            className="button secondary"
            disabled={port < 1 || port > 65535}
            onClick={() => void preview()}
          >
            Descobrir e rever plano
          </button>
          {plan && (
            <div className="form-info">
              <p>
                {plan.provider} · TCP {plan.externalPort} → {plan.localAddress}:
                {plan.internalPort}
                <br />
                Gateway {plan.gateway} · WAN indicada {plan.wan} · lease pedido{" "}
                {plan.leaseSeconds} segundos.
              </p>
              <p>
                Esta etapa vai alterar o router. Não confirma que os amigos
                conseguem entrar. NAT-PMP pode atribuir outra porta; o resultado
                mostra a porta efetiva. A regra será removida ao parar o
                servidor; falhas de limpeza são apresentadas e a renovação é
                interrompida.
              </p>
              <label className="eula-check">
                <OreToggle
                  checked={accepted}
                  onChange={(e) => setAccepted(e.target.checked)}
                />
                <span>
                  Criar este encaminhamento TCP temporário para o servidor nesta
                  rede.
                </span>
              </label>
              <button
                className="button primary"
                disabled={!accepted || server.state !== "online"}
                onClick={() => void change(false)}
              >
                Criar mapeamento
              </button>
              {server.state !== "online" && (
                <p>
                  Inicia o servidor e revê novamente o plano antes de criar a
                  regra.
                </p>
              )}
            </div>
          )}
        </fieldset>
      )}
      {pending && <p role="status">A contactar o gateway desta interface…</p>}
      {notice && <p role="status">{notice}</p>}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </article>
  );
}
