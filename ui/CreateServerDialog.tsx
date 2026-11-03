import { errorText } from "./lib/format";
import { DialogBackdrop } from "./DialogPresence";
import { useEffect, useRef, useState } from "react";
import {
  ChevronRight,
  Download,
  LoaderCircle,
  LockKeyhole,
  Plus,
  Settings2,
  X,
} from "lucide-react";
import { api, desktop, openEula, type Server } from "./api";
import { OreToggle } from "./OreToggle";
import { MemoryControl } from "./MemoryControl";

export function CreateModal({
  servers,
  onClose,
  onCreated,
}: {
  servers: Server[];
  onClose: () => void;
  onCreated: (id: string) => Promise<void>;
}) {
  const ports = new Set(servers.map((s) => s.port));
  let nextPort = 25565;
  while (ports.has(nextPort)) nextPort++;
  const [form, setForm] = useState({
    name: "",
    software: "paper" as "paper" | "fabric",
    version: "",
    memoryMb: 2048,
    port: nextPort,
    contact: servers[0]?.contact || "",
    javaMajor: 25,
    javaPath: "auto",
    eulaAccepted: false,
  });
  const [versions, setVersions] = useState<string[]>([]);
  const versionRequest = useRef(0);
  const [checking, setChecking] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  async function loadVersions() {
    const request = ++versionRequest.current;
    setChecking(true);
    setError("");
    try {
      const rows = await api("versions", {
        contact: form.contact,
        software: form.software,
      });
      if (request !== versionRequest.current) return;
      const supported = rows.filter(
        (v) =>
          /^\d+(\.\d+){1,2}$/.test(v) &&
          (Number(v.split(".")[0]) >= 26 || Number(v.split(".")[1]) >= 20),
      );
      setVersions(supported);
      if (!form.version && supported[0]) changeVersion(supported[0]);
    } catch (e) {
      if (request === versionRequest.current) setError(errorText(e));
    } finally {
      if (request === versionRequest.current) setChecking(false);
    }
  }
  function changeVersion(version: string) {
    setForm((old) => ({
      ...old,
      version,
      javaMajor: Number(version.split(".")[0]) >= 26 ? 25 : 21,
    }));
  }
  useEffect(() => {
    if (form.contact) void loadVersions();
  }, [form.software]);
  return (
    <DialogBackdrop
      className="modal-backdrop"
      onClick={() => {
        if (!saving) onClose();
      }}
    >
      <section
        className="modal create-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="create-title"
        onClick={(e) => e.stopPropagation()}
      >
        <button
          className="modal-close"
          disabled={saving}
          onClick={onClose}
          aria-label="Fechar criação"
        >
          <X size={20} />
        </button>
        <div className="eyebrow">MAIS UM MUNDO À TUA ESPERA</div>
        <h2 id="create-title">
          Uma nova aventura<span>.</span>
        </h2>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            setSaving(true);
            setError("");
            try {
              const { id } = await api("create", form);
              await api("install", { id });
              await onCreated(id);
            } catch (e) {
              setError(errorText(e));
            } finally {
              setSaving(false);
            }
          }}
        >
          <fieldset disabled={saving}>
            <label>
              Nome do servidor
              <input
                autoFocus
                required
                maxLength={60}
                placeholder="Ex.: Survival com os amigos"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </label>
            <div className="form-row">
              <label>
                Software
                <select
                  value={form.software}
                  onChange={(e) => {
                    setForm({
                      ...form,
                      software: e.target.value as "paper" | "fabric",
                      version: "",
                    });
                    setVersions([]);
                  }}
                >
                  <option value="paper">Paper · plugins</option>
                  <option value="fabric">Fabric · mods</option>
                </select>
                <small>
                  {form.software === "fabric"
                    ? "Cria um perfil separado. Mods partilhados exigem Fabric também nos PCs dos jogadores."
                    : "Compatível com plugins Bukkit/Paper."}
                </small>
              </label>
              <label>
                Versão do Minecraft
                <div className="version-field">
                  <input
                    required
                    list="paper-versions"
                    placeholder={checking ? "A consultar…" : "Ex.: 26.3"}
                    value={form.version}
                    onChange={(e) => changeVersion(e.target.value)}
                  />
                  <button
                    type="button"
                    disabled={checking || !form.contact}
                    title="Consultar versões do software selecionado"
                    aria-label="Consultar versões"
                    onClick={() => void loadVersions()}
                  >
                    {checking ? (
                      <LoaderCircle size={16} className="spin" />
                    ) : (
                      <Download size={16} />
                    )}
                  </button>
                </div>
                <datalist id="paper-versions">
                  {versions.map((v) => (
                    <option key={v} value={v} />
                  ))}
                </datalist>
                <small>O instalador aceita apenas builds estáveis.</small>
              </label>
            </div>
            <MemoryControl
              value={form.memoryMb}
              onChange={(memoryMb) => setForm({ ...form, memoryMb })}
            />
            <label>
              Contacto do perfil
              <input
                type="text"
                required
                placeholder="Email ou página do projeto"
                value={form.contact}
                onChange={(e) => setForm({ ...form, contact: e.target.value })}
              />
              <small>
                {form.software === "paper"
                  ? "Enviado no User-Agent à API PaperMC. Não cria uma conta."
                  : "Guardado no perfil; o download usa a API oficial Fabric."}
              </small>
            </label>
            <button
              className="advanced-toggle"
              type="button"
              onClick={() => setAdvanced(!advanced)}
            >
              <Settings2 size={14} />
              {advanced ? "Ocultar opções avançadas" : "Opções avançadas"}
              <ChevronRight size={14} className={advanced ? "rotated" : ""} />
            </button>
            {advanced && (
              <div className="advanced-fields">
                <div className="form-row">
                  <label>
                    Porta local
                    <input
                      type="number"
                      min={1024}
                      max={65535}
                      required
                      value={form.port}
                      onChange={(e) =>
                        setForm({ ...form, port: Number(e.target.value) })
                      }
                    />
                  </label>
                  <label>
                    Java mínimo
                    <input
                      type="number"
                      min={21}
                      max={99}
                      required
                      value={form.javaMajor}
                      onChange={(e) =>
                        setForm({ ...form, javaMajor: Number(e.target.value) })
                      }
                    />
                  </label>
                </div>
                <label>
                  Executável Java
                  <input
                    required
                    value={form.javaPath}
                    onChange={(e) =>
                      setForm({ ...form, javaPath: e.target.value })
                    }
                  />
                  <small>
                    auto descarrega Temurin verificado, sem instalar no Windows.
                    Também podes indicar java ou um caminho próprio.
                  </small>
                </label>
              </div>
            )}
            <label className="eula-check">
              <OreToggle
                required
                checked={form.eulaAccepted}
                onChange={(e) =>
                  setForm({ ...form, eulaAccepted: e.target.checked })
                }
              />
              <span>
                Li e aceito a{" "}
                <a
                  href="https://aka.ms/MinecraftEULA"
                  target="_blank"
                  rel="noreferrer"
                  onClick={(e) => {
                    if (desktop) {
                      e.preventDefault();
                      void openEula().catch((error) =>
                        setError(errorText(error)),
                      );
                    }
                  }}
                >
                  EULA do Minecraft
                </a>{" "}
                para este servidor.
              </span>
            </label>
          </fieldset>
          {error && (
            <p className="inline-error" role="alert">
              {error}
            </p>
          )}
          <div className="modal-actions">
            <span>
              <LockKeyhole size={13} />
              Criado apenas neste computador
            </span>
            <button className="button primary" disabled={saving}>
              {saving ? (
                <LoaderCircle size={17} className="spin" />
              ) : (
                <Plus size={17} />
              )}
              Criar e preparar
            </button>
          </div>
        </form>
      </section>
    </DialogBackdrop>
  );
}
