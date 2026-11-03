# Backups cloud

Em **Mundo → Backups cloud**, escolhe S3, Google Drive ou Dropbox. Cada destino tem a sua configuração e o envio automático é opcional. O motor precisa de continuar ligado para enviar cópias, mesmo que feches o painel.

## S3 compatível

Usa um bucket privado num serviço compatível com S3. Introduz o endpoint HTTPS, a região, o bucket, um prefixo para o servidor e as credenciais de acesso. O endpoint não deve incluir o nome do bucket. Para Cloudflare R2, a região é `auto`.

Guarda o bucket e o prefixo utilizados: precisas do mesmo destino para recuperar o histórico noutro computador. Alterar estes campos abre um histórico separado. A opção de deduplicação permite reutilizar blocos já enviados e retomar depois de reiniciar.

## Google Drive

Esta prévia ainda requer a configuração de uma aplicação OAuth:

1. Ativa a Google Drive API no Google Cloud e configura o ecrã de consentimento.
2. Cria um cliente OAuth do tipo **Desktop app**. Se estiver em modo de teste, adiciona a tua conta aos utilizadores de teste.
3. No Nethergate, abre **Google Drive → Ligar Google Drive**, introduz o Client ID e o client secret, caso seja exigido pelo registo.
4. Autoriza o acesso no browser.

A aplicação pede o âmbito `drive.appdata`. As cópias ficam na área privada da aplicação, que não aparece como uma pasta normal no Drive. Para recuperar esse histórico noutro PC, usa a mesma aplicação OAuth e o mesmo identificador cloud.

## Dropbox

1. Cria uma aplicação **Scoped access / App folder** na Dropbox App Console.
2. Ativa `files.content.read`, `files.content.write` e `files.metadata.read`.
3. Regista `http://127.0.0.1:53682/oauth/callback` como redirect URI.
4. Introduz a **App Key** no Nethergate e autoriza no browser. O fluxo usa PKCE e não precisa de distribuir o App Secret.

As cópias ficam na pasta da aplicação. A autorização de Drive e Dropbox usa a porta local 53682; se estiver ocupada, fecha a aplicação que a está a usar e tenta novamente.

## Recuperar uma cópia

Abre o histórico do destino e transfere a cópia para os backups locais. Depois usa **Restaurar** no histórico local, com o servidor parado. Transferir uma cópia da cloud não substitui automaticamente o mundo.

Um envio interrompido pode ser retomado com a ação correspondente. Os blocos confirmados são reutilizados; o bloco que estava em transferência pode precisar de ser reenviado. Desligar uma conta no Nethergate remove as credenciais locais, preservando os backups remotos. Podes revogar o acesso na conta do fornecedor.

## Limites da prévia

As credenciais guardadas dependem da proteção da conta Windows. Drive e Dropbox foram testados com serviços simulados; falta validar os fluxos completos com aplicações OAuth e contas reais.

A retenção automática está disponível para S3. Os blocos partilhados e os de envios incompletos não são apagados automaticamente, pelo que remover entradas do histórico pode não libertar imediatamente o espaço. Drive e Dropbox não têm retenção automática. O fornecedor pode cobrar armazenamento e operações de leitura ou transferência.
