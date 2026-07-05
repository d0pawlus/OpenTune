# M4 — decyzje i uwagi (notatnik wykonawczy)

Decyzje podjęte w trakcie realizacji M4 tam, gdzie plan (Task 1 brief) zostawiał
wybór lub rzeczywistość realnego INI go korygowała. Kryterium jak w M2/M3:
ścieżka optymalna, nieblokująca przyszłego rozwoju. Plan:
`docs/superpowers/plans/2026-07-04-m4-table-editors.md`; badania:
`docs/notes/m4-research.md`. Fixture golden-gate: `speeduino.ini` @
`0832dc1d25b108cf33b30167284c44e3edd3d35a` (noisymime/speeduino, GPL-3.0).

## Task 1 — Wall #1 (scattered comms keys)

- **`SCATTERED_COMMS_KEYS` zaimplementowane wg briefu 1:1** — `pageReadCommand`,
  `pageValueWrite`, `burnCommand`, `blockingFactor`, `blockReadTimeout`,
  `interWriteDelay`, `pageActivationDelay`, `messageEnvelopeFormat` czytane z
  `[Constants]`/`[OutputChannels]`, pierwszy element listy per-page (Speeduino
  używa identycznego szablonu na każdą stronę).
- **Odkryty przy realnym pliku: inline-comment na wartości scattered muszą być
  ucinane PRZED podziałem po przecinku** — `blockingFactor = 251 ; Serial
  buffer is 257 bytes...` (l.259 @ 0832dc1d) i `interWriteDelay = 10 ;Ignored
  when tsWriteBlocks is on` (l.266) inaczej trafiają do `require_u32` z resztą
  komentarza doklejoną do liczby → parse error. `extract_scattered_comms`
  woła `strip_inline_comment` (już istniejące w `parser.rs`, honoruje cudzysłowy)
  na surowej wartości przed `first_list_element`.
- **`ochGetCommand` NIE jest w `SCATTERED_COMMS_KEYS`** (zgodnie z briefem) —
  w realnym pliku nigdy nie żyje w `[Constants]`; fallback na istniejący
  `extract_och_get_command`/`[OutputChannels]` scanner, uruchamiany tylko gdy
  `[MegaTune]`/`[TunerStudio]` w ogóle nie deklaruje klucza. `parse_definition`'s
  override nadal wygrywa, gdy oba istnieją.

## Task 1 — Wall #2 (`lastOffset` = start, nie end)

- **Wszystkie 4 miejsca w `constants_fields.rs`** (scalar/array/bits/string)
  zmienione z `Known(offset + width)` na `Known(offset)` — `lastOffset`
  rozwiązuje się teraz do offsetu POCZĄTKU poprzedniego pomyślnie sparsowanego
  pola, nie jego końca. Semantyka poisoningu (nieznana klasa → `Poisoned`)
  bez zmian.
- **M2-owe przypięcia (`tests/constants.rs`) korygowane, nie chronione**:
  `parses_constants_and_pages` (`veTable`/`ego_min_lambda`/`coolantGate`/
  `engineName` — offsety 12/29/30/31 → 2/28/28/28, aliasy do poprzedników) i
  `unknown_constant_class_poisons_later_lastoffset_on_the_same_page`
  (`resumed` 6 → 5) przypinały STARĄ, błędną semantykę running-end. Kształt
  `Definition` bez zmian — poprawione tylko rozwiązane wartości, z komentarzem
  cytującym realny plik jako źródło prawdy.

## Task 1 — Wall #3 (odkryty na golden gate, nie przewidziany przez brief)

- **`[PcVariables]` obsługiwało tylko `scalar` bez offsetu — `array`/`bits`/
  `string` bez offsetu w ogóle nie były podłączone** w
  `parse_constant_line`'s `match (class, page)`: `("array", None)`,
  `("bits", None)`, `("string", None)` spadały do `UnknownClass`. Realny plik
  intensywnie używa tych klas w `[PcVariables]` (`wueAFR`, `tsCanId`,
  `AUXin00Alias`, ...; l.50-154) — 39 diagnostyk na golden gate. To luka
  gramatyki w JUŻ modelowanej sekcji (scalar-bez-offsetu już działał), więc
  naprawione parserowo (małe, lokalne: `parse_array_no_offset`/
  `parse_bits_no_offset`/`parse_string_no_offset`, ten sam wzorzec pól co
  wariant z offsetem minus pole offsetu), nie dopisane do allowlisty.
