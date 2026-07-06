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
- **`indicator`** (7×: 6× `indicator` + 1× `indicatorPanel`, ta druga ujęta
  przez podciąg — poprawka licznika w Task 2, wcześniej błędnie 6×) — widget
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
  **Rozwiązane w Task 2** — patrz sekcja "Task 2" poniżej; te dwa wpisy
  usunięte z `ALLOWED_DIAGNOSTICS` (już nie matchują niczego).
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

## Task 2 — pełna gramatyka `[TableEditor]`/`[CurveEditor]` + `[VeAnalyze]`

Zakres sankcjonowany przez kontrolera wykraczający poza dosłowny brief
(resolutions z Task 1 review), zwinięty do Task 2:

- **`find_raw`/scattered-comms fix (osobny commit `fix(ini):` PRZED
  `feat(ini):`)** — `find_raw` jest last-wins, ale `extract_scattered_comms`
  była doklejana PO parach z sekcji głównej, więc przy kluczu zadeklarowanym
  w obu miejscach scattered wygrywałby — sprzecznie z komentarzem
  twierdzącym "first-wins keeps the MegaTune value". Wcześniej nieosiągalne
  (realny plik nigdy nie deklaruje żadnego z 8 `SCATTERED_COMMS_KEYS` w
  `[MegaTune]`/`[TunerStudio]` — zweryfikowane grepem, stąd zmiana
  bezpieczna/no-op na golden gate). Naprawione: `extract_scattered_comms`
  budowane PIERWSZE, potem `extend`-owane parami z sekcji głównej, żeby
  last-wins trafiał na wartość `[MegaTune]`/`[TunerStudio]`. Przypięte nowym
  testem `megatune_value_wins_when_key_is_also_scattered_into_constants`
  (`tests/parse_comms.rs`) z jawnie różnymi wartościami w obu miejscach.

- **Cross-reference tabel/krzywych rozszerzony o `[PcVariables]`** —
  `ui_table_curve_parser.rs`'s `is_known_constant` sprawdza teraz OBA
  `constants` i `pc_variables` (wcześniej tylko `constants`, przekazywane
  przez `parse_ui`/`definition.rs`). To usuwa dokładnie 2 wpisy z golden-gate
  allowlisty: `` `wueAFR` ``/`` `wueRecommended` `` (potwierdzone: obie nazwy
  są w `[PcVariables]`, l.71/75 realnego pliku; po zmianie 0 diagnostyk dla
  tych dwóch krzywych). `` `systemTempGauge` `` (1 diagnostyka
  `[GaugeConfigurations]`) POZOSTAJE — to literówka górnego pliku (brakujące
  przecinki, l.5262), `gauges_parser.rs` poza zakresem plików Task 2, nie
  naprawiane tutaj.

- **`groupMenu`/`groupChildMenu` usunięte z allowlisty** — zweryfikowane
  uruchomieniem golden gate z tymczasowym dumpem diagnostyk: 0 dopasowań.
  `ui_parser.rs`'s `parse_menu_line` już ciche je toleruje (brak
  reprezentowalnego celu pod zamrożonym `MenuItem`); wpisy w allowliście
  były martwe od początku (Task 1 przewidział je z brief 1.7, ale menu
  parser konsumuje je bez diagnostyki).

- **Bare `indicator` rozbity na `` `indicator` ``/`` `indicatorPanel` ``** —
  te dwie precyzyjne, otoczone backtickami formy są rozłączne (detal dla
  `indicatorPanel` nigdy nie zawiera dokładnego podciągu `` `indicator` ``)
  i razem nadal pokrywają wszystkie 7 realnych wystąpień (6× `indicator` +
  1× `indicatorPanel`, l.3195-3201). Poprawiony też literówka licznika w
  sekcji powyżej (6× → 7×; `indicatorPanel` był już liczony przez podciąg,
  ale suma była błędnie podana jako 6 zamiast 7).

