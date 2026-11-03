import { errorText } from "./lib/format";
import { formatDateTime, formatTime } from "./lib/format";
import type { Status, ExternalTest } from "./api/models/DdnsPanel";
import { OreToggle } from "./OreToggle";
import { useEffect, useRef, useState } from "react";
import { Copy, ExternalLink, LoaderCircle } from "lucide-react";
import { api, openDuckDns, type Server } from "./api";

export function DdnsPanel({
  server,
  onOpenWan,
  networkRevision,
  mappedPort,
}: {
  server: Server;
  onOpenWan: () => void;
  networkRevision: number;
  mappedPort?: number;
}) {
  const [status, setStatus] = useState<Status | null>(null);
  const [hostname, setHostname] = useState("");
  const [token, setToken] = useState("");
  const [port, setPort] = useState(server.port);
  const [automatic, setAutomatic] = useState(false);
  const [consent, setConsent] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [testConsent, setTestConsent] = useState(false);
  const [result, setResult] = useState<ExternalTest | null>(null);
  const mounted = useRef(false);
  const busy = useRef(false);
  const generation = useRef(0);
  const testRevision = useRef(0);
  useEffect(() => {
    if (mappedPort) {
      setPort(mappedPort);
      setResult(null);
      setNotice(
        `O router atribuiu a porta ${mappedPort}. Guarda as definições DuckDNS para usar esta porta no endereço partilhado.`,
      );
    }
  }, [mappedPort]);
  useEffect(() => {
    let closed = false;
    mounted.current = true;
    const request = ++generation.current;
    setStatus(null);
    setToken("");
    setConsent(false);
    api("ddns_status", { id: server.id })
      .then((value) => {
        if (closed || request !== generation.current) return;
        setStatus(value);
        setHostname(value.hostname || "");
        setPort(value.externalPort || server.port);
        setAutomatic(value.automatic);
      })
      .catch((e) => {
        if (!closed && request === generation.current) setError(errorText(e));
      });
    const timer = window.setInterval(() => {
      if (busy.current) return;
      const request = generation.current;
      api("ddns_status", { id: server.id })
        .then((value) => {
          if (!closed && request === generation.current) setStatus(value);
        })
        .catch(() => {});
    }, 15000);
    return () => {
      closed = true;
      mounted.current = false;
      ++generation.current;
      window.clearInterval(timer);
    };
  }, [server.id]);
  useEffect(() => {
    ++testRevision.current;
    setResult(null);
    setTestConsent(false);
  }, [
    server.state,
    server.port,
    server.localAddress,
    status?.hostname,
    status?.externalPort,
    status?.lastPublishedIp,
    status?.lastSuccess,
    networkRevision,
  ]);
  async function run(operation: "save" | "update" | "pause") {
    if (busy.current) return;
    busy.current = true;
    const request = ++generation.current;
    setWorking(true);
    setError("");
    setNotice("");
    try {
      const value = await api(`ddns_${operation}`, {
        id: server.id,
        expectedHostname: status?.hostname,
        settings: { hostname, token, externalPort: port, automatic, consent },
      });
      if (!mounted.current || request !== generation.current) return;
      setStatus(value);
      if (operation === "save") {
        setToken("");
        setConsent(false);
        setNotice(
          value.automatic
            ? "Guardado. A atualização automática começa enquanto o motor estiver ligado, mesmo com o painel fechado."
            : "Guardado. Usa Publicar agora para atualizar o nome no DuckDNS.",
        );
      } else if (operation === "pause") {
        setAutomatic(false);
        setNotice(
          "Atualizações automáticas pausadas. O nome mantém o último IP publicado; o acesso de rede não foi fechado.",
        );
      } else
        setNotice(
          "O DuckDNS confirmou este IPv4. Testa agora o nome a partir de outra rede.",
        );
    } catch (e) {
      if (mounted.current && request === generation.current)
        setError(errorText(e));
    } finally {
      busy.current = false;
      if (mounted.current) setWorking(false);
    }
  }
  async function test() {
    if (busy.current || !testConsent || !status?.hostname) return;
    busy.current = true;
    const request = ++generation.current;
    const revision = testRevision.current;
    setWorking(true);
    setError("");
    setResult(null);
    try {
      const value = await api("ddns_test", {
        id: server.id,
        expectedHostname: status.hostname,
        consent: testConsent,
      });
      if (
        mounted.current &&
        request === generation.current &&
        revision === testRevision.current
      )
        setResult(value);
    } catch (e) {
      if (
        mounted.current &&
        request === generation.current &&
        revision === testRevision.current
      )
        setError(errorText(e));
    } finally {
      busy.current = false;
      if (mounted.current) setWorking(false);
    }
  }
  const endpoint = status?.hostname
    ? `${status.hostname}${status.externalPort === 25565 ? "" : `:${status.externalPort}`}`
    : "";
  return (
    <section className="connection-card ddns-panel" id={`dns-${server.id}`}>
      <h3>Endereço DuckDNS</h3>
      <p>
        Mantém o mesmo nome quando o IP público muda. Usa ligação direta, sem
        passar pelo túnel playit.
      </p>
      <div className="form-info">
        Requer uma porta encaminhada no router e permissão na firewall. CGNAT
        pode impedir o acesso direto, mesmo com o DNS atualizado.
        <button type="button" className="button secondary" onClick={onOpenWan}>
          Configurar ligação direta
        </button>
      </div>
      {!status && !error && <p role="status">A ler o nome deste servidor…</p>}
      {status && !status.supported && (
        <p className="form-info">
          A configuração DuckDNS está disponível apenas no Windows nesta versão.
        </p>
      )}
      <details className="dns-account-help">
        <summary>Como obter um nome e token DuckDNS</summary>
        <ol>
          <li>Abre o DuckDNS e entra na tua conta.</li>
          <li>
            Regista um nome disponível, por exemplo o-teu-mundo.duckdns.org.
          </li>
          <li>
            Copia o token da tua conta para o campo abaixo. Não precisas de o
            enviar pelo chat.
          </li>
        </ol>
        <button
          type="button"
          className="button secondary"
          onClick={() =>
            void openDuckDns().catch((e) => setError(errorText(e)))
          }
        >
          <ExternalLink size={16} /> Abrir DuckDNS
        </button>
      </details>
      {status?.configured && (
        <>
          <div className="connection-address internet-public-address">
            <code>{endpoint}</code>
            <button
              className="button secondary button-copy"
              aria-label="Copiar endereço"
              title="Copiar endereço"
              type="button"
              onClick={() =>
                void navigator.clipboard.writeText(endpoint).then(
                  () => setNotice("Nome para teste copiado."),
                  () => setNotice(`Endereço: ${endpoint}`),
                )
              }
            >
              <Copy size={16} />
            </button>
          </div>
          <p>
            {status.automatic
              ? "Atualização automática ativa · a cada 5 minutos com o motor ligado."
              : "Atualização automática pausada."}
          </p>
          <p>
            {status.lastSuccess
              ? `Última confirmação DuckDNS: ${formatDateTime(status.lastSuccess)} · ${status.lastPublishedIp}`
              : "IP público ainda por confirmar nesta sessão."}
          </p>
          {status.needsNetworkUpdate && (
            <p className="form-info" role="status">
              A rede mudou depois da última confirmação.
              {status.automatic
                ? " A atualização autorizada foi antecipada; respeita pelo menos 30 segundos entre tentativas. Aguarda a confirmação abaixo."
                : " As atualizações automáticas estão pausadas. Usa Publicar agora quando quiseres atualizar o nome para esta rede."}
              O último IP publicado pode pertencer à ligação anterior.
            </p>
          )}
          {status.wanChangedAt && (
            <p className="form-info" role="status">
              O IP WAN mudou e o DuckDNS confirmou o novo IP às{" "}
              {formatTime(status.wanChangedAt)}. Revê router/firewall e volta a
              testar Minecraft; DNS atualizado não garante acesso.
            </p>
          )}
          <p className="connection-tag">
            Acesso ao Minecraft: verificar separadamente
          </p>
          {status.error && (
            <p className="inline-error" role="status">
              {status.error}
            </p>
          )}
          <div className="modal-actions">
            <button
              type="button"
              className="button primary"
              disabled={working}
              onClick={() => void run("update")}
            >
              Publicar agora
            </button>
            <button
              type="button"
              className="button secondary"
              disabled={working || !status.automatic}
              onClick={() => void run("pause")}
            >
              Pausar atualizações
            </button>
          </div>
          <details className="dns-test">
            <summary>Testar este endereço pela internet</summary>
            <div className="internet-step">
              <label className="eula-check">
                <OreToggle
                  checked={testConsent}
                  disabled={working}
                  onChange={(e) => setTestConsent(e.target.checked)}
                />
                <span>
                  Autorizo enviar {endpoint} ao mcsrvstat.us para testar a
                  resposta Minecraft.
                </span>
              </label>
              <button
                type="button"
                className="button primary"
                disabled={working || !testConsent || server.state !== "online"}
                onClick={() => void test()}
              >
                {working && <LoaderCircle className="spin" size={16} />}{" "}
                Verificar ligação pelo nome
              </button>
              {server.state !== "online" && (
                <p>Inicia o servidor para verificar a ligação.</p>
              )}
              {result && (
                <div
                  role="status"
                  className={`connection-result ${result.state === "reported_online" ? "success" : ""}`}
                >
                  <strong>
                    {result.state === "reported_online"
                      ? "O Minecraft respondeu pelo nome público"
                      : result.state === "reported_offline"
                        ? "O serviço externo não recebeu resposta Minecraft"
                        : "Não foi possível confirmar a resposta Minecraft"}
                  </strong>
                  <p>
                    {result.state === "reported_online"
                      ? "Confirma a entrada com uma conta autorizada a partir de outra rede."
                      : "Revê o IP publicado, a propagação DNS, o encaminhamento e a firewall. Se existir CGNAT, usa a opção Túnel playit acima."}
                  </p>
                  <small>
                    {formatDateTime(
                      result.observedAt
                        ? result.observedAt * 1000
                        : result.checkedAt,
                    )}{" "}
                    · Resultado sujeito a cache até cinco minutos.
                  </small>
                </div>
              )}
            </div>
          </details>
        </>
      )}
      <details className="dns-settings" open={!status?.configured}>
        <summary>
          {status?.configured
            ? "Editar nome e atualização automática"
            : "Configurar o nome deste servidor"}
        </summary>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void run("save");
          }}
        >
          <fieldset disabled={working || !status?.supported}>
            <label>
              Nome registado no DuckDNS
              <input
                required
                maxLength={75}
                placeholder="o-teu-mundo.duckdns.org"
                value={hostname}
                onChange={(e) => setHostname(e.target.value)}
              />
            </label>
            <label>
              Token DuckDNS
              <input
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={token}
                placeholder={
                  status?.hasToken
                    ? "Já guardado; deixa vazio para manter o mesmo"
                    : "Token da tua conta DuckDNS"
                }
                onChange={(e) => setToken(e.target.value)}
              />
              <small>
                Protegido pela tua conta do Windows. Ao mudares de nome, volta a
                introduzir o token.
              </small>
            </label>
            <label>
              Porta externa TCP no router
              <input
                type="number"
                required
                min={1}
                max={65535}
                value={port}
                onChange={(e) => setPort(Number(e.target.value))}
              />
              <small>
                Deve coincidir com a porta externa da tua regra. Guardar aqui
                não altera o router.
              </small>
            </label>
            <label className="eula-check">
              <OreToggle
                checked={automatic}
                onChange={(e) => setAutomatic(e.target.checked)}
              />
              <span>
                Atualizar o IP a cada 5 minutos com o motor ligado, mesmo com o
                servidor parado ou o painel fechado.
              </span>
            </label>
            <label className="eula-check">
              <OreToggle
                required
                checked={consent}
                onChange={(e) => setConsent(e.target.checked)}
              />
              <span>
                Autorizo consultar api.ipify.org e enviar o nome, token e IPv4
                público ao DuckDNS para atualizar este registo.
              </span>
            </label>
            <button className="button primary" disabled={working || !consent}>
              {working && <LoaderCircle className="spin" size={16} />} Guardar
              configuração
            </button>
          </fieldset>
        </form>
      </details>
      <p className="field-hint">
        A confirmação do DuckDNS não comprova entrada no Minecraft. A propagação
        DNS pode demorar. Este nome continua a apontar para o teu IP público;
        não cria um túnel nem esconde esse IP.
      </p>
      {notice && (
        <p className="gameplay-success" role="status">
          {notice}
        </p>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
