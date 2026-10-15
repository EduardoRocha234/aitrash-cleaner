<p align="center"><img src="assets/logo.png" alt="aitrash-cleaner" width="640"></p>

<h1 align="center">aitrash-cleaner</h1>

<p align="center"><a href="README.md">English</a> · <b>Português (Brasil)</b></p>

**Libere espaço em disco ocupado por agentes de IA.** O `aitrash` encontra e limpa o lixo que Claude Code, OpenAI Codex CLI, Gemini CLI/Antigravity, Cursor, Windsurf e Kiro deixam na máquina: transcrições antigas em `~/.claude/projects` e `~/.codex/sessions`, caches dos editores, scripts de teste/debug esquecidos nos projetos, dev servers órfãos, worktrees `claude/*` abandonadas e chaves de API em texto puro dentro das transcrições.

É um binário único (cerca de 2 MB), sem runtime, para Linux, macOS e Windows. No WSL ele também varre o perfil do Windows em `/mnt/c/Users`. Por padrão tudo vai para a lixeira do sistema, então nada se perde por acidente.

## Instalação

**Linux / macOS / WSL:**
```bash
curl -LsSf https://github.com/EduardoRocha234/aitrash-cleaner/releases/latest/download/aitrash-installer.sh | sh
```

**Windows (PowerShell):**
```powershell
irm https://github.com/EduardoRocha234/aitrash-cleaner/releases/latest/download/aitrash-installer.ps1 | iex
```

O instalador detecta o sistema e a arquitetura, baixa o binário certo para `~/.local/bin` e coloca essa pasta no PATH. Abra um novo terminal depois e rode `aitrash`.

**Via npm (quem já tem Node):**
```bash
npx aitrash            # roda sem instalar
npm i -g aitrash       # instala o comando aitrash
```

