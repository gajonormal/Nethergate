import { useRef, useState } from "react";
import {
  Gamepad2,
  LoaderCircle,
  LockKeyhole,
  MemoryStick,
  Plus,
  Server as ServerIcon,
  Terminal,
  X,
} from "lucide-react";
import { type Server } from "./api";

import nethergateLogo from "./assets/nethergate-logo.svg";

import { ServerTransferPanel } from "./ServerTransferPanel";

import { Badge } from "./ServerBadge";
import { ServerButton } from "./ServerButton";
import { WorldPreview } from "./WorldPreview";

export function LibraryPage({
  servers,
  connected,
  loading,
  pending,
  refresh,
  open,
  action,
  onCreate,
}: {
  servers: Server[];
  connected: boolean;
  loading: boolean;
  pending: string;
  refresh: () => Promise<void>;
  open: (server: Server) => void;
  action: (action: "start" | "stop" | "install", server: Server) => void;
  onCreate: () => void;
}) {
  const [serverSearch, setServerSearch] = useState("");
  const serverSearchInput = useRef<HTMLInputElement>(null);
  const running = servers.filter((s) => s.state === "online").length;
  const filteredServers = servers.filter((server) =>
    `${server.name} ${server.software} ${server.version}`
      .toLocaleLowerCase("pt-PT")
      .includes(serverSearch.trim().toLocaleLowerCase("pt-PT")),
  );
  return (
    <>
      <div className="page-heading">
        <div>
          <h1 className="library-brand" aria-label="Nethergate">
            <img className="nethergate-lockup" src={nethergateLogo} alt="" />
          </h1>
        </div>
        <div className="library-heading-actions">
          <button
            className="button primary"
            disabled={!connected}
            onClick={() => onCreate()}
          >
            <Plus size={18} />
            Novo servidor
          </button>
          <ServerTransferPanel onImported={() => void refresh()} />
        </div>
      </div>
      <div className="library-toolbar">
        <div
          className="server-search"
          role="search"
          aria-label="Pesquisa na biblioteca"
        >
          <svg
            className="pixel-search"
            width="20"
            height="20"
            viewBox="0 0 20 20"
            aria-hidden="true"
            shapeRendering="crispEdges"
          >
            <path
              fill="currentColor"
              fillRule="evenodd"
              d="M2 2h12v12H2V2Zm2 2v8h8V4H4Z"
            />
            <path fill="currentColor" d="M12 12h3v3h3v3h-3v-3h-3z" />
          </svg>
          <input
            ref={serverSearchInput}
            type="search"
            aria-label="Procurar servidores"
            placeholder="Procurar servidores…"
            value={serverSearch}
            onChange={(event) => setServerSearch(event.target.value)}
          />
          {serverSearch && (
            <button
              type="button"
              className="search-clear"
              aria-label="Limpar pesquisa"
              onClick={() => {
                setServerSearch("");
                serverSearchInput.current?.focus();
              }}
            >
              <X size={16} />
            </button>
          )}
        </div>
        <span className="library-status" role="status">
          <span className={`local-dot ${connected ? "" : "disconnected"}`} />
          {running} / {servers.length} online
        </span>
      </div>
      <section
        className="library"
        aria-label="Biblioteca de servidores"
        tabIndex={0}
      >
        {loading ? (
          <div className="loading">
            <LoaderCircle className="spin" />A ligar ao motor local…
          </div>
        ) : !connected ? (
          <div className="empty-state">
            <ServerIcon size={30} />
            <h3>O motor ainda não está disponível</h3>
            <p>
              Verifica a mensagem acima. A biblioteca volta a aparecer quando a
              ligação for restabelecida.
            </p>
            <button className="button secondary" onClick={() => void refresh()}>
              Tentar novamente
            </button>
          </div>
        ) : (
          <div className="server-grid">
            {serverSearch.trim() && !filteredServers.length && (
              <p className="search-empty" role="status">
                Nenhum servidor corresponde a “{serverSearch}”.
              </p>
            )}
            {filteredServers.map((server, index) => (
              <article className="server-card" key={server.id}>
                <div className={`world-art art-${index % 3}`}>
                  <WorldPreview key={server.directory} id={server.id} />
                  <span className="world-label">
                    <Gamepad2 size={13} />
                    MINECRAFT JAVA
                  </span>
                  <Badge state={server.state} />
                </div>
                <div className="server-card-body">
                  <div className="server-card-title">
                    <h2>{server.name}</h2>
                  </div>
                  <p className="server-subtitle">
                    {server.software === "fabric" ? "Fabric" : "Paper"}{" "}
                    {server.version}
                    <span>·</span>
                    {server.installed ? "Instalado" : "Por instalar"}
                  </p>
                  <div className="server-specs">
                    <span>
                      <MemoryStick size={14} />
                      {(server.memoryMb / 1024).toLocaleString("pt-PT", {
                        maximumFractionDigits: 1,
                      })}{" "}
                      GB RAM
                    </span>
                    <span>
                      <LockKeyhole size={13} />
                      Local · {server.port}
                    </span>
                  </div>
                  <div className="card-actions">
                    <button
                      className="button secondary"
                      onClick={() => open(server)}
                    >
                      <Terminal size={15} />
                      Abrir
                    </button>
                    <ServerButton
                      server={server}
                      pending={pending === server.id}
                      onAction={(type) => void action(type, server)}
                    />
                  </div>
                </div>
              </article>
            ))}
            {servers.length === 0 && (
              <button className="new-card" onClick={() => onCreate()}>
                <span>
                  <Plus size={24} />
                </span>
                <strong>Uma nova aventura</strong>
                <p>Cria o teu próximo servidor</p>
              </button>
            )}
          </div>
        )}
      </section>
    </>
  );
}
