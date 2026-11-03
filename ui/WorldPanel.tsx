import {
  formatDateTime,
  errorText as message,
  formatBytes as size,
} from "./lib/format";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { OreToggle } from "./OreToggle";
import { useEffect, useState } from "react";
import {
  Archive,
  Check,
  Clock3,
  HardDrive,
  LoaderCircle,
  RefreshCw,
  RotateCcw,
  ShieldCheck,
  Square,
  X,
} from "lucide-react";
import { api, type BackupInfo, type Server } from "./api";
import { AutoBackupPanel } from "./AutoBackupPanel";
import { BackupDestinationPanel } from "./BackupDestinationPanel";
import { ServerTransferPanel } from "./ServerTransferPanel";
import { MigrationPanel } from "./MigrationPanel";
import { CloudProvidersPanel } from "./CloudProvidersPanel";
import { Disclosure } from "./Disclosure";
import { RestoreRecoveryPanel } from "./RestoreRecoveryPanel";

const date = (value: number) =>
  formatDateTime(value, {
    day: "2-digit",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

export function WorldPanel({
  server,
  onNotice,
}: {
  server: Server;
  onNotice: (text: string) => void;
}) {
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [pending, setPending] = useState("");
  const [restoreTarget, setRestoreTarget] = useState<BackupInfo | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const stopped = ["offline", "error"].includes(server.state) && !server.pid;
  const working =
    !!pending || ["backing_up", "restoring"].includes(server.state);

  async function load() {
    try {
      setBackups(await api("backups", { id: server.id }));
      setError("");
    } catch (e) {
      setBackups([]);
      setError(message(e));
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void load();
  }, [server.id, server.state]);

  async function create() {
    setPending("backup");
    setError("");
    try {
      const backup = await api("create_backup", { id: server.id });
      await load();
      onNotice(`Backup criado: ${backup.fileCount} ficheiros verificados.`);
    } catch (e) {
      setError(message(e));
    } finally {
      setPending("");
    }
  }
  async function restore() {
    if (!restoreTarget || !confirmed) return;
    setPending("restore");
    setError("");
    try {
      await api("restore_backup", {
        id: server.id,
        backupId: restoreTarget.id,
      });
      setRestoreTarget(null);
      await load();
      onNotice(
        "Mundo restaurado. O estado anterior ficou guardado num backup automático.",
      );
    } catch (e) {
      setError(message(e));
    } finally {
      setPending("");
    }
  }
  async function stop() {
    setPending("stop");
    setError("");
    try {
      await api("stop", { id: server.id });
      onNotice("A guardar e parar. Aguarda o estado Parado.");
    } catch (e) {
      setError(message(e));
    } finally {
      setPending("");
    }
  }
  const total = backups.reduce((sum, backup) => sum + backup.sizeBytes, 0);
  return (
    <section className="world-panel">
      <RestoreRecoveryPanel server={server} onRecovered={() => void load()} />
      <div className="world-heading">
        <div>
          <h2>Backups</h2>
        </div>
        <button
          className="button primary"
          disabled={!stopped || working}
          onClick={() => void create()}
        >
          {pending === "backup" || server.state === "backing_up" ? (
            <LoaderCircle size={16} className="spin" />
          ) : (
            <Archive size={16} />
          )}
          Criar backup
        </button>
      </div>
      {!stopped && !working && (
        <div className="world-stop-note">
          <ShieldCheck size={20} />
          <div>
            <strong>Primeiro, guarda e para o servidor.</strong>
            <p>
              Os backups desta versão são feitos com o mundo parado para manter
              as cópias consistentes.
            </p>
          </div>
          {["online", "starting"].includes(server.state) && (
            <button
              className="button secondary"
              disabled={!!pending}
              onClick={() => void stop()}
            >
              <Square size={13} />
              Parar e guardar
            </button>
          )}
        </div>
      )}
      {working && (
        <div className="world-working" role="status">
          <LoaderCircle className="spin" size={20} />
          <div>
            <strong>
              {pending === "restore" || server.state === "restoring"
                ? "A verificar e restaurar os mundos…"
                : "A guardar uma cópia completa…"}
            </strong>
            <p>
              Podes consultar a consola. O arranque fica bloqueado até esta
              operação terminar.
            </p>
          </div>
        </div>
      )}
      {error && (
        <div className="inline-error world-error" role="alert">
          {error}
        </div>
      )}
      <div className="backup-summary">
        <div>
          <Archive size={19} />
          <span>
            Cópias guardadas<strong>{backups.length}</strong>
          </span>
        </div>
        <div>
          <HardDrive size={19} />
          <span>
            Dados nos backups<strong>{size(total)}</strong>
          </span>
        </div>
      </div>
      <div className="backup-list-header">
        <h3>Histórico de backups</h3>
        <button
          className="icon-button"
          aria-label="Atualizar backups"
          disabled={working}
          onClick={() => {
            setLoading(true);
            setError("");
            void load();
          }}
        >
          <RefreshCw size={16} />
        </button>
      </div>
      {loading ? (
        <div className="backup-empty">
          <LoaderCircle className="spin" size={25} />
          <p>A ler as cópias guardadas…</p>
        </div>
      ) : backups.length === 0 ? (
        <div className="backup-empty">
          <Archive size={35} />
          <h3>Ainda não há backups</h3>
          <p>
            {stopped
              ? "Cria a primeira cópia dos teus mundos."
              : "Para o servidor e cria o primeiro backup do teu progresso."}
          </p>
        </div>
      ) : (
        <div className="backup-list">
          {backups.map((backup) => (
            <article className="backup-row" key={backup.id}>
              <div
                className={`backup-icon ${backup.reason === "before_restore" ? "safety" : ""}`}
              >
                {backup.reason === "before_restore" ? (
                  <ShieldCheck size={20} />
                ) : (
                  <Archive size={20} />
                )}
              </div>
              <div className="backup-details">
                <strong>
                  {backup.reason === "before_restore"
                    ? "Antes do restauro"
                    : backup.reason === "before_mods"
                      ? "Antes de alterar mods"
                      : backup.reason === "before_plugins"
                        ? "Antes de alterar plugins"
                        : backup.reason === "automatic"
                          ? "Backup automático"
                          : "Backup manual"}
                </strong>
                <span>
                  <Clock3 size={11} />
                  {date(backup.createdAt)}
                </span>
                <small>
                  {backup.worlds.length} mundos · {backup.fileCount} ficheiros ·
                  Minecraft {backup.minecraftVersion}
                </small>
              </div>
              <span className="backup-size">{size(backup.sizeBytes)}</span>
              <button
                className="button secondary"
                disabled={
                  !stopped ||
                  working ||
                  backup.minecraftVersion !== server.version
                }
                onClick={() => {
                  setError("");
                  setConfirmed(false);
                  setRestoreTarget(backup);
                }}
              >
                <RotateCcw size={14} />
                Restaurar
              </button>
            </article>
          ))}
        </div>
      )}
      <div className="world-options">
        <Disclosure title="Backups automáticos" hint="Agendamento e retenção">
          <AutoBackupPanel
            server={server}
            onHistoryChange={() => void load()}
          />
        </Disclosure>
        <Disclosure
          title="Pasta de backups"
          hint="Neste PC ou num disco externo"
        >
          <BackupDestinationPanel
            server={server}
            onChange={() => void load()}
          />
        </Disclosure>
        <Disclosure title="Backups cloud" hint="Drive, Dropbox e S3">
          <CloudProvidersPanel
            server={server}
            onHistoryChange={() => void load()}
          />
        </Disclosure>
        <Disclosure
          title="Transferir ou atualizar"
          hint="Exportação e versão Minecraft"
        >
          <article className="connection-card transfer-card">
            <h3>Transferir para outro computador</h3>
            <p>Exporta com o servidor parado e importa no outro PC.</p>
            <ServerTransferPanel server={server} />
          </article>
          <MigrationPanel server={server} onNotice={onNotice} />
        </Disclosure>
        <Disclosure title="O que inclui um backup?">
          <div className="backup-footnotes">
            <div>
              <ShieldCheck size={17} />
              <p>
                <strong>Um passo atrás, com uma rede de segurança.</strong>{" "}
                Antes de restaurar, o Nethergate guarda uma cópia completa do
                estado atual e verifica todos os ficheiros do backup escolhido.
              </p>
            </div>
            <p>
              Estas cópias incluem os ficheiros dos mundos, não os plugins, as
              contas, o Java ou as definições do servidor. São locais, sem
              compressão. A retenção só limpa cópias automáticas antigas quando
              o agendamento está ativo. Os ficheiros substituídos no restauro
              também ficam preservados; o espaço real ocupado pode ser maior do
              que o total acima. Cópias no mesmo disco não protegem contra
              avaria desse disco.
            </p>
          </div>
        </Disclosure>
      </div>
      <DialogPresence>
        {restoreTarget && (
          <DialogBackdrop
            className="modal-backdrop"
            onClick={() => {
              if (!pending) setRestoreTarget(null);
            }}
          >
            <section
              className="modal restore-modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="restore-title"
              onClick={(e) => e.stopPropagation()}
            >
              <button
                className="modal-close"
                disabled={!!pending}
                aria-label="Cancelar restauro"
                onClick={() => setRestoreTarget(null)}
              >
                <X size={20} />
              </button>
              <div className="eyebrow">VOLTAR A UM MOMENTO ANTERIOR</div>
              <h2 id="restore-title">Restaurar este backup?</h2>
              <p>
                O progresso atual dos mundos de <strong>{server.name}</strong>{" "}
                será substituído pelo estado de {date(restoreTarget.createdAt)}.
              </p>
              <div className="restore-facts">
                <span>
                  <Archive size={16} />
                  {restoreTarget.worlds.join(", ")}
                </span>
                <span>
                  <HardDrive size={16} />
                  {size(restoreTarget.sizeBytes)} · {restoreTarget.fileCount}{" "}
                  ficheiros
                </span>
              </div>
              <div className="restore-safety">
                <ShieldCheck size={20} />
                <p>
                  Primeiro criaremos automaticamente uma cópia do estado atual.
                  Se os ficheiros do backup estiverem alterados ou a versão for
                  diferente, o restauro será recusado.
                </p>
              </div>
              <label className="eula-check">
                <OreToggle
                  checked={confirmed}
                  disabled={!!pending}
                  onChange={(e) => setConfirmed(e.target.checked)}
                />
                <span>
                  Quero substituir os mundos atuais por este backup, preservando
                  primeiro o meu progresso atual.
                </span>
              </label>
              {error && (
                <p className="inline-error" role="alert">
                  {error}
                </p>
              )}
              <div className="modal-actions">
                <button
                  className="button secondary"
                  disabled={!!pending}
                  onClick={() => setRestoreTarget(null)}
                >
                  Cancelar
                </button>
                <button
                  className="button primary"
                  disabled={!confirmed || !stopped || !!pending}
                  onClick={() => void restore()}
                >
                  {pending ? (
                    <LoaderCircle className="spin" size={16} />
                  ) : (
                    <Check size={16} />
                  )}
                  Guardar atual e restaurar
                </button>
              </div>
            </section>
          </DialogBackdrop>
        )}
      </DialogPresence>
    </section>
  );
}