- **`$name`-referencje w listach opcji `bits` (np. `algorithmNames = bits,
  U08, [0:2], $loadSourceNames`) zapisywane dosłownie jako pojedyncza opcja**
  — ekspansja makr `$name` jest świadomie poza zakresem preprocesora
  (udokumentowane w `preprocessor.rs`); brak crashu, fail-open.

## Golden-gate allowlist — nowe wpisy (ponad 4 z briefu)

Brief przewidział tylko `commandButton`/`settingSelector`/`groupMenu`/
`groupChildMenu` (i te faktycznie występują w realnym pliku — l.2014-2019,
l.2689+, l.3279+ — więc nie są martwe). Uruchomienie golden gate na
niezmodyfikowanym pliku ujawniło 102 diagnostyki nieujęte briefem; po
naprawie Wall #3 (39 zniknęło) zostały 63, wszystkie skategoryzowane i
dopisane do `ALLOWED_DIAGNOSTICS` (`tests/real_ini.rs`) z uzasadnieniem:

- **`settingOption`** (39×) — nazwany preset konsumowany przez
  `settingSelector`, sam nie jest bindowalnym polem; brak zamrożonego
  `FieldKind`. Task 2+ gramatyka.
- **`indicator`** (6×, obejmuje też `indicatorPanel` przez podciąg) — widget
  lampki stanu (kolor wł./wył.); brak zamrożonego `FieldKind`.
- **`` `text` ``** (4×) — statyczny blok tekstu pomocniczego w dialogu.
- **`` `graphLine` ``** (3×) — definicja serii dla wbudowanego live-graph.
- **`` `liveGraph` ``** (1×) — kontener wbudowanego live-graph.
- **`` `help` ``** (1×) — link do tematu pomocy, informacyjny.
- **`` `webHelp` ``** (1×) — link do pomocy webowej, informacyjny.
- **`` `gauge` ``** (2×, jako słowo kluczowe dialogu, np.
  `gauge = fuelPressureGauge` w `[UserDefined]`) — osadzony widget gauge
  wewnątrz panelu dialogu. (Odmienne od `[GaugeConfigurations]` — te samo
  słowo w `[CurveEditor]`, np. `gauge = cltGauge`, jest po prostu cicho
  ignorowane przez parser tabel/krzywych, nigdy nie trafia do diagnostyk.)
- **`"has no bound constant name"`** (2×) — PRZEDINIOWY (M2) świadomy
  degrade: `displayOnlyField` użyty jako inline-komentarz w dialogu bez
  związanej stałej (`"#  !!! Please note that 1.0 means 100% !!!"`, l.2962,
  l.3442). Nie nowa luka — istniejący kod `parse_constant_backed_field` już
  to obsługiwał diagnostyką.
- **`` `wueAFR` ``, `` `wueRecommended` ``** — `warmup_afr_curve`/
  `warmup_analyzer_curve` (l.4904-4921) odwołują się do osi `[PcVariables]`
  (`wueAFR`/`wueRecommended`, l.71-75), nie `[Constants]`. `parse_ui`
  dostaje tylko `&parsed.constants` (`definition.rs`) — rozszerzenie
  rozpoznawania osi krzywych/tabel o `pc_variables` to gramatyka Task 2+,
  nie ściana Task 1. `Definition::constant` też celowo nie przeszukuje
  `pc_variables` (osobna przestrzeń nazw, udokumentowane w `definition.rs`).
- **`` `systemTempGauge` ``** — realny błąd w górnym pliku: gałąź Fahrenheit
  (l.5262) `systemTempGauge = systemTemp     "System Temp"         "F", ...`
  ma brakujące przecinki między nazwą kanału, tytułem i jednostką (literówka
  autorów speeduino.ini, nie luka parsera — ten sam duch tolerancji co
  `[121, 251]` na `blockingFactor` z M3).

## Uwagi procesowe

- **Kolejność commitów:** commit 1 = obie ściany (Wall #1 + Wall #2 + Wall #3,
  wszystkie parser-local) + ich testy; commit 2 = zwendorowany plik +
  golden-gate test + ten dokument. `git add -A` z briefu pominięte świadomie —
  w drzewie roboczym leżała niezwiązana, wcześniejsza zmiana w `package.json`
  (`allowScripts`) spoza zakresu tego zadania; dodawane tylko konkretne pliki.
