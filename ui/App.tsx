import { errorText } from "./lib/format";
import { LibraryPage } from "./LibraryPage";
import { useServerLibrary } from "./hooks/useServerLibrary";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { ApplicationMenu } from "./ApplicationMenu";
import { useEffect, useState } from "react";
import {
  ArrowRight,
  Check,
  CircleHelp,
  Copy,
  Globe2,
  Layers3,
  LockKeyhole,
  Settings2,
  Terminal,
  X,
  Users,
  Network,
  Package,
} from "lucide-react";
import { api, type Server } from "./api";
import { WindowChrome } from "./WindowChrome";

import { WorldPanel } from "./WorldPanel";
import { PluginsPanel } from "./PluginsPanel";
import { PlayersPanel } from "./PlayersPanel";
import { EnergyStatus } from "./EnergyStatus";
import { ConnectionPanel } from "./ConnectionPanel";
import { LibraryPanel } from "./LibraryPanel";
import { ServerMetrics } from "./ServerMetrics";
import { NotificationPanel } from "./NotificationPanel";
import { RecoveryPanel } from "./RecoveryPanel";
import { BackgroundPanel } from "./BackgroundPanel";

import { active } from "./server-state";
import { Badge } from "./ServerBadge";
import { ServerButton } from "./ServerButton";
import { Console } from "./ServerConsole";
import { ServerSettings } from "./ServerSettingsPanel";
import { CreateModal } from "./CreateServerDialog";

