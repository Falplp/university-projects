# Mission Control

Terminálová Mission Control platforma v Rustu a Ratatui pro sledování AI agentů. Zobrazuje stav pipeline, komunikaci s LLM, MCP nástroje, tokeny, cenu, dobu kroků, chyby, historii, SOUL.md a artefakty. Obsahuje Human-in-the-Loop schválení, SQLite persistenci a bezpečně omezený přístup k souborům a příkazům.

## Rychlé spuštění

```bash
cargo run
```

Testy a statická kontrola:

```bash
cargo test --all-targets
cargo check --all-targets
```

## Ovládání

- `1`–`4`, `j` / `k`, šipky nebo myš — vybrat agenta
- `Tab`, `←` / `→` — Live, History, Errors, SOUL.md, Artifacts, LLM Output a Task Queue
- `t` — zadat nový úkol; během běžící pipeline se úkol přidá do fronty, jinak spustí nový běh; `Enter` potvrdí a `Esc` zruší
- `l` — otevřít frontu čekajících úkolů
- `g` — spustit vybraného agenta přes jeho nakonfigurovaný LLM provider
- `m` — otevřít výběr modelu/provideru pro označeného agenta
- `f` — vypsat obsah povoleného workspace
- `v` — bezpečný `git status` bez shellu; vyžaduje povolený terminál
- `o` — připojit OpenClaw Gateway; po připojení znovu stisknout `o` a vytvořit skutečný session úkol
- `b` — načíst dávku z `tasks.txt` a postupně ji zpracovat přes OpenClaw
- `x` — zastavit aktivní OpenClaw session
- `w` — změnit workspace; `c` — změnit téma
- `a` — schválit výsledek v Human Review
- `r` — otevřít formulář zpětné vazby a vrátit úkol generátoru
- `Space` — pozastavit/obnovit demo pipeline
- `?` / `F1` — nápověda; `q` / `Esc` — konec

## Konfigurace

Vše je v `config.toml`: agenti, role, modely, provider, povolené nástroje, SOUL.md, workspaces, databáze a bezpečnost. Agenti v TUI se vytvářejí z této konfigurace, nejsou pevně zapsaní v UI.

Podporované LLM providery:

- `demo` — offline provider vhodný pro testy,
- `ollama` — lokální Ollama (`OLLAMA_BASE_URL`),
- `openai` — klíč v `OPENAI_API_KEY`,
- `anthropic` — klíč v `ANTHROPIC_API_KEY`.

Klávesa `m` zobrazí profily Offline Demo, Ollama/Qwen, OpenAI/GPT-4o a volitelně Anthropic/Claude. Výchozí pipeline používá OpenAI pro Architecta i Reviewera a Ollamu pro Generatora. Vyberte profil šipkami a potvrďte Enterem, nebo na něj klikněte. Klávesa `g` potom spustí HTTP request na pozadí, takže UI nezamrzne. Tokeny z odpovědi se přičtou agentovi a úplný text je v záložce `LLM Output`. Pro první bezpečný pokus vyberte profil `Offline Demo`, který nepotřebuje API klíč ani běžící server.

## Skutečný úkol od začátku do schválení

Klávesou `t` zadejte vlastní úkol, například `Vytvoř Python REST endpoint pro health check`, a potvrďte Enterem. Aplikace potom automaticky:

1. pošle zadání Architectovi a uloží jeho specifikaci,
2. předá specifikaci Code Generatoru,
3. zapíše Generatorovy bloky `<file path="...">...</file>` do povoleného workspace,
4. předá artefakty Reviewerovi,
5. zastaví se v Human Approval.

Pokud během běhu zadáte další úkoly přes `t`, aktivní úkol zůstane nedotčený. Čekající úkoly se zobrazí v `Task Queue` a po `Approve` se spouštějí postupně FIFO.

Před Human Approval se v záložce `Artifacts` zobrazí diff změněných souborů. To umožní zkontrolovat přidané a odebrané řádky před schválením.

Pro skutečné modely nastavte model každého agenta přes `m`, nebo odpovídající hodnoty v `config.toml`. Architect a Reviewer používají OpenAI, Generator Ollamu. Anthropic zůstává pouze volitelný provider a není součástí výchozího běhu. Bez API klíče nebo běžícího Ollama serveru se chyba zobrazí v `Errors` a pipeline se bezpečně zastaví.

### Ollama

Na hostu spusťte Ollamu a stáhněte model nastavený pro Code Generator:

```bash
ollama serve
ollama pull qwen2.5-coder:14b
```

Lokální běh používá `http://localhost:11434`; Docker Compose nastavuje pro kontejner `http://host.docker.internal:11434`. Pro cloud providery vložte `OPENAI_API_KEY` a `ANTHROPIC_API_KEY` do `.env`, nikdy přímo do `config.toml`.

## Přístup k souborům notebooku

Aplikace nemá automaticky přístup k celému notebooku. Vidí pouze adresář nastavený jako `security.workspace_root`. Cesty s `..`, absolutní cesty a symlinky vedoucí ven jsou odmítnuty.

