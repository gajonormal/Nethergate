import { formatTime } from "./lib/format";
import type { Connection } from "./ConnectionPanel";
import type { Server } from "./api";

export function NetworkChangePanel({
  connection,
  server,
  working,
  onStop,
  onOpenListen,
  onOpenWan,
  onDns,
}: {
  connection: Connection;
  server: Server;
  working: boolean;
  onStop: () => void;
  onOpenListen: () => void;
  onOpenWan: () => void;
  onDns: () => void;
}) {
  const network = connection.network;
  if (!network.supported) return null;
  const missing = network.bindState === "missing";
  if (
    !missing &&
    !network.changedAt &&
    !network.error &&
    !network.checking &&
    network.hasLocalNetwork !== false
  )
    return null;
  return (
    <aside className="network-change form-info" aria-label="Mudança de rede">
      <h3>
        {missing
          ? "O IP do servidor já não está nesta rede"
          : network.error
            ? "Não foi possível observar a rede"
            : network.checking
              ? "A confirmar uma alteração de rede…"
              : network.hasLocalNetwork === false
                ? "Sem interface IPv4 de rede de casa detetada"
                : "A rede deste PC mudou"}
      </h3>
      <div role="status">
        {missing && (
          <p>
            O IP {connection.bindAddress} já não está disponível. Com o servidor
            parado, escolhe um IP desta rede antes de iniciar.
          </p>
        )}
        {network.error && (
          <p>{network.error} A ligação e as permissões ficam por confirmar.</p>
        )}
        {network.changedAt && (
          <p>
            Alteração confirmada às {formatTime(network.changedAt)}. Os
            diagnósticos anteriores foram invalidados; isto não confirma mudança
            do IP WAN ou acessibilidade pública.
          </p>
        )}
        {network.hasLocalNetwork === false && (
          <p>
            Verifica o cabo ou Wi-Fi. A deteção observa interfaces
            Ethernet/Wi-Fi com IPv4 privado e gateway; não prova que a internet
            esteja desligada.
          </p>
        )}
      </div>
      <details className="network-change-help">
        <summary>O que rever depois de mudar de rede?</summary>
        <ol>
          <li>
            Revê o IP local onde o Minecraft escuta. Alterações exigem servidor
            parado.
          </li>
          <li>
            Para ligação direta, revê o encaminhamento no router e as permissões
            de firewall.
          </li>
          <li>
            Revê o nome DuckDNS e testa novamente a entrada por outra rede.
            Atualizações automáticas só usam a autorização já guardada.
          </li>
        </ol>
        <small>
          A observação local confirma duas amostras, a cada três segundos. Não
          altera o IP de escuta, router, firewall ou túnel.
        </small>
      </details>
      <div className="network-change-actions">
        {missing && server.state === "online" && (
          <button
            type="button"
            className="button secondary"
            disabled={working}
            onClick={onStop}
          >
            Parar e guardar para rever o IP
          </button>
        )}
        <button
          type="button"
          className="button secondary"
          onClick={onOpenListen}
        >
          Rever IP local
        </button>
        <button type="button" className="button secondary" onClick={onOpenWan}>
          Rever router e firewall WAN
        </button>
        <button type="button" className="button secondary" onClick={onDns}>
          Rever DuckDNS
        </button>
      </div>
    </aside>
  );
}