export function App() {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [creating, setCreating] = useState(false);
  const [help, setHelp] = useState(false);
  const [tab, setTab] = useState("console");
  const [pending, setPending] = useState("");
  const { servers, connected, loading, refresh, reset } =
    useServerLibrary(setError);
  const selected = servers.find((s) => s.id === selectedId);
  useEffect(() => {
    if (notice) {
      const timer = setTimeout(() => setNotice(""), 4500);
      return () => clearTimeout(timer);
    }
  }, [notice]);
  async function action(type: "start" | "stop" | "install", server: Server) {
    setPending(server.id);
    setError("");
    try {
      await api(type, { id: server.id });
      await refresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending("");
    }
  }
  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setNotice("Endereço copiado.");
    } catch {
      setNotice(`Endereço: ${text}`);
    }
  }
  function open(server: Server, section = "console") {
    setSelectedId(server.id);
    setTab(section);
    setError("");
  }

  return (
    <div
      className={`app-shell ${selected ? "server-workspace" : "library-workspace"}`}
    >
      <main>
        <nav className="creative-tabs" aria-label="Navegação principal">
          <button
            className={`creative-tab ${!selected ? "active" : ""}`}
            aria-current={!selected ? "page" : undefined}
            onClick={() => setSelectedId(null)}
            title="Servidores"
          >
            <Layers3 size={26} />
            <span>Servidores</span>
          </button>
          {selected &&
            [
              { id: "console", label: "Consola", icon: Terminal },
              { id: "world", label: "Mundo", icon: Globe2 },
              { id: "players", label: "Jogadores", icon: Users },
              {
                id: "plugins",
                label: selected.software === "fabric" ? "Mods" : "Plugins",
                icon: Package,
              },
              { id: "connection", label: "Amigos", icon: Network },
              { id: "settings", label: "Definições", icon: Settings2 },
            ].map(({ id, label, icon: Icon }) => (
              <button
                key={id}
                className={`creative-tab ${tab === id ? "active" : ""}`}
                aria-current={tab === id ? "page" : undefined}
                onClick={() => setTab(id)}
                title={id === "connection" ? "Jogar com amigos" : label}
              >
                <Icon size={26} />
                <span>{label}</span>
              </button>
            ))}
          <div className="side-controls">
            <div className="sidebar-bottom">
              <ApplicationMenu>
                <NotificationPanel />
                <LibraryPanel
                  busy={servers.some(active)}
                  onChanged={async () => {
                    reset();
                    setSelectedId(null);
                    setError("");
                    setTab("console");
                    await refresh();
                  }}
                />
                <BackgroundPanel />
                <button className="nav-item" onClick={() => setHelp(true)}>
                  <CircleHelp size={18} />
                  <span>Como começar</span>
                </button>
              </ApplicationMenu>
              <div className="engine">
                <span
                  className={`local-dot ${connected ? "" : "disconnected"}`}
                />
                <div>
                  {connected ? "Motor local ligado" : "A ligar ao motor"}
                  <small>v0.2.0</small>
                </div>
              </div>
            </div>
          </div>
        </nav>
        <div className="main-content">
          <WindowChrome onError={setError} />
          <EnergyStatus />
          {error && (
            <div className="error-banner" role="alert">
              <div>
                <strong>Não foi possível concluir</strong>
                <p>{error}</p>
              </div>
              <button aria-label="Fechar mensagem" onClick={() => setError("")}>
                <X size={17} />
              </button>
            </div>
          )}
          {!selected ? (
            <LibraryPage
              servers={servers}
              connected={connected}
              loading={loading}
              pending={pending}
              refresh={refresh}
              open={open}
              action={action}
              onCreate={() => setCreating(true)}
            />
          ) : (
            <>
              <div className="page-heading detail-heading">
                <div className="server-identity">
                  <h1 title={selected.name}>{selected.name}</h1>
                  <Badge state={selected.state} />
                  <span
                    className="server-runtime"
                    title={`${selected.memoryMb} MB de RAM${selected.pid ? ` · PID ${selected.pid}` : ""}`}
                  >
                    {selected.software === "fabric" ? "Fabric" : "Paper"}{" "}
                    {selected.version}
                  </span>
                </div>
                <ServerButton
                  server={selected}
                  pending={pending === selected.id}
                  onAction={(type) => void action(type, selected)}
                />
              </div>
              <div
                className={`workspace-body ${tab === "console" ? "console-workspace" : ""}`}
                key={`${selected.id}-${tab}`}
                role="region"
                aria-label="Conteúdo do servidor"
                tabIndex={0}
              >
                {tab === "console" && (
                  <div className="address-bar">
                    <div>
                      <span className="address-icon">
                        <LockKeyhole size={20} />
                      </span>
                      <div>
                        <small>ENDEREÇO NESTE COMPUTADOR</small>
                        <strong>
                          {selected.localAddress || "Endereço não reconhecido"}
                        </strong>
                      </div>
                    </div>
                    <button
                      className="button secondary button-copy"
                      aria-label="Copiar endereço"
                      title="Copiar endereço"
                      disabled={!selected.localAddress}
                      onClick={() => void copy(selected.localAddress!)}
                    >
                      <Copy size={15} />
                    </button>
                  </div>
                )}
                {selected.error && (
                  <div className="server-error">
                    <strong>O servidor precisa da tua atenção</strong>
                    <p>{selected.error}</p>
                  </div>
                )}
                {tab === "console" ? (
                  <>
                    <ServerMetrics server={selected} />
                    <RecoveryPanel server={selected} />
                    <Console server={selected} onError={setError} />
                  </>
                ) : tab === "world" ? (
                  <WorldPanel
                    key={selected.id}
                    server={selected}
                    onNotice={setNotice}
                  />
                ) : tab === "players" ? (
                  <PlayersPanel key={selected.id} server={selected} />
                ) : tab === "plugins" ? (
                  <PluginsPanel key={selected.id} server={selected} />
                ) : tab === "connection" ? (
                  <ConnectionPanel
                    key={selected.id}
                    server={selected}
                    onChanged={refresh}
                    onNavigate={setTab}
                  />
                ) : (
                  <ServerSettings
                    key={selected.id}
                    server={selected}
                    onSave={async (settings) => {
                      await api("settings", { id: selected.id, settings });
                      await refresh();
                      setNotice("Definições guardadas.");
                    }}
                  />
                )}
              </div>
            </>
          )}
          <footer>
            <span>
              Nethergate <span className="footer-version">PRÉVIA 0.2.0</span>
            </span>
            <span>Feito para os teus mundos.</span>
          </footer>
        </div>
      </main>
      {notice && (
        <div className="toast" role="status">
          <Check size={17} />
          {notice}
        </div>
      )}
      <DialogPresence>
        {creating && (
          <CreateModal
            servers={servers}
            onClose={() => setCreating(false)}
            onCreated={async (id) => {
              setCreating(false);
              await refresh();
              setSelectedId(id);
              setTab("console");
            }}
          />
        )}
      </DialogPresence>
      <DialogPresence>
        {help && (
          <DialogBackdrop
            className="modal-backdrop"
            onClick={() => setHelp(false)}
          >
            <section
              className="modal help-modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="help-title"
              onClick={(e) => e.stopPropagation()}
            >
              <button
                className="modal-close"
                aria-label="Fechar ajuda"
                onClick={() => setHelp(false)}
              >
                <X size={20} />
              </button>
              <div className="eyebrow">OS PRIMEIROS PASSOS</div>
              <h2 id="help-title">O teu próximo mundo, em minutos.</h2>
              <ol>
                <li>
                  <strong>Cria um servidor.</strong> Escolhe a versão, a RAM e
                  aceita a EULA do Minecraft. O PaperMC será descarregado e
                  verificado.
                </li>
                <li>
                  <strong>Carrega em Iniciar.</strong> Acompanha a consola até
                  aparecer o estado Online. O primeiro arranque pode demorar
                  enquanto prepara o mundo.
                </li>
                <li>
                  <strong>Entra no Minecraft Java.</strong> Usa a mesma versão
                  do servidor e liga-te ao endereço localhost mostrado no hub.
                </li>
                <li>
                  <strong>Para pelo hub.</strong> O Nethergate envia o comando
                  stop e espera que o mundo seja guardado.
                </li>
              </ol>
              <p className="form-note">
                O modo automático prepara Java 21 ou 25 no Windows x64. Os
                backups e o restauro ficam em Mundo; o diagnóstico e as
                permissões de rede ficam em Jogar com amigos. O playit permite
                partilhar sem configurar o router.
              </p>
              <button className="button primary" onClick={() => setHelp(false)}>
                Vamos a isso
                <ArrowRight size={16} />
              </button>
            </section>
          </DialogBackdrop>
        )}
      </DialogPresence>
    </div>
  );
}