- **Widening resolution nie dotyczy `Definition::constant()`** —
  `is_known_constant` w `ui_table_curve_parser.rs` to lokalna, wyłącznie
  diagnostyczna pomoc; publiczne `Definition::constant()`/`pc_variables`
  jako osobna przestrzeń nazw pozostają bez zmian (`definition.rs`).

- **`warmup_analyzer_curve` — multi-series curve, nie do przedstawienia pod
  zamrożonym `CurveDef`** (real file l.4915-4923): drugi `yBins =
  wueRecommended` (po `yBins = wueRates`) plus dwa `lineLabel = "..."` —
  legenda dla dwuseriowej krzywej (bieżąca WUE vs. rekomendowana z
  analizatora). Zamrożony `CurveDef` ma jeden slot `y_bins`/brak pola na
  serie/legendy. Zdecydowano: NIE przerabiać zamrożonego kształtu.
  `lineLabel` cicho ignorowany (brak celu — ten sam duch co `topicHelp` w
  `[CurveEditor]`, CurveDef nie ma pola `help`); drugi `yBins` po prostu
  nadpisuje pierwszy (last-wins, jak każdy inny atrybut pojedynczej
  wartości w tym module) — `y_bins` tej krzywej ostatecznie = `wueRecommended`,
  nie `wueRates`. Zero nowych diagnostyk (zgodne z golden gate). Zgłoszone
  do kontrolera jako ograniczenie warte rozważenia w przyszłym zadaniu, gdy
  `[TableEditor]`/`[CurveEditor]` będą renderowane (Tasks 5/6): jeśli
  multi-series ma znaczenie funkcjonalne, `CurveDef` będzie wymagał
  rozszerzenia (poza zakresem Task 2 — kształty są zamrożone).

- **Port-note (`ui_table_curve_parser.rs`) rozszerzony** o pełny zestaw pól
  hyper-tuner (`Table`/`Curve` z `config.ts:153-176`), `© 2021 Piotr
  Rogowski`, i notatkę że `x_channel`/`y_channel` to nasze rozszerzenie
  (hyper-tuner capturuje 2. token `xBins`/`yBins` ale nigdy go nie używa).

- **DTO/bindings** — `TableDto` rozszerzone (`title`, `page`, `x_channel`,
  `y_channel`, `xy_labels`, `up_down_label`, `help`; `grid_height`/
  `grid_orient`/`map3d_id` świadomie NIE projektowane — nieużywane przez
  frontend); nowe `AxisDto`/`CurveDto`; `DefinitionDto.curves` (Vec, nie
  Option — jak `tables`). Bindings.ts się zmienił (spodziewane); 3 istniejące
  frontendowe fixtures (`App.integration.test.tsx`,
  `Dashboard.test.tsx`, `DialogEngine.test.tsx`) potrzebowały jednoliniowej
  poprawki (`curves: []`) obok istniejącego `tables: []` — `DefinitionDto`
  ma `curves` jako required `Vec`, nie `Option`, symetrycznie z `tables`.

- **`lambdaTargetTables` i `[WueAnalyze]` — świadomie odroczone** (to jest
  wpis, do którego odsyłają doc-comments `ve_analyze.rs` i
  `ve_analyze_parser.rs`; recenzja Task 2 wykryła, że odsyłacz wisiał bez
  celu — domknięte tutaj). Klucz `lambdaTargetTables` (lista tabel celu
  lambda/AFR obok `veAnalyzeMap`) jest cicho pomijany — zamrożony
  `VeAnalyzeDef` reprezentuje pojedynczy mapping `veAnalyzeMap`, który w
  gałęzi `#else` realnego pliku (nasz target: AFR, nie lambda) niesie
  wszystko, czego M4 Task 11 (deterministyczny `ve_analyze`) potrzebuje.
  `[WueAnalyze]` (analog dla warmup enrichment) nie jest parsowany w ogóle —
  parser dopasowuje sekcję ścisłym `inner.trim() == "VeAnalyze"`, więc
  `[WueAnalyze]` nie jest połykany (dowód: golden gate zielony bez wpisu w
  allowliście dla kluczy `wueAnalyzeMap`). Oba do ewentualnego podjęcia,
  gdy analiza WUE/lambda wejdzie do zakresu (post-M4); rozszerzenie
  `VeAnalyzeDef` będzie wtedy addytywne.

