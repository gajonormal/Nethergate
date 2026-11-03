import { errorText as message } from "./lib/format";
import type { Inventory, Hit, Plan } from "./api/models/PluginsPanel";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { useEffect, useState } from "react";
import {
  Download,
  LoaderCircle,
  Package,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { api, type Server } from "./api";
import { PluginSolverPanel } from "./PluginSolverPanel";
import { ManualJarsPanel } from "./ManualJarsPanel";
import { Disclosure } from "./Disclosure";

export function PluginsPanel({ server }: { server: Server }) {
  const modded = server.software === "fabric";
  const kind = modded ? "mods" : "plugins";
  const engine = modded ? "Fabric" : "Paper";
  const side = (value?: string) =>
    ({
      required: "Necessário nos PCs dos jogadores",
      optional: "Opcional segundo o autor; pode ser exigido por outro mod",
      unsupported: "Apenas no servidor",
      unknown: "Necessidade no cliente por confirmar",
    })[value || "unknown"] || "Necessidade no cliente por confirmar";
  const [inventory, setInventory] = useState<Inventory | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [searched, setSearched] = useState(false);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [working, setWorking] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [solverProject, setSolverProject] = useState<string | null>(null);
  const stopped = ["offline", "error"].includes(server.state) && !server.pid;
  async function exportPack() {
    setWorking("export");
    setError("");
    setNotice("");
    try {
      const value = await api("export_modpack", { id: server.id });
      setNotice(
        `Pacote criado: ${value.path} · ${value.clientMods} mods para os jogadores. Envia este ficheiro .mrpack aos amigos para importarem num launcher compatível. A versão de Minecraft e o Fabric Loader ficam definidos no pacote. ${value.serverOnlyExcluded} mods exclusivos do servidor foram excluídos.`,
      );
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking("");
    }
  }
  async function load() {
    setInventory(await api("plugins", { id: server.id }));
  }
  useEffect(() => {
    void load().catch((e) => setError(message(e)));
  }, [server.id, server.state]);
  async function search() {
    setWorking("search");
    setError("");
    setNotice("");
    try {
      const value = await api("plugin_search", {
        id: server.id,
        query,
      });
      setHits(value.hits);
      setSearched(true);
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking("");
    }
  }
  async function preview(projectId: string) {
    setWorking(projectId);
    setError("");
    setNotice("");
    try {
      setPlan(await api("plugin_preview", { id: server.id, projectId }));
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking("");
    }
  }
  async function install() {
    if (!plan) return;
    setWorking("install");
    setError("");
    try {
      const result = await api("plugin_install", {
        id: server.id,
        projectId: plan.projectId,
        versionId: plan.versionId,
        fingerprint: plan.fingerprint,
      });
      setPlan(null);
      await load();
      setNotice(
        `${result.installed} ${result.version} instalado. Será carregado no próximo arranque. Cópia anterior: ${result.snapshot}${result.worldBackup ? " · Backup do mundo também guardado." : " · O mundo ainda não tinha sido criado."}`,
      );
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking("");
    }
  }
  return (
    <section className="plugins-panel settings-panel">
      <h2>{modded ? "Mods" : "Plugins"}</h2>
      <Disclosure
        title="JARs manuais"
        hint="Importar, identificar ou desativar ficheiros"
      >
        <ManualJarsPanel
          server={server}
          onChange={() => void load().catch((e) => setError(message(e)))}
        />
      </Disclosure>
      <p>
        Catálogo Modrinth · {engine} · Minecraft {server.version}.
      </p>
      {modded && (
        <Disclosure title="Pacote para os jogadores" hint="Exportar .mrpack">
          <article className="connection-card">
            <h3>O mesmo conjunto para os jogadores.</h3>
            <p>
              Os jogadores precisam da mesma versão de Minecraft, de Fabric
              Loader e dos mods assinalados como necessários no cliente. O
              pacote .mrpack fixa as versões e os hashes dos mods geridos; não
              inclui configurações privadas, mundos ou credenciais.
            </p>
            <button
              className="button secondary"
              disabled={!!working || !stopped || !!inventory?.pending}
              onClick={() => void exportPack()}
            >
              <Download size={16} /> Exportar pacote para os jogadores
            </button>
            <p>
              O pacote pode ser importado por um launcher com suporte .mrpack.
              No launcher oficial, instala Fabric separadamente e coloca os
              mesmos JARs em mods. Mods só de cliente, como gráficos, ficam fora
              do gestor do servidor.
            </p>
          </article>
        </Disclosure>
      )}
      {!stopped && (
        <p className="form-info">
          Podes consultar o catálogo. Para o servidor antes de instalar ou
          atualizar.
        </p>
      )}
      {inventory?.pending && (
        <p className="inline-error">
          Há uma alteração interrompida. Consulta a consola e recupera a cópia
          anterior antes de continuar.
        </p>
      )}
      <article className="connection-card plugin-inventory-card">
        <div className="players-heading">
          <h3>
            <Package size={18} /> Instalados pelo Nethergate
          </h3>
          <div className="plugin-inventory-actions">
            <button
              className="button secondary"
              disabled={!!working}
              onClick={() => void load().catch((e) => setError(message(e)))}
            >
              <RefreshCw size={16} /> Atualizar lista
            </button>
            <button
              className="button secondary"
              disabled={
                !!working ||
                !inventory?.installed.length ||
                !!inventory?.pending
              }
              onClick={() => setSolverProject("")}
            >
              Rever atualizações
            </button>
          </div>
        </div>
        {inventory && !inventory.installed.length && (
          <p>Ainda não há {kind} geridos neste servidor.</p>
        )}
        {inventory?.installed.map(({ plugin, intact }) => (
          <div className="plugin-row" key={plugin.projectId}>
            <div>
              <strong>{plugin.title}</strong>
              <p>
                Versão {plugin.versionNumber} ·{" "}
                {intact ? "JAR verificado" : "JAR alterado ou ausente"} ·{" "}
                {plugin.filename}
              </p>
              {modded && <p>{side(plugin.distribution?.clientSide)}</p>}
            </div>
            <button
              className="button secondary"
              disabled={!!working || !intact || inventory.pending}
              onClick={() => setSolverProject(plugin.projectId)}
            >
              Rever atualização e dependências
            </button>
          </div>
        ))}
        {!!inventory?.external.length && (
          <p>
            JARs fora do gestor: {inventory.external.join(", ")}. São
            preservados. Confirma que não instalas outra cópia do mesmo{" "}
            {modded ? "mod" : "plugin"}
            com um nome diferente.
          </p>
        )}
      </article>
      <form
        className="plugin-search"
        onSubmit={(e) => {
          e.preventDefault();
          void search();
        }}
      >
        <label>
          Procurar {kind}
          <input
            maxLength={100}
            placeholder={
              modded
                ? "Nome do mod, por exemplo Fabric API"
                : "Nome do plugin, por exemplo Chunky"
            }
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <button className="button primary" disabled={!!working}>
          {working === "search" ? (
            <LoaderCircle className="spin" size={16} />
          ) : (
            <Search size={16} />
          )}{" "}
          Pesquisar no Modrinth
        </button>
      </form>
      {searched && !hits.length && (
        <p>
          Sem resultados para esta versão. Tenta outro nome; não serão
          instaladas versões declaradas para outro Minecraft.
        </p>
      )}
      <div className="plugin-results">
        {hits.map((hit) => (
          <article className="connection-card" key={hit.projectId}>
            <h3 title={hit.title}>{hit.title}</h3>
            <p className="catalog-description" title={hit.description}>
              {hit.description}
            </p>
            <p className="catalog-meta">
              Por {hit.author} · {hit.downloads.toLocaleString("pt-PT")}{" "}
              downloads
            </p>
            <button
              className="button secondary"
              disabled={!!working}
              onClick={() => setSolverProject(hit.projectId)}
            >
              {working === hit.projectId && (
                <LoaderCircle size={16} className="spin" />
              )}{" "}
              Escolher versão
            </button>
          </article>
        ))}
      </div>
      <DialogPresence>
        {solverProject !== null && (
          <PluginSolverPanel
            server={server}
            project={solverProject}
            onClose={() => setSolverProject(null)}
            onInstalled={(text) => {
              setNotice(text);
              void load().catch((e) => setError(message(e)));
            }}
          />
        )}
      </DialogPresence>
      {notice && (
        <p className="gameplay-success" role="status">
          {notice}
        </p>
      )}
      {error && !plan && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      <DialogPresence>
        {plan && (
          <DialogBackdrop className="modal-backdrop">
            <section
              className="modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="plugin-review-title"
            >
              <button
                className="modal-close"
                disabled={working === "install"}
                aria-label="Fechar detalhes"
                onClick={() => setPlan(null)}
              >
                <X size={20} />
              </button>
              <h2 id="plugin-review-title">
                {plan.previous ? "Atualizar" : "Instalar"} {plan.title}?
              </h2>
              <p>
                Versão {plan.versionNumber} · {engine} / Minecraft{" "}
                {plan.minecraft} ·{" "}
                {(plan.size / 1048576).toLocaleString("pt-PT", {
                  maximumFractionDigits: 2,
                })}{" "}
                MB
              </p>
              <p>
                {plan.previous
                  ? `Substitui a versão ${plan.previous.versionNumber}.`
                  : `Adiciona um JAR novo à pasta ${kind}.`}{" "}
                Será executado pelo servidor no próximo arranque.
              </p>
              <p>
                O download vem do CDN Modrinth e será verificado por SHA-512.
                Antes de alterar ficheiros, o motor guarda os mundos existentes,
                a pasta {kind} e o registo do gestor. O restauro do separador
                Mundo recupera os mundos; a cópia da pasta {kind} fica separada.
              </p>
              <p>
                Compatibilidade declarada pelo autor; conflitos com outros{" "}
                {kind}
                podem surgir no arranque. Dependências obrigatórias em falta têm
                de ser instaladas primeiro.
              </p>
              {modded && (
                <p>
                  {side(plan.distribution.clientSide)}. Mods marcados apenas
                  para cliente são recusados no servidor. Instala primeiro
                  dependências obrigatórias, como Fabric API quando for pedida.
                </p>
              )}
              <p className="firewall-path">{plan.sourceFilename}</p>
              {plan.blockers.map((blocker, i) => (
                <p className="inline-error" key={i}>
                  {blocker}
                </p>
              ))}
              {plan.missingProjects.map((id) => (
                <button
                  className="button secondary"
                  disabled={!!working}
                  key={id}
                  onClick={() => void preview(id)}
                >
                  Ver dependência obrigatória
                </button>
              ))}
              {!stopped && (
                <p className="inline-error">
                  Para o servidor para aplicar esta alteração.
                </p>
              )}
              {working === "install" && (
                <p role="status">
                  A descarregar, verificar e criar as cópias. Aguarda a
                  conclusão.
                </p>
              )}
              {error && (
                <p className="inline-error" role="alert">
                  {error}
                </p>
              )}
              <div className="modal-actions">
                <button
                  className="button secondary"
                  disabled={working === "install"}
                  onClick={() => setPlan(null)}
                >
                  Cancelar
                </button>
                <button
                  className="button primary"
                  disabled={
                    !!working ||
                    !stopped ||
                    !!plan.blockers.length ||
                    !!inventory?.pending
                  }
                  onClick={() => void install()}
                >
                  {working === "install" ? (
                    <LoaderCircle size={16} className="spin" />
                  ) : (
                    <Download size={16} />
                  )}{" "}
                  Confirmar {plan.previous ? "atualização" : "instalação"}
                </button>
              </div>
            </section>
          </DialogBackdrop>
        )}
      </DialogPresence>
    </section>
  );
}
