import { errorText as message } from "./lib/format";
import type { Audience, Access } from "./api/models/PlayTogetherPanel";
import { useEffect, useRef, useState } from "react";
import {
  Check,
  Copy,
  Gamepad2,
  Globe2,
  LoaderCircle,
  LockKeyhole,
  Network,
  Play,
  Users,
} from "lucide-react";
import { api, type Server } from "./api";
import { preserveConnectionProbe, type Connection } from "./ConnectionPanel";
import { FirewallPanel } from "./FirewallPanel";
import { SharingPanel } from "./SharingPanel";
import { DdnsPanel } from "./DdnsPanel";
import { NetworkChangePanel } from "./NetworkChangePanel";
import { Disclosure } from "./Disclosure";

export function PlayTogetherPanel({
  server,
  onChanged,
  onNavigate,
  onOpenWan,
  onOpenListen,
  onObservedConnection,
  mappedPort,
}: {
  server: Server;
  onChanged: () => Promise<void>;
  onNavigate: (tab: string) => void;
  onOpenWan: () => void;
  onOpenListen: () => void;
  onObservedConnection: (connection: Connection) => void;
  mappedPort?: number;
}) {
  const [audience, setAudience] = useState<Audience>("internet");
  const [method, setMethod] = useState<"playit" | "duckdns" | "assisted">(
    () => {
      try {
        const saved = localStorage.getItem(`nethergate-address-${server.id}`);
        return saved === "duckdns" || saved === "assisted" ? saved : "playit";
      } catch {
        return "playit";
      }
    },
  );
  function chooseMethod(value: "playit" | "duckdns" | "assisted") {
    setMethod(value);
    try {
      localStorage.setItem(`nethergate-address-${server.id}`, value);
    } catch {
      /* The choice still works without persistent storage. */
    }
  }
  const [tunnelActive, setTunnelActive] = useState(false);
  const [connection, setConnection] = useState<Connection | null>(null);
  const [access, setAccess] = useState<Access | null>(null);
  const [chosen, setChosen] = useState("");
  const [name, setName] = useState("");
  const [pending, setPending] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const mounted = useRef(true);
  const busy = useRef(false);
  const revision = useRef(0);
  const online = server.state === "online";
  const stopped = ["offline", "error"].includes(server.state) && !server.pid;
  const ready =
    !!access?.authenticated && access.enabled && access.players.length > 0;

  async function refresh(diagnose = false) {
    const request = ++revision.current;
    const [nextConnection, nextAccess, share] = await Promise.all([
      api(diagnose ? "diagnose_connection" : "connection", {
        id: server.id,
      }),
      api("players", { id: server.id }),
      api("share_status", { id: server.id }),
    ]);
    if (!mounted.current || request !== revision.current) return;
    setConnection((current) =>
      diagnose
        ? nextConnection
        : preserveConnectionProbe(current, nextConnection),
    );
    onObservedConnection(nextConnection);
    setAccess(nextAccess);
    setTunnelActive(
      ["checking", "direct", "connecting", "tunnel", "recovering"].includes(
        share.state,
      ),
    );
    setChosen((current) =>
      nextConnection.adapters.some((a) => a.ip === current)
        ? current
        : nextConnection.adapters.find((a) => a.configured)?.ip ||
          nextConnection.adapters[0]?.ip ||
          "",
    );
  }
  useEffect(() => {
    mounted.current = true;
    let closed = false;
    let timer: number;
    const poll = async () => {
      if (!busy.current) {
        try {
          await refresh();
        } catch (e) {
          if (!closed) setError(message(e));
        }
      }
      if (!closed) timer = window.setTimeout(() => void poll(), 4000);
    };
    void poll();
    return () => {
      closed = true;
      mounted.current = false;
      ++revision.current;
      window.clearTimeout(timer);
    };
  }, [server.id, server.state, server.port, server.localAddress]);

  async function perform(label: string, operation: () => Promise<void>) {
    if (busy.current) return;
    busy.current = true;
    setPending(label);
    setError("");
    setNotice("");
    try {
      await operation();
    } catch (e) {
      if (mounted.current) setError(message(e));
    } finally {
      busy.current = false;
      if (mounted.current) setPending("");
    }
  }
  async function player(operation: string) {
    await api("player_operation", {
      id: server.id,
      operation,
      name: name.trim(),
    });
    if (mounted.current) setName("");
    await refresh();
  }
  async function configure(mode: "local" | "lan") {
    await api("set_connection", { id: server.id, mode, address: chosen });
    await refresh();
    await onChanged();
  }
  async function copy(address: string) {
    try {
      await navigator.clipboard.writeText(address);
      setNotice("Endereço copiado.");
    } catch {
      setNotice(`Copia este endereço: ${address}`);
    }
  }
  const configuredAdapter = connection?.adapters.find((a) => a.configured);
  const endpoint =
    audience === "local"
      ? connection?.localAddress
      : configuredAdapter?.address;
  const probe =
    audience === "local"
      ? connection?.probe
      : configuredAdapter?.address === connection?.probe?.address
        ? connection?.probe
        : configuredAdapter?.probe;

  return (
    <section className="settings-panel play-together">
      <div className="players-heading">
        <div>
          <h2>
            <Gamepad2 size={23} /> Jogar com amigos
          </h2>
        </div>
      </div>
      {connection && (
        <NetworkChangePanel
          connection={connection}
          server={server}
          working={!!pending}
          onStop={() =>
            void perform("stop-network", async () => {
              await api("stop", { id: server.id });
              await onChanged();
              await refresh();
            })
          }
          onOpenListen={onOpenListen}
          onOpenWan={onOpenWan}
          onDns={() => {
            setAudience("internet");
            chooseMethod("duckdns");
            window.setTimeout(
              () =>
                document
                  .getElementById(`dns-${server.id}`)
                  ?.scrollIntoView({ behavior: "smooth", block: "start" }),
              0,
            );
          }}
        />
      )}
      <fieldset className="audience-picker" disabled={!!pending}>
        <legend>Onde vão jogar?</legend>
        {(
          [
            ["local", "Só neste computador", LockKeyhole],
            ["lan", "Na mesma rede", Network],
            ["internet", "Pela internet", Globe2],
          ] as const
        ).map(([value, label, Icon]) => (
          <label key={value} className={audience === value ? "selected" : ""}>
            <input
              type="radio"
              name={`audience-${server.id}`}
              value={value}
              checked={audience === value}
              onChange={() => {
                setAudience(value);
                setError("");
                setNotice("");
              }}
            />
            <Icon size={18} />
            {label}
          </label>
        ))}
      </fieldset>

      <Disclosure
        title="Servidor e jogadores"
        hint={online && ready ? "Pronto para partilhar" : "Rever preparação"}
      >
        <div className="together-preparation">
          <div className="together-checklist" aria-live="polite">
            <span>
              {online ? <Check size={16} /> : <Play size={16} />}
              {online
                ? "Servidor pronto"
                : server.state === "starting"
                  ? "O Minecraft está a iniciar…"
                  : "O servidor precisa de estar ligado"}
            </span>
            {connection && (
              <span>
                {connection.authenticated
                  ? "✓ Autenticação oficial ativa"
                  : "É necessário ativar a autenticação oficial"}
              </span>
            )}
            {audience === "internet" && access && (
              <span>
                {ready
                  ? `✓ Whitelist ativa · ${access.players.length} jogador(es)`
                  : "Adiciona os jogadores e ativa a whitelist"}
              </span>
            )}
          </div>
          {stopped && (
            <button
              className="button primary"
              disabled={
                !!pending ||
                !server.eulaAccepted ||
                (server.installed &&
                  connection?.network.bindState === "missing")
              }
              onClick={() =>
                void perform("start", async () => {
                  await api(server.installed ? "start" : "install", {
                    id: server.id,
                  });
                  await onChanged();
                })
              }
            >
              {pending === "start" ? (
                <LoaderCircle className="spin" size={16} />
              ) : (
                <Play size={16} />
              )}
              {server.installed ? "Iniciar servidor" : "Preparar servidor"}
            </button>
          )}
          {stopped && connection?.network.bindState === "missing" && (
            <p className="form-info">
              Revê o IP local antes de iniciar: o endereço configurado já não
              está numa interface da rede atual.
              <button
                type="button"
                className="button secondary"
                onClick={onOpenListen}
              >
                Escolher IP da rede atual
              </button>
            </p>
          )}
          {server.state === "starting" && (
            <p role="status">
              <LoaderCircle size={16} className="spin" /> Aguarda a mensagem de
              prontidão do Minecraft.
            </p>
          )}
          {server.state === "installing" && (
            <p role="status">
              <LoaderCircle size={16} className="spin" /> A preparar os
              ficheiros. Quando terminar, escolhe Iniciar servidor.
            </p>
          )}
          {server.error && <p className="inline-error">{server.error}</p>}
          {!server.eulaAccepted && (
            <p className="inline-error">
              É necessário aceitar a EULA ao configurar o servidor.
            </p>
          )}
          {connection && !connection.authenticated && (
            <p className="form-info">
              Com o servidor parado, repõe online-mode=true no ficheiro
              server.properties.
              <button
                className="button secondary"
                onClick={() => onNavigate("console")}
              >
                Abrir consola
              </button>
            </p>
          )}

          {audience === "internet" && access && (
            <div className="together-access">
              <h4>
                <Users size={17} /> Quem pode entrar
              </h4>
              {!!access.players.length && (
                <p>
                  Autorizados: {access.players.map((p) => p.name).join(", ")}
                </p>
              )}
              <form
                className="players-add"
                onSubmit={(e) => {
                  e.preventDefault();
                  void perform("player", () => player("add"));
                }}
              >
                <label>
                  Nome no Minecraft
                  <input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    required
                    minLength={3}
                    maxLength={16}
                    pattern="[A-Za-z0-9_]{3,16}"
                    placeholder="Nome do jogador"
                    disabled={!online || !!pending || !access.authenticated}
                  />
                </label>
                <button
                  className="button secondary"
                  disabled={
                    !online ||
                    !!pending ||
                    !access.authenticated ||
                    !/^[A-Za-z0-9_]{3,16}$/.test(name.trim())
                  }
                >
                  Adicionar jogador
                </button>
              </form>
              {!online && <p>Inicia o servidor para adicionar jogadores.</p>}
              {!access.enabled && (
                <button
                  className="button primary"
                  disabled={
                    !online ||
                    !!pending ||
                    !access.authenticated ||
                    !access.players.length
                  }
                  onClick={() =>
                    void perform("whitelist", () => player("enable"))
                  }
                >
                  Ativar whitelist
                </button>
              )}
              <button
                className="button secondary"
                onClick={() => onNavigate("players")}
              >
                Gerir jogadores
              </button>
            </div>
          )}
        </div>
      </Disclosure>

      {connection && audience === "internet" && (
        <div className="together-sharing">
          <fieldset className="audience-picker" disabled={!!pending}>
            <legend>Ligação pela internet</legend>
            {(
              [
                ["playit", "Túnel playit"],
                ["duckdns", "Nome DuckDNS"],
                ["assisted", "Ligação assistida"],
              ] as const
            ).map(([value, label]) => (
              <label key={value} className={method === value ? "selected" : ""}>
                <input
                  type="radio"
                  name={`method-${server.id}`}
                  value={value}
                  checked={method === value}
                  onChange={() => chooseMethod(value)}
                />
                {label}
              </label>
            ))}
          </fieldset>
          {method === "duckdns" ? (
            <>
              {tunnelActive && (
                <p className="form-info">
                  Existe uma partilha ou preparação em curso. Escolher DuckDNS
                  não termina essa operação.
                  <button
                    type="button"
                    className="button secondary"
                    onClick={() => chooseMethod("playit")}
                  >
                    Gerir partilha
                  </button>
                </p>
              )}
              <DdnsPanel
                mappedPort={mappedPort}
                key={server.id}
                server={server}
                onOpenWan={onOpenWan}
                networkRevision={connection.network.revision}
              />
            </>
          ) : (
            <>
              {!ready && (
                <p className="form-info">
                  Revê Servidor e jogadores antes de partilhar.
                </p>
              )}
              {(!["local", "lan"].includes(connection.scope) ||
                (connection.scope === "lan" && !configuredAdapter)) && (
                <p className="form-info">
                  O IP atual precisa de ser revisto. Para usar o playit neste
                  PC, para o servidor e escolhe o acesso local.
                  <button
                    className="button secondary"
                    disabled={!stopped || !!pending}
                    onClick={() =>
                      void perform("local", () => configure("local"))
                    }
                  >
                    Usar acesso local para playit
                  </button>
                </p>
              )}
              <SharingPanel
                smart={method === "assisted"}
                onOpenWan={onOpenWan}
                server={server}
                networkRevision={connection.network.revision}
                authenticated={access?.authenticated ?? false}
                whitelist={ready}
              />
            </>
          )}
        </div>
      )}

      {connection && audience !== "internet" && (
        <div className="internet-step">
          <h3>Endereço para jogar</h3>
          {audience === "local" && (
            <>
              <p>Usa o endereço abaixo no Minecraft Java deste computador.</p>
              {connection.scope !== "local" && (
                <p>
                  O servidor também aceita ligações noutras interfaces. Para o
                  limitar a este PC, para-o e aplica esta opção.
                </p>
              )}
            </>
          )}
          {audience === "lan" && (
            <>
              <p>
                Os amigos precisam de estar na mesma rede. Escolhe o IP com o
                servidor parado; a firewall é revista separadamente.
              </p>
              <label>
                Ligação de rede
                <select
                  value={chosen}
                  disabled={!!pending || !connection.adapters.length}
                  onChange={(e) => setChosen(e.target.value)}
                >
                  {!connection.adapters.length && (
                    <option value="">Sem interface disponível</option>
                  )}
                  {connection.adapters.map((a) => (
                    <option key={a.ip} value={a.ip}>
                      {a.name} · {a.ip}
                    </option>
                  ))}
                </select>
              </label>
              {!stopped && !configuredAdapter && (
                <p>Para o servidor para escolher o IP da rede.</p>
              )}
              {connection.discoveryError && (
                <p className="inline-error">{connection.discoveryError}</p>
              )}
            </>
          )}
          <div className="together-address-actions">
            {audience === "local" ? (
              <button
                className="button secondary"
                disabled={!stopped || !!pending || connection.scope === "local"}
                onClick={() => void perform("local", () => configure("local"))}
              >
                Limitar a este computador
              </button>
            ) : (
              <button
                className="button secondary"
                disabled={
                  !stopped || !!pending || !chosen || !connection.authenticated
                }
                onClick={() => void perform("lan", () => configure("lan"))}
              >
                Permitir ligações neste IP
              </button>
            )}
            {endpoint && (
              <div className="connection-address">
                <code>{endpoint}</code>
                <button
                  className="button secondary button-copy"
                  aria-label="Copiar endereço"
                  title="Copiar endereço"
                  disabled={!online}
                  onClick={() => void copy(endpoint)}
                >
                  <Copy size={16} />
                </button>
              </div>
            )}
            <button
              className="button primary"
              disabled={!!pending || !online}
              onClick={() => void perform("diagnose", () => refresh(true))}
            >
              {pending === "diagnose" && (
                <LoaderCircle size={16} className="spin" />
              )}{" "}
              Testar Minecraft neste PC
            </button>
          </div>
          {audience === "lan" && (
            <FirewallPanel
              server={server}
              scope={connection.scope}
              address={connection.bindAddress}
            />
          )}
          {probe && (
            <p
              role="status"
              className={
                probe.status === "minecraft"
                  ? "gameplay-success"
                  : "inline-error"
              }
            >
              {probe.status === "minecraft"
                ? `Minecraft respondeu · ${probe.version}`
                : probe.detail}
            </p>
          )}
          <p>
            No Minecraft Java {server.version}: Multijogador → Ligação direta →
            cola o endereço.
          </p>
          {server.software === "fabric" && (
            <p>
              Os jogadores precisam do Fabric e dos mods correspondentes.
              Exporta o pacote no separador Mods.
            </p>
          )}
          {audience === "lan" && (
            <p>
              A resposta neste PC não confirma a ligação de outro dispositivo.
              Pede a um amigo para entrar usando este endereço.
            </p>
          )}
        </div>
      )}
      {!connection && !error && <p role="status">A verificar o servidor…</p>}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}