**Manual:** baixe o arquivo do seu sistema em [Releases](https://github.com/EduardoRocha234/aitrash-cleaner/releases/latest), extraia e coloque o `aitrash` em uma pasta do PATH.

**Do código-fonte** (precisa do [Rust](https://rustup.rs)):
```bash
cargo install --git https://github.com/EduardoRocha234/aitrash-cleaner
```

### Atualizar e desinstalar

Para atualizar, rode o mesmo comando de instalação de novo.

```bash
rm ~/.local/bin/aitrash        # instalador de uma linha (Windows: del $HOME\.local\bin\aitrash.exe)
npm rm -g aitrash              # npm
cargo uninstall aitrash        # código-fonte
```

## Uso

```
aitrash                    # resumo: um grupo por linha (não altera nada)
aitrash -v                 # cada item em detalhe
aitrash scan --home        # varre todos os projetos do home
aitrash clean              # menu por grupo; vai para a lixeira
aitrash clean -n           # dry-run
aitrash secrets --all      # chaves/tokens em texto puro nas transcrições
aitrash secrets --clean    # escolhe transcrições com segredos para apagar
aitrash du                 # quanto cada agente e cache ocupa
aitrash paths              # caminhos conhecidos neste sistema
```

Flags úteis: `--only global,project,system,secrets`, `--older-than 30d`, `--json`, `--verbose`, `--no-wsl`, `--hard` (apaga direto, sem lixeira).

## O que procura

| Categoria | Exemplos |
|---|---|
| global | transcrições/sessões antigas (`~/.claude/projects`, `~/.codex/sessions`, `~/.gemini/tmp`), caches de Cursor/Windsurf/Antigravity, credenciais (`auth.json`) |
| project | `*.bak`, `*.orig`, `test_fix*.py`, `debug_*.js`, `check_*.sh`, `scratch/`, `.md` de resumo, `CLAUDE.md`/`AGENTS.md`/`.claude/` |
| system | caches (playwright, npx, npm, pip, uv, pnpm), dev servers órfãos, worktrees/stashes/branches `claude/*`, Docker parado, linhas de agentes no `.bashrc`/`.zshrc` |
| secrets | `sk-ant-`, `sk-`, `ghp_`, `AKIA…`, `xox?-`, `AIza…`, chaves privadas, `*_TOKEN=`/`*_SECRET=` (sempre mascarados) |

## Segurança

- `scan` nunca altera nada. `clean` sem terminal interativo vira dry-run, a não ser que você passe `--yes`.
- Remoção vai para a lixeira do SO. Para apagar de vez, use `--hard`.
- Só os itens de confiança **alta** vêm pré-marcados. Arquivo rastreado pelo git nunca é pré-marcado, e um arquivo de projeto não rastreado sobe um nível de confiança.
- Itens protegidos `[!]` (credenciais, `.claude.json`, rc files, arquivos rastreados) só aparecem com `--hard`, e cada um pede confirmação.
- Não segue symlinks e não remove nada fora do home ou dos caminhos passados.

## Perguntas frequentes

### Como liberar espaço ocupado pelo Claude Code?

Rode `aitrash du` para ver quanto o `~/.claude` ocupa e depois `aitrash clean`. As transcrições de sessão em `~/.claude/projects` (arquivos `.jsonl`) são a maior parte, junto com `file-history`, `paste-cache`, `debug`, `shell-snapshots`, `session-env` e `todos`. Planos antigos em `~/.claude/plans` também são oferecidos, mas nunca vêm pré-marcados. Por padrão só entram arquivos com mais de 30 dias; mude com `--older-than 7d`.

### É seguro apagar `~/.claude/projects`?

Esses arquivos são o histórico das conversas. Apagar os antigos só impede dar `--resume` nessas sessões; configurações, credenciais e `CLAUDE.md` ficam intactos. O `aitrash` só marca transcrições mais velhas que `--older-than`, manda para a lixeira e nunca mexe em `.credentials.json` ou `.claude.json` sem `--hard` e confirmação item a item.

### Por que o `~/.codex` (OpenAI Codex CLI) está tão grande?

O Codex guarda toda sessão em `~/.codex/sessions` e `~/.codex/archived_sessions`, além de logs em `~/.codex/log`, e nada disso é limpo sozinho. `aitrash clean --only global` lista e remove os antigos.

### Como limpar o cache do Cursor ou do Windsurf?

Cursor, Windsurf, Antigravity e Kiro são forks do VS Code, então acumulam `Cache`, `CachedData`, `Code Cache`, `GPUCache`, `logs`, `CachedExtensionVSIXs` e crash dumps na pasta do app (`~/.config/Cursor` no Linux, `~/Library/Application Support/Cursor` no macOS, `%APPDATA%\Cursor` no Windows). Tudo isso é recriado sob demanda. `aitrash du` mostra o tamanho e `aitrash clean` remove só essas pastas de cache, nunca configurações, extensões ou config de MCP.

### E o Gemini CLI e o Antigravity?

O `aitrash` varre `~/.gemini/tmp` e, do Antigravity, `~/.gemini/antigravity/brain` e `browser_recordings`. Credenciais OAuth (`oauth_creds.json`, `google_accounts.json`) ficam protegidas.

### O agente deixou `test_fix.py`, `debug_*.js` e `.md` de resumo espalhados no projeto. Dá para limpar?

Sim. `aitrash scan` (projeto atual) ou `aitrash scan --home` (todos os projetos) encontra scripts de rascunho, backups (`*.bak`, `*.orig`), pastas `scratch/` e markdown de resumo escrito pela IA. Arquivo rastreado pelo git nunca vem pré-marcado.

### As transcrições dos agentes têm chaves de API?

Com frequência, sim: tudo que você colou ou que uma ferramenta imprimiu vai parar na transcrição em texto puro. `aitrash secrets --all` acha chaves da Anthropic, OpenAI, GitHub, AWS, Slack e Google, chaves privadas e linhas `*_TOKEN=`, sempre mascaradas na saída. `aitrash secrets --clean` deixa apagar as transcrições afetadas. Troque (rotacione) qualquer chave encontrada.

### Funciona no Windows e no WSL?

Sim. Há binário nativo para Windows e, no WSL, ele também varre o perfil do Windows em `/mnt/c/Users` (desative com `--no-wsl`).

### Qual a diferença para `rm -rf ~/.claude` ou `ncdu`?

| | `aitrash` | `rm -rf` | `ncdu` / analisadores de disco |
|---|---|---|---|
| Sabe quais arquivos de agente podem sair | sim | não | não |
| Preserva credenciais e configurações | sim | não | manual |
| Filtro de idade para sessões | sim | não | não |
| Acha sobras dentro dos projetos | sim | não | não |
| Acha segredos vazados nas transcrições | sim | não | não |
| Desfazer (lixeira do SO) | sim | não | não |

## Build

```
cargo build --release                                     # ./target/release/aitrash
cargo build --release --target x86_64-unknown-linux-musl  # binário 100% estático
```

## Lançar uma versão

O workflow `.github/workflows/release.yml` é gerado pelo [cargo-dist](https://github.com/axodotdev/cargo-dist) (`dist generate`; não edite à mão). Para lançar:

1. Suba `version` no `Cargo.toml`.
2. Rode `git commit -am "release: v0.2.0" && git tag v0.2.0 && git push --follow-tags`.

O CI compila para Linux (x86_64/arm64, estático), macOS (Intel/Apple Silicon) e Windows. Depois publica os binários, os instaladores e os checksums no GitHub Releases, e o pacote `aitrash` no npm. O npm usa trusted publishing (OIDC), sem token: o pacote `aitrash` confia no workflow `release.yml` deste repositório.

## Licença

MIT
