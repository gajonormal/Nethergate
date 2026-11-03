import { errorText as message, formatTime, formatDateTime } from "./lib/format";
import type { PublicIp, ExternalTest } from "./api/models/InternetPanel";
import { OreToggle } from "./OreToggle";
import { useState } from "react";
import { Copy, Globe2, LoaderCircle, RefreshCw } from "lucide-react";
import { api, type Server } from "./api";
import { PublicAccessPanel } from "./PublicAccessPanel";
import { RouterPanel } from "./RouterPanel";

export function InternetPanel({
  server,
  scope,
  address,
  authenticated,
  whitelist,
  onMappedPort,
}: {
  server: Server;
  scope: string;
  address: string;
  authenticated: boolean;
  whitelist: boolean;
  onMappedPort?: (port: number) => void;
}) {
  const [publicIp, setPublicIp] = useState<PublicIp | null>(null);
  const [port, setPort] = useState(server.port);
  const [wan, setWan] = useState("");
  const [wanHint, setWanHint] = useState("");
  const [result, setResult] = useState<ExternalTest | null>(null);
  const [pending, setPending] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [consent, setConsent] = useState(false);
  const validPort = Number.isInteger(port) && port > 0 && port <= 65535;
  async function lookup() {
    setPending("ip");
    setError("");
    setResult(null);
    setWanHint("");
    setConsent(false);
    setNotice("");
    try {
      setPublicIp(await api("internet_ip", { id: server.id, consent: true }));
    } catch (e) {
      setPublicIp(null);
      setError(message(e));
    } finally {
      setPending("");
    }
  }
  async function test() {
    if (!publicIp || !consent) return;
    setPending("test");
    setError("");
    setResult(null);
    try {
      setResult(
        await api("internet_test", {
          id: server.id,
          expectedIp: publicIp.ip,
          port,
          consent,
        }),
      );
    } catch (e) {
      setError(message(e));
    } finally {
      setPending("");
    }
  }
  async function compare() {
    setPending("wan");
    setError("");
    setWanHint("");
    try {
      const result = await api("internet_wan", {
        id: server.id,
        wan,
      });
      setWanHint(result.state);
    } catch (e) {
      setError(message(e));
    } finally {
      setPending("");
    }
  }
  async function copy() {
    if (!publicIp) return;
    const endpoint = `${publicIp.ip}:${port}`;
    try {
      await navigator.clipboard.writeText(endpoint);
      setNotice(
        "Endereço para teste copiado. Ainda não confirma que esteja acessível.",
      );
    } catch {
      setNotice(`Endereço para teste: ${endpoint}`);
    }
  }
  return (
    <>
      <article className="connection-card internet-panel">
        <div className="players-heading">
          <h3>
            <Globe2 size={18} /> Preparar acesso pela internet
          </h3>
          <span className="connection-tag">Direto · router e firewall</span>
        </div>
        <p>
          O acesso na rede de casa já pode funcionar. Para chegar de outra rede,
          é preciso encaminhar a porta no router e permitir essa origem na
          firewall.
        </p>
        <details className="direct-network-option">
          <summary>Firewall · permitir acesso público</summary>
          <PublicAccessPanel
            server={server}
            scope={scope}
            address={address}
            authenticated={authenticated}
            whitelist={whitelist}
          />
        </details>
        <details className="direct-network-option">
          <summary>Router · encaminhamento UPnP ou NAT-PMP</summary>
          <RouterPanel
            server={server}
            onMapped={(value) => {
              setPort(value.externalPort);
              onMappedPort?.(value.externalPort);
              setWan(value.wan);
              setResult(null);
            }}
          />
        </details>
        <div className="internet-checklist">
          <span>
            {scope === "lan"
              ? "✓ IP da rede configurado"
              : "Escolhe primeiro um IP da rede de casa"}
          </span>
          <span>
            {authenticated
              ? "✓ Autenticação oficial ativa"
              : "É necessária autenticação oficial"}
          </span>
          <span>
            {whitelist
              ? "✓ Whitelist ativa"
              : "Adiciona os nomes e ativa a whitelist antes de partilhar"}
          </span>
          <span>
            {server.state === "online"
              ? "✓ Servidor online"
              : "Inicia o servidor para testar a entrada"}
          </span>
        </div>
        <div className="internet-step">
          <h4>1. Consultar o endereço público</h4>
          <p>
            O botão consulta api.ipify.org para descobrir o IPv4 observado nesta
            ligação. Uma VPN pode mudar o resultado. Não configura acesso ao
            servidor.
          </p>
          <button
            className="button secondary"
            disabled={!!pending}
            onClick={() => void lookup()}
          >
            {pending === "ip" ? (
              <LoaderCircle size={16} className="spin" />
            ) : (
              <RefreshCw size={16} />
            )}{" "}
            {publicIp ? "Atualizar IP público" : "Consultar IP público"}
          </button>
          {publicIp && (
            <div className="connection-address internet-public-address">
              <code>{publicIp.ip}</code>
              <small>Consulta às {formatTime(publicIp.checkedAt)}</small>
            </div>
          )}
        </div>
        <div className="internet-step">
          <h4>2. Preparar o encaminhamento no router</h4>
          <p>
            Na administração do router, procura Encaminhamento de portas. Estes
            são os valores para o teu servidor; ainda não foram aplicados pela
            aplicação.
          </p>
          <label>
            Porta externa TCP
            <input
              type="number"
              min={1}
              max={65535}
              step={1}
              value={port || ""}
              disabled={!!pending}
              onChange={(e) => {
                setPort(Number(e.target.value));
                setResult(null);
                setConsent(false);
                setNotice("");
              }}
            />
          </label>
          <dl className="internet-router-plan">
            <dt>Protocolo</dt>
            <dd>TCP</dd>
            <dt>Porta externa</dt>
            <dd>{validPort ? port : "Escolhe uma porta válida"}</dd>
            <dt>IP de destino no PC</dt>
            <dd>
              {scope === "lan" ? address : "Seleciona um IP da rede de casa"}
            </dd>
            <dt>Porta interna</dt>
            <dd>{server.port}</dd>
          </dl>
          <p>
            A permissão LAN do Nethergate está limitada à sub-rede local. Não é
            suficiente para ligações vindas da internet. A permissão WAN e a
            ativação do router precisam de uma confirmação própria.
          </p>
          {publicIp && (
            <>
              <label>
                IPv4 WAN mostrado no router (opcional)
                <input
                  value={wan}
                  placeholder="Endereço da ligação Internet/WAN, não o IP LAN"
                  disabled={!!pending}
                  onChange={(e) => {
                    setWan(e.target.value);
                    setWanHint("");
                  }}
                />
              </label>
              <button
                className="button secondary"
                disabled={!!pending || !wan.trim()}
                onClick={() => void compare()}
              >
                Comparar endereços
              </button>
              {wanHint && (
                <p className="form-info">
                  {wanHint === "same"
                    ? "O IP WAN coincide com o público observado. Isto não garante que a porta esteja acessível."
                    : wanHint === "upstream_nat"
                      ? "O router mostra um IP privado ou partilhado na ligação WAN. Pode existir NAT do operador ou outro router a montante; abrir uma porta neste router pode não bastar."
                      : wanHint === "different"
                        ? "Os endereços diferem. Confirma se há VPN, outra ligação ativa, mudança de IP ou NAT a montante. Isto, por si só, não confirma CGNAT."
                        : "Não foi fornecido um IP WAN."}
                </p>
              )}
            </>
          )}
        </div>
        <div className="internet-step">
          <h4>3. Testar a partir de fora</h4>
          <p>
            O serviço mcsrvstat.us pode consultar o IP e a porta a partir da sua
            infraestrutura. Os resultados podem estar em cache até cinco minutos
            e não confirmam autenticação nem entrada no mundo.
          </p>
          {publicIp && validPort && (
            <div className="connection-address">
              <code>
                {publicIp.ip}:{port}
              </code>
              <button
                className="button secondary button-copy"
                aria-label="Copiar endereço para teste"
                title="Copiar endereço para teste"
                onClick={() => void copy()}
              >
                <Copy size={16} />
              </button>
            </div>
          )}
          <label className="eula-check internet-consent">
            <OreToggle
              checked={consent}
              disabled={!!pending || !publicIp || !validPort}
              onChange={(e) => setConsent(e.target.checked)}
            />
            <span>
              Quero enviar este IP e porta ao mcsrvstat.us para o teste. O
              serviço poderá consultar informações públicas do servidor.
            </span>
          </label>
          <button
            className="button primary"
            disabled={!!pending || !publicIp || !validPort || !consent}
            onClick={() => void test()}
          >
            {pending === "test" ? (
              <LoaderCircle size={16} className="spin" />
            ) : (
              <Globe2 size={16} />
            )}{" "}
            Testar com serviço externo
          </button>
          {result && (
            <div
              className={`connection-result ${result.state === "reported_online" ? "success" : ""}`}
            >
              <strong>
                {result.state === "reported_online"
                  ? "O serviço externo reporta uma resposta Minecraft"
                  : result.state === "reported_offline"
                    ? "O serviço externo não reporta um servidor online"
                    : "Resposta externa inconclusiva"}
              </strong>
              <p>
                {result.address}
                {result.version ? ` · ${result.version}` : ""}
              </p>
              <p>
                {result.state === "reported_offline"
                  ? "Confirma servidor online, encaminhamento, firewall, IP e possíveis limitações do operador. Este resultado não identifica sozinho a causa."
                  : "Este resultado não identifica o servidor de forma autenticada nem confirma uma entrada no jogo."}
              </p>
              <small>
                {result.observedAt
                  ? `Observação do serviço: ${formatDateTime(result.observedAt * 1000)}. `
                  : "Hora original da observação não fornecida. "}
                Pode estar em cache até cinco minutos. A repetição nesse
                intervalo reutiliza o resultado.
              </small>
            </div>
          )}
          <p>
            O teste final é entrar pelo Pojav com o Wi-Fi desligado e dados
            móveis ativos, usando o endereço público e uma conta autorizada. Não
            uses o IP 192.168.x.x fora de casa.
          </p>
        </div>
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        {notice && (
          <p className="gameplay-success" role="status">
            {notice}
          </p>
        )}
      </article>
    </>
  );
}
