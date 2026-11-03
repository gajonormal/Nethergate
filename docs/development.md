# Desenvolvimento

Usa Node.js 22 e Rust estável com `rustfmt` e `clippy`. O desktop Windows precisa das ferramentas C++ do Visual Studio e do WebView2. Os dois lockfiles Cargo são mantidos: o núcleo e o desktop são projetos Cargo separados.

## Preparar e executar

```sh
npm ci
npm run desktop
```

`npm run dev` inicia apenas o frontend. As operações reais precisam do adaptador local ou do desktop; os testes de interface usam uma API simulada.

## Verificações

```sh
npm run format:check
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
npm run build
```

No Windows, verifica também o adaptador desktop:

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --features custom-protocol --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --features custom-protocol --locked
```

`npm run format` aplica a formatação do frontend, dos testes e das configurações. O build também rejeita imports, variáveis e parâmetros TypeScript não utilizados.

Os testes ignorados pelo Cargo dependem de recursos externos e devem ser executados apenas depois de rever os seus requisitos. As ferramentas manuais em `scripts/` podem preparar fixtures ou alterar regras de firewall; não fazem parte da execução normal da suite.

## Interface

```sh
npx playwright install chromium
npm run test:ui
```

Em Linux, a preparação de uma máquina nova pode exigir `npx playwright install --with-deps chromium`. A suite compila o frontend e verifica tipografia, seleção de texto, cabeçalho responsivo, animações dos diálogos e fluxos cloud simulados. As capturas geradas são locais e estão excluídas do Git.

As variáveis opcionais `PLAYWRIGHT_MODULE` e `PLAYWRIGHT_EXECUTABLE_PATH` permitem usar uma instalação existente do Playwright e do Chromium. Sem estas variáveis, são usadas as dependências instaladas no projeto.

## Compatibilidade de nomes

A marca visível é Nethergate. `NetherGateData`, cabeçalhos `X-NetherGate-*`, prefixos de regras e pastas, e o contexto de proteção de segredos conservam a grafia histórica. Alterá-los sem migração pode impedir o acesso a dados ou o reconhecimento de regras existentes.

## Preparar uma publicação

O workflow de CI verifica o núcleo em Windows e Linux, o frontend e o adaptador desktop Windows. O workflow manual Windows candidate gera um instalador com tamanho e SHA-256, sem publicar uma release.

`npm run package:windows` gera o instalador em `src-tauri/target/release/bundle/nsis/`. Antes de o distribuir, testa a instalação, atualização e preservação da biblioteca num Windows limpo. O CI remoto e os testes locais não substituem esse ensaio. Confirma também as atribuições em [recursos de terceiros](../THIRD_PARTY_NOTICES.md).

Não incluas mundos, backups, tokens, contas OAuth, logs ou capturas pessoais no commit. Os relatórios históricos de desenvolvimento são locais e estão excluídos do Git.