## Task 3 — `set_cells`: zapis komórek per-gest (model → session → owner → IPC)

- **Decode-modify-set zamiast równoległej ścieżki bajtowej** —
  `Tune::set_cells` dekoduje całą tablicę (`get`), nakłada komórki gestu i
  re-enkoduje przez istniejące `Tune::set`. Dzięki temu walidacja zakresów
  (per-element `low`/`high` z `encode_scalar` → `ModelError::OutOfRange`),
  dirty-tracking i undo są współdzielone verbatim z M2 — zero nowej logiki
  kodeka, zero ryzyka dywergencji. Walidacja indeksów (out-of-bounds →
  `TypeMismatch`) i typu (skalar → `TypeMismatch`) dzieje się PRZED dotknięciem
  jakiegokolwiek bajtu; odrzucony gest nie zostawia śladu.
- **Jeden gest = jeden `Edit` = jeden krok undo** — cała paczka komórek
  (paste/smooth/multi-select w przyszłym edytorze tabel) przechodzi przez
  jedno wywołanie `set`, więc `undo()` cofa cały gest atomowo. Pusty gest
  (`&[]`) to świadomy no-op (`Ok`), bez wpisu undo.
- **Trade-off ciągłego spanu zaakceptowany** — `page_deltas` (session.rs)
  diffuje strony do JEDNEGO ciągłego spanu `first_changed..=last_changed`;
  odległa komórka rozciąga span i bajty pomiędzy są przepisywane identycznymi
  wartościami. Dla realnych gestów edytora (sąsiadujące komórki) span jest
  minimalny; koszt gorszego przypadku to nadmiarowe bajty na wire przy
  identycznej zawartości — przypięte testem
  `set_cells_reaches_the_wire_as_one_contiguous_span` jako udokumentowane
  zachowanie, nie bug.
- **`Session::set_cells` lustrzane wobec `set_value`** — walidacja na klonie,
  wire przed commitem, `TuneDirtyEvent` z modelu; arm `SetCells` w ownerze w
  kształcie arma `SetValue` (`with_session` + `emit_dirty` + dokładnie jedna
  odpowiedź). `CellEditDto { index: u32, value: f64 }` mapowane na `(u32, f64)`
  na granicy ownera — krotki pozostają wewnętrzne dla Rusta (specta 0.0.12
  nie zniesie usize/u64 przez IPC).
- **Fixture testowe 4x8 zamiast 16x16 z briefu** — wspólne fixture modelu
  (`tests/common/mod.rs`) ma strony 64-bajtowe; pełne 16x16 U08 (256 B) się
  nie mieści. Zachowania pod testem (multi-cell, bounds, range, jeden krok
  undo) są niezależne od kształtu; indeks 17 pozostaje ważny, 9999 poza
  zakresem — asercje briefu weszły verbatim. INI sesyjnego testu deklaruje
  analogiczny `veTable = array, U08, 0, [4x8]` (bundlowany sample INI nie ma
  żadnej tablicy).
- **Pin sygnatury z Task 0 usunięty z `contract.rs`** — istniał wyłącznie po
  to, by przypiąć seam bez wywoływania `todo!()`; realne testy zachowania
  (`tests/tune.rs::set_cells_*`) ćwiczą teraz dokładnie tę sygnaturę.
- **Staging jak w Task 1** — `git add -A` z briefu pominięte; w drzewie nadal
  leży niezwiązana zmiana `package.json` (`allowScripts`), dodawane tylko
  konkretne pliki zadania + zregenerowany `src/ipc/bindings.ts`.
