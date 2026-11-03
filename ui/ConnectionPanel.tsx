import { errorText as message, formatTime } from "./lib/format";
import type { Probe, Connection } from "./api/models/ConnectionPanel";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { useEffect, useRef, useState } from "react";
import {
  Check,
  Copy,
  LoaderCircle,
  LockKeyhole,
  Network,
  RefreshCw,
  X,
} from "lucide-react";
import { api, type Server } from "./api";
import { FirewallPanel } from "./FirewallPanel";
import { InternetPanel } from "./InternetPanel";
import { PlayTogetherPanel } from "./PlayTogetherPanel";

export type { Connection } from "./api/models/ConnectionPanel";
export function preserveConnectionProbe(
  current: Connection | null,
  next: Connection,
): Connection {
  if (
    next.testedAt !== null ||
    !current ||
    current.state !== next.state ||
    current.bindAddress !== next.bindAddress ||
    current.port !== next.port ||
    current.network.revision !== next.network.revision ||
    next.network.checking ||
    next.discoveryError ||
    JSON.stringify(current.adapters.map((a) => [a.name, a.ip, a.gateway])) !==
      JSON.stringify(next.adapters.map((a) => [a.name, a.ip, a.gateway]))
  )
    return next;
  return {
    ...next,
    probe: current.probe,
    portState: current.portState,
    testedAt: current.testedAt,
    adapters: next.adapters.map((a) => ({
      ...a,
      probe:
        current.adapters.find(
          (p) => p.ip === a.ip && p.configured === a.configured,
        )?.probe ?? null,
    })),
  };
}

function ProbeResult({ probe }: { probe: Probe | null }) {
  if (!probe)
    return <span className="connection-tag">Por testar neste PC</span>;
  return (
    <div
      className={`connection-result ${probe.status === "minecraft" ? "success" : ""}`}
    >
      <strong>
        {probe.status === "minecraft"
          ? `Minecraft respondeu · ${probe.version}`
          : probe.status === "tcp_only"
            ? "Porta aberta; Minecraft não confirmado"
            : "Sem resposta TCP"}
      </strong>
      <p>{probe.detail}</p>
      {probe.elapsedMs != null && (
        <small>
          {probe.elapsedMs} ms para ligar e obter o estado; não é uma medição do
          lag no jogo.
        </small>
      )}
    </div>
  );
}

