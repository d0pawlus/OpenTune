# M7 — notatnik decyzji (kickoff)

- **Data:** 2026-07-19
- **Zakres:** [ROADMAP.md — M7](../ROADMAP.md#m7--ai-assistant--mcp-server-),
  [ADR-0008](../adr/0008-ai-integration.md),
  [design spec AI](../superpowers/specs/2026-06-21-ai-tuning-and-analysis-design.md)
- **Stan:** wycinki zdefiniowane; plan wycinka 1 gotowy
  ([2026-07-19-m7-ai-core.md](../superpowers/plans/2026-07-19-m7-ai-core.md));
  decyzje D-1…D-7 **rozstrzygnięte przez użytkownika 2026-07-19** — wykonanie
  wycinka 1 odblokowane.

## Stan wejściowy (rekonesans 2026-07-19)

- Deterministyczny rdzeń istnieje i jest jedynym silnikiem liczb:
  `opentune-analysis` (zero zależności) z `ve_analyze` / `log_stats` /
  `detect_anomaly` / `virtual_dyno`; DTO serde+specta już są w
  `src-tauri/src/dto.rs`, mostki w `analysis_bridge.rs` / `log_bridge.rs`.
- Cała mutacja tune'a przechodzi przez jedną ścieżkę: owner (§9) →
  `Session::set_value/set_cells/burn` → walidacja INI `low`/`high` w
  `opentune-model` (`codec::encode_scalar`, odrzucenie — nie clamp).
- Symulator ma pełną ścieżkę burn/backing-memory (ADR-0004) — testy guardraili
  bez sprzętu, wzorzec wyroczni `ecu_page` w testach `session.rs`.
- **Brak** infrastruktury audytu (zero tracing/log crate) — greenfield.
- **CSP** produkcyjne (`connect-src 'self' ipc:`) blokuje wywołania AI
  z webview — HTTP do providerów musi iść przez backend (reqwest).
- **Brak** bezpiecznego magazynu sekretów (tylko localStorage + JSON w
  `app_config_dir`).
- Serial live-write nadal `SERIAL_UNSUPPORTED` — e2e mutacji tylko symulator.

## Rozstrzygnięcia pozycji otwartych ze specu (§8)

| Pozycja | Rozstrzygnięcie |
| --- | --- |
| Nazwa crate'a | `ai` (`opentune-ai`) — zgodnie z ARCHITECTURE §5.10 |
| Schematy narzędzi | zdefiniowane w planie wycinka 1 (ręczne `serde_json`, bez schemars) |
| Transport/auth MCP | decyzja D-2 poniżej |
| Fizyka virtual dyno | dostarczona w M5 — pozycja zamknięta |

## Wycinki

| # | Wycinek | Zakres | Stan | Blokery |
| --- | --- | --- | --- | --- |
| 1 | `m7-ai-core` | crate `opentune-ai`: rejestr narzędzi + polityka uprawnień + guardraile + audyt; mostek do ownera; testy na symulatorze | **w wykonaniu** | brak |
| 2 | `m7-provider-byok` | trait `AiProvider`; providerzy **Anthropic i OpenAI** (D-1); klucze w keyring (D-3); komendy + sekcja ustawień AI (opt-in) | po 1 | brak (D rozstrzygnięte) |
| 3 | `m7-assistant-ui` | panel czatu (sekcja stacked, D-4), streaming tokenów, transkrypt tool-calli, przegląd propozycji + ręczny apply istniejącą ścieżką `setCells` | po 2 | brak (D rozstrzygnięte) |
| 4 | `m7-mcp-server` | serwer MCP (rmcp 2.2, HTTP 127.0.0.1 + token, D-2) na tym samym rejestrze i guardrailach; toggle w ustawieniach; dokumentacja podłączenia Claude Code/Desktop | po 1 (∥ 3) | brak (D rozstrzygnięte) |
| 5 | `m7-closure` | ROADMAP/ARCHITECTURE/README, audyt a11y+i18n nowego UI, evidence, release v0.3.0 (D-5) | po 2–4 | brak (D rozstrzygnięte) |

Kolejność rekomendowana: **1 → 2 → (3 ∥ 4) → 5**. Każdy wycinek = osobny
plan w `docs/superpowers/plans/`.

## Decyzje — rozstrzygnięte przez użytkownika 2026-07-19

| Id | Obszar | Decyzja | Status |
| --- | --- | --- | --- |
| D-1 | Providerzy | **Anthropic + OpenAI** od razu, oba za traitem `AiProvider` (Anthropic: raw `reqwest` + SSE; OpenAI: `async-openai` 0.41 lub raw reqwest — do wyboru w planie wycinka 2) | przyjęte |
| D-2 | Transport MCP | Streamable HTTP na `127.0.0.1` w działającej aplikacji + losowy bearer token per instalacja (rmcp 2.2, walidacja host/origin); Claude Desktop przez `npx mcp-remote` — udokumentować | przyjęta rekomendacja |
| D-3 | Klucz API | crate `keyring` 4.x (OS keychain), klucz nigdy nie wraca do frontendu; fallback env var na Linuksie bez Secret Service | przyjęta rekomendacja |
| D-4 | Panel asystenta | Bez znaczenia dla użytkownika — domyślnie kolejna sekcja stacked. **M8 będzie milestone'em UX/UI**, tam wejdzie szlif layoutu | przyjęte (default) |
| D-5 | Release | **v0.3.0** po M7, tym samym pipeline'em co v0.2.0 | przyjęte |
| D-6 | Model gałęzi/PR | **Osobny PR per wycinek** (`m7-ai-core`, `m7-provider-byok`, `m7-assistant-ui`, `m7-mcp-server`, `m7-closure`) | przyjęte |
| D-7 | Limity guardraili | `max_delta_pct = 15.0`, `max_cells_per_change = 64`, `min_interval_ms = 1000`; poziom autorytetu zaszyty na `advisory` | przyjęta rekomendacja |

Konsekwencja D-1 dla wycinka 2: zakres obejmuje obu providerów od razu —
trait `AiProvider` projektowany od początku pod dwie implementacje (różne
wire-shape'y: Anthropic `tool_use`/`tool_result` vs OpenAI
`tool_calls`/`role:"tool"` — cienki adapter w każdej implementacji).

Zapowiedź: **M8 = UX/UI** (przeprojektowanie/szlif interfejsu) — panel
asystenta w M7 świadomie minimalny.

## Ryzyka progowe

- rmcp iteruje szybko (2.2.0 z 2026-07-08) — pin `2.2.x`, re-check
  sygnatur makr (`tool_router`, `Parameters<T>`) przy implementacji wycinka 4.
- Drift API Anthropic bez SDK (adaptive thinking zamiast `budget_tokens`,
  odrzucane `temperature` na Opus 4.7+) — cały wire-shape w jednym module.
- Tokeny streamowane do UI nie mogą walczyć z pętlą rAF gauge'ów —
  throttling aktualizacji stanu czatu.
- Deferred-polish M5/M6 żyje tylko w zamkniętych PR-ach (wbrew notatce
  o utrwalaniu follow-upów) — przenieść do issues przy okazji wycinka 5.

## Wiadomości do wysłania

Brak — kickoff nie wymaga komunikacji zewnętrznej.