Příklad pro konkrétní projekt:

```toml
[security]
workspace_root = "/home/user/projects/my-project"
read_only = false
allow_terminal = false
terminal_allowlist = ["git", "cargo", "openclaw"]
```

Pro pouhé sledování nastavte `read_only = true`. Terminál je ve výchozím stavu vypnutý. Pokud jej zapnete, příkazy se spouštějí přímo bez shellu a jen z allowlistu; TUI v současnosti nabízí bezpečný `git status`.

Automatickou validaci lze nastavit například takto:

```toml
[security]
allow_terminal = true
terminal_allowlist = ["git", "cargo"]
validation_command = ["cargo", "test", "--offline"]
```

Po zápisu artefaktů se příkaz spustí v workspace bez shellu. Při `allow_terminal = false` se validace z bezpečnostních důvodů přeskočí a událost se zobrazí v logu.

## Live event adapter

Aplikace průběžně sleduje `events.jsonl` a načítá jen nové řádky. Vadný řádek se zobrazí jako chyba, ale nezastaví další události.

```json
{"timestamp":"14:35:01","agent":"code-generator","kind":"tool","message":"filesystem.write src/main.py","tokens":12,"duration_ms":240}
```

## SQLite persistence

Pipeline, eventy, chyby a metriky se ukládají do `mission_control.sqlite3`. Cestu lze změnit pomocí `database_path` nebo `MISSION_CONTROL_DB`. Databáze se vytvoří automaticky.

## OpenClaw WebSocket

Aplikace používá skutečný OpenClaw Gateway WebSocket protokol v4. Připojení provede challenge/connect handshake s `operator.read`, `operator.write` a `operator.approvals` a načte roster přes `sessions.subscribe`. Pro každý úkol nejprve vytvoří prázdnou session přes `sessions.create`, počká na potvrzení `sessions.messages.subscribe` a teprve potom odešle zadání přes `sessions.send`. Díky tomu nelze zmeškat koncovou lifecycle událost ani přeskočit další úkol ve frontě.

Gateway musí běžet a token musí být pouze v prostředí procesu:

```bash
openclaw gateway status --deep
openclaw gateway auth-token --show
```

Druhý příkaz zobrazí tajný token v interaktivním terminálu. Token nikam neposílejte a nevkládejte do historie příkazů. Bezpečně jej načtěte například skrytým vstupem a spusťte aplikaci ve stejném shellu:

```bash
read -rsp "OpenClaw token: " OPENCLAW_GATEWAY_TOKEN; echo
export OPENCLAW_GATEWAY_TOKEN
cargo run
```

V TUI stiskněte `o` pro připojení. Po zobrazení `OPENCLAW CONNECTED` stiskněte `o` znovu, napište úkol a potvrďte Enterem. Session se objeví v `Task Queue`, živé události v `Live Overview` a chyby v `Errors`. Klávesa `x` aktivní session zastaví.

### Dávka úkolů ze souboru

Do `tasks.txt` napište jeden úkol na jeden řádek. Prázdné řádky a řádky začínající `#` se ignorují:

```text
# Příprava dokumentace
Prozkoumej workspace a vytvoř soubor project-summary.md.
Spusť dostupné testy a vytvoř soubor test-report.md s výsledky.
Zkontroluj README.md a doplň chybějící instrukce ke spuštění.
```

V TUI stiskněte `b`. Aplikace se podle potřeby připojí ke Gateway a úkoly spustí postupně. Následující úkol začne až po úspěšném dokončení nebo chybě předchozího. Aktuální stav je v záložce `Task Queue`.

Výsledek celé dávky se ukládá do jediného souboru `logs/openclaw-task-results.log`. Pro každý úkol obsahuje stav `SUCCESS` nebo `ERROR`, text zadání, session a detail výsledku; na konci je souhrn počtu úspěchů a chyb. Cesty lze změnit v `config.toml`:

```toml
[openclaw]
task_input_file = "tasks.txt"
task_report_file = "logs/openclaw-task-results.log"
```

## Notifikace

Při přechodu do Human Review lze odeslat obecný webhook nebo Telegram zprávu. Zkopírujte `.env.example` na `.env` a nastavte pouze potřebné hodnoty. Prázdné hodnoty funkci bezpečně vypnou.

## Docker

```bash
cp .env.example .env
mkdir -p workspace
docker compose build
docker compose run --rm mission-control
```

Kontejner vidí jen `./workspace` připojený jako `/workspace`; SQLite je v pojmenovaném volume. Ollama zůstává na hostu a kontejner ji oslovuje přes `host.docker.internal:11434`. API klíče patří pouze do `.env`, který se nemá commitovat.

## Co je reálné a co demonstrační

Reálné jsou provider HTTP konektory, OpenClaw WebSocket v4 session řízení a streaming, SQLite, živé JSONL události, bezpečnost cest, Git/CLI allowlist, SOUL.md, notifikace, fronta úkolů, diff artefaktů, konfigurovatelná validace a uživatelské workflow.