export function ConnectionPanel({
  server,
  onChanged,
  onNavigate,
}: {
  server: Server;
  onChanged: () => Promise<void>;
  onNavigate: (tab: string) => void;
}) {
  const [data, setData] = useState<Connection | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [working, setWorking] = useState(false);
  const [chosen, setChosen] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [mappedPort, setMappedPort] = useState<number | undefined>();
  const generation = useRef(0);
  const [view, setView] = useState<"play" | "network">("play");
  const panel = useRef<HTMLDivElement>(null);
  function navigateView(next: "play" | "network") {
    setView(next);
    panel.current?.closest(".workspace-body")?.scrollTo({ top: 0 });
  }
  const wanSection = useRef<HTMLDetailsElement>(null);
  const listenSection = useRef<HTMLDetailsElement>(null);
  function openListen() {
    setView("network");
    window.requestAnimationFrame(() => {
      if (listenSection.current) {
        listenSection.current.open = true;
        listenSection.current.scrollIntoView({
          behavior: "smooth",
          block: "start",
        });
        listenSection.current.querySelector("summary")?.focus();
      }
    });
  }
  function openWan() {
    setView("network");
    window.requestAnimationFrame(() => {
      if (wanSection.current) {
        wanSection.current.open = true;
        wanSection.current.scrollIntoView({
          behavior: "smooth",
          block: "start",
        });
        wanSection.current.querySelector("summary")?.focus();
      }
    });
  }
  const stopped = ["offline", "error"].includes(server.state) && !server.pid;
  useEffect(() => {
    generation.current += 1;
    let closed = false;
    setData(null);
    setError("");
    api("connection", { id: server.id })
      .then((result) => {
        if (!closed) {
          setData(result);
          setChosen(
            result.adapters.find((a) => a.configured)?.ip ||
              result.adapters[0]?.ip ||
              "",
          );
        }
      })
      .catch((e) => {
        if (!closed) setError(message(e));
      });
    return () => {
      closed = true;
    };
  }, [server.id, server.state, server.port, server.localAddress]);
  async function diagnose() {
    const requestGeneration = generation.current;
    setWorking(true);
    setError("");
    setNotice("");
    try {
      const result = await api("diagnose_connection", {
        id: server.id,
      });
      if (requestGeneration !== generation.current) return;
      setData(result);
      if (!result.adapters.some((a) => a.ip === chosen))
        setChosen(result.adapters[0]?.ip || "");
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking(false);
    }
  }
  async function save(mode: string) {
    setWorking(true);
    setError("");
    setNotice("");
    try {
      await api("set_connection", { id: server.id, mode, address: chosen });
      setData(await api("connection", { id: server.id }));
      setConfirm(false);
      setNotice(
        "Ligação guardada. Inicia o servidor e executa o diagnóstico para testar.",
      );
      await onChanged();
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking(false);
    }
  }
  async function copy(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      setNotice("Endereço copiado.");
    } catch {
      setNotice(`Copia este endereço: ${value}`);
    }
  }
  return (
    <div className="connection-panel" ref={panel}>
      <nav className="friends-navigation" aria-label="Secções de Amigos">
        <button
          type="button"
          className={`button ${view === "play" ? "primary" : "secondary"}`}
          aria-pressed={view === "play"}
          onClick={() => navigateView("play")}
        >
          Jogar e partilhar
        </button>
        <button
          type="button"
          className={`button ${view === "network" ? "primary" : "secondary"}`}
          aria-pressed={view === "network"}
          onClick={() => navigateView("network")}
        >
          Rede e diagnóstico
        </button>
      </nav>
      <div hidden={view !== "play"}>
        <PlayTogetherPanel
          mappedPort={mappedPort}
          server={server}
          onChanged={onChanged}
          onNavigate={onNavigate}
          onOpenWan={openWan}
          onOpenListen={openListen}
          onObservedConnection={(result) =>
            setData((current) => preserveConnectionProbe(current, result))
          }
        />
      </div>
      <div className="connection-advanced" hidden={view !== "network"}>
        <section className="settings-panel">
          <div className="players-heading">
            <div>
              <h2>
                <Network size={23} /> Diagnóstico de rede
              </h2>
            </div>
            <button
              className="button primary"
              disabled={working}
              onClick={() => void diagnose()}
            >
              {working ? (
                <LoaderCircle size={16} className="spin" />
              ) : (
                <RefreshCw size={16} />
              )}{" "}
              Testar ligação
            </button>
          </div>
          {!data && !error && (
            <p className="players-hint">A ler a configuração de rede…</p>
          )}
          {data && (
            <>
              <div className="connection-facts">
                <span>
                  Modo:{" "}
                  <b>
                    {data.scope === "local"
                      ? "Só neste PC"
                      : data.scope === "lan"
                        ? "IP da rede de casa"
                        : data.scope === "all"
                          ? "Todas as interfaces"
                          : "IP personalizado"}
                  </b>
                </span>
                <span>
                  Autenticação:{" "}
                  <b>{data.authenticated ? "Oficial" : "Desativada"}</b>
                </span>
                <span>
                  Whitelist:{" "}
                  <b>{data.whitelistEnabled ? "Ativa" : "Desativada"}</b>
                </span>
              </div>
              <details className="network-section">
                <summary>
                  Diagnóstico local · portas e resposta Minecraft
                </summary>
                <article className="connection-card">
                  <h3>
                    <LockKeyhole size={18} /> Neste computador
                  </h3>
                  <div className="connection-address">
                    <code>{data.localAddress || "IP não reconhecido"}</code>
                    <button
                      className="button secondary button-copy"
                      aria-label="Copiar endereço"
                      title="Copiar endereço"
                      disabled={!data.localAddress}
                      onClick={() => void copy(data.localAddress!)}
                    >
                      <Copy size={16} />
                    </button>
                  </div>
                  <p>
                    No Minecraft Java, abre Multijogador → Ligação direta e usa
                    este endereço.
                  </p>
                  <ProbeResult probe={data.probe} />
                  {data.probe?.status === "minecraft" &&
                    !data.managedProcess && (
                      <p className="inline-error">
                        Há um Minecraft a responder, mas não é um processo
                        gerido por esta instância da aplicação.
                      </p>
                    )}
                  {data.portState !== "not_tested" && (
                    <p className="players-hint">
                      {data.portState === "available"
                        ? "A porta está disponível neste endereço para iniciar um servidor."
                        : data.portState === "occupied"
                          ? "A porta está ocupada neste endereço."
                          : "Não foi possível confirmar se a porta está disponível."}
                    </p>
                  )}
                  {!data.managedProcess && (
                    <p className="players-hint">
                      O teu servidor está parado. Inicia-o para testar a
                      resposta do Minecraft.
                    </p>
                  )}
                </article>
                <article className="connection-card">
                  <h3>
                    <Network size={18} /> Na mesma rede de casa
                  </h3>
                  <p>
                    Os amigos precisam de estar na mesma rede. Redes de
                    convidados e isolamento Wi-Fi podem impedir a ligação.
                  </p>
                  {data.adapters.length ? (
                    data.adapters.map((adapter) => (
                      <div className="connection-adapter" key={adapter.ip}>
                        <div>
                          <strong>{adapter.name}</strong>
                          <code>{adapter.address}</code>
                        </div>
                        <span className="connection-tag">
                          {adapter.configured
                            ? "IP configurado"
                            : "IP não selecionado"}
                        </span>
                        <button
                          className="button secondary button-copy"
                          aria-label="Copiar endereço"
                          title="Copiar endereço"
                          disabled={!adapter.configured}
                          onClick={() => void copy(adapter.address)}
                        >
                          <Copy size={16} />
                        </button>
                        {adapter.configured && (
                          <div className="connection-adapter-result">
                            <ProbeResult
                              probe={
                                adapter.address === data.probe?.address
                                  ? data.probe
                                  : adapter.probe
                              }
                            />
                          </div>
                        )}
                      </div>
                    ))
                  ) : (
                    <p className="players-hint">
                      Não foi encontrado um IP privado numa interface
                      Ethernet/Wi-Fi ativa com gateway. Liga-te à rede de casa e
                      volta a testar.
                    </p>
                  )}
                  {data.discoveryError && (
                    <p className="inline-error">{data.discoveryError}</p>
                  )}
                  <p className="connection-unverified">
                    Acesso a partir de outro PC: <strong>não confirmado</strong>
                    . Um teste feito nesta máquina não verifica a firewall nem a
                    ligação do amigo.
                  </p>
                </article>
              </details>
              <details className="network-section">
                <summary>Firewall da rede local</summary>
                <FirewallPanel
                  key={`${server.id}-${data.bindAddress}-${server.port}-${data.network.revision}`}
                  server={server}
                  scope={data.scope}
                  address={data.bindAddress}
                />
              </details>
              <details className="network-section" ref={wanSection}>
                <summary>
                  Ligação direta · router, firewall e teste externo
                </summary>
                <InternetPanel
                  onMappedPort={setMappedPort}
                  key={`internet-${server.id}-${data.bindAddress}-${server.port}-${data.network.revision}`}
                  server={server}
                  scope={data.scope}
                  address={data.bindAddress}
                  authenticated={data.authenticated}
                  whitelist={data.whitelistEnabled}
                />
              </details>
              <details className="network-section" ref={listenSection}>
                <summary>IP de escuta do Minecraft</summary>
                <div className="connection-settings">
                  <h3>Escolher o acesso</h3>
                  <p>
                    As alterações de IP aplicam-se no próximo arranque. A regra
                    de firewall pode ser verificada e corrigida no painel acima.
                  </p>
                  {!stopped && (
                    <div className="form-info">
                      Para o servidor antes de alterar o acesso.
                    </div>
                  )}
                  <div className="connection-options">
                    <button
                      className="button secondary"
                      disabled={!stopped || working || data.scope === "local"}
                      onClick={() => void save("local")}
                    >
                      <LockKeyhole size={16} /> Usar só neste PC
                    </button>
                    <label>
                      Interface da rede de casa
                      <select
                        value={chosen}
                        disabled={!stopped || working || !data.adapters.length}
                        onChange={(e) => setChosen(e.target.value)}
                      >
                        {!data.adapters.length && (
                          <option value="">Sem IP disponível</option>
                        )}
                        {data.adapters.map((a) => (
                          <option key={a.ip} value={a.ip}>
                            {a.name} · {a.ip}
                          </option>
                        ))}
                      </select>
                    </label>
                    <button
                      className="button primary"
                      disabled={
                        !stopped || working || !chosen || !data.authenticated
                      }
                      onClick={() => setConfirm(true)}
                    >
                      <Network size={16} /> Permitir neste IP…
                    </button>
                  </div>
                  {!data.authenticated && (
                    <p className="inline-error">
                      O acesso à rede exige autenticação oficial nesta versão.
                    </p>
                  )}
                  <p className="players-hint">
                    Se o IP do PC mudar, escolhe o novo endereço antes de
                    iniciar. As regras de firewall e de encaminhamento que já
                    existirem continuam a aplicar-se.
                  </p>
                </div>
              </details>
              {data.testedAt && (
                <p className="players-hint">
                  Diagnóstico feito neste PC às {formatTime(data.testedAt)}. Os
                  resultados podem mudar com a rede ou o estado do servidor.
                </p>
              )}
            </>
          )}
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
        </section>
      </div>
      <DialogPresence>
        {confirm && data && (
          <DialogBackdrop className="modal-backdrop">
            <section
              className="modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="connection-confirm-title"
            >
              <button
                className="modal-close"
                disabled={working}
                aria-label="Cancelar alteração de ligação"
                onClick={() => setConfirm(false)}
              >
                <X size={20} />
              </button>
              <h2 id="connection-confirm-title">
                Permitir ligações em {chosen}:{data.port}?
              </h2>
              <p>
                O Minecraft passará a aceitar ligações neste IP da rede de casa.
                O acesso depende da firewall e das regras que já existirem no
                router; a internet ainda não foi verificada.
              </p>
              <p>
                {data.whitelistEnabled
                  ? "A whitelist está ativa. Confirma que os nomes dos teus amigos estão autorizados."
                  : "A whitelist está desativada. Qualquer conta oficial que consiga chegar ao servidor poderá tentar entrar."}
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
                  disabled={working || !stopped}
                  onClick={() => void save("lan")}
                >
                  {working ? (
                    <LoaderCircle size={16} className="spin" />
                  ) : (
                    <Check size={16} />
                  )}{" "}
                  Permitir neste IP
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
    </div>
  );
}
