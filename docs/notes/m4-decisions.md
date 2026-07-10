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

## Task 4 — Frontend table-editing core: selection, ops, TSV, heatmap

- **WRITE FRESH potwierdzone** (ADR-0006) — hypertuner-cloud (MIT) to
  read-only viewer bez selection/ops/clipboard do zapożyczenia; LibreTune
  (GPL-2) tylko do studiowania semantyki. Cztery moduły
  (`selection.ts`/`tableOps.ts`/`tsv.ts`/`heatmap.ts`) napisane od zera wg
  pinów briefu, zero portowanego kodu.
- **Interpolate = bilinear zakotwiczony na rogach** — cztery rogi rect
  pozostają nietknięte, reszta rekonstruowana z nich (`fr`/`fc` jako ułamki
  0..1 wzdłuż wiersza/kolumny); 1×N/N×1 degeneruje się do liniowej (h=0 lub
  w=0 → odpowiedni ułamek = 0); pojedyncza komórka to no-op (`h===0 &&
  w===0` na wejściu). Decyzja niepisana wprost w briefie ale spójna z jego
  duchem: jeśli którykolwiek róg jest nie-skończony, `interpolateRect`
  zwraca `[]` całościowo (nie da się zakotwiczyć na NaN) — bezpieczny
  fail-closed zamiast częściowej interpolacji z jednym rogiem "zgadywanym".
- **Smooth = jeden przebieg jądra 3×3 (środek 4, krawędź 2, róg 1) z
  przycięciem okna i renormalizacją** — sąsiedzi poza granicami siatki są
  po prostu pomijani (nie traktowani jako 0), a `weight` sumuje tylko wagi
  faktycznie użytych sąsiadów, więc dzielnik renormalizuje się do
  rzeczywistego rozmiaru okna (potwierdzone przypadkiem testowym z rogu
  siatki: dzielnik 9 zamiast 16). Sąsiedzi są czytani z całej siatki
  (niezależnie od `rect`), zapisy trafiają wyłącznie do komórek wewnątrz
  `rect` — spike poza zaznaczeniem nigdy nie jest nadpisywany, ale nadal
  wpływa (jako odczyt) na wygładzanie sąsiadującej komórki w zaznaczeniu.
- **Sentinel NaN/null — "nigdy edytowane, nigdy nie kontrybuują"
  zaimplementowane jednolicie we wszystkich pięciu operacjach** —
  `editable = Number.isFinite`; `scaleRect`/`stepRect` filtrują nie-skończone
  komórki z wyniku (nie emitują dla nich `CellEdit`); `setEqualRect` pomija
  je i przy liczeniu średniej, i przy zapisie (dodatkowo: gdy WSZYSTKIE
  komórki zaznaczenia są nie-skończone, zwraca `[]` zamiast `NaN` —
  przypadek brzegowy nieopisany w briefie, rozstrzygnięty na rzecz
  fail-closed); `smoothRect` pomija nie-skończony środek całkowicie (bez
  edita) i nie-skończonych sąsiadów w sumie ważonej; `interpolateRect`
  pomija nie-skończone komórki niebędące rogami (kontynuuje pętlę) oraz
  całościowo odrzuca rect z nie-skończonym rogiem (patrz wyżej).
- **`CellEdit.index` = ten sam row-major flat index co `CellEditDto.index`
  z Task 3** — `idx(g,r,c) = r*g.cols+c`, identyczne z `cellIndices`
  (selection.ts) i z tym, czego oczekuje `set_cells` po stronie Rusta;
  żadnego mapowania pośredniego, Task 5 może przekazać `CellEdit[]`
  bezpośrednio jako payload komendy.
- **TSV: `toTsv`/`parseTsv` — puste/nieliczbowe komórki i przecinek jako
  separator dziesiętny** — `toTsv` renderuje nie-skończone wartości jako
  pusty string (nie `"NaN"`), żeby wklejenie z powrotem nie psuło
  `parseTsv`; `parseTsv` akceptuje przecinek PL-locale (`replace(",", ".")`
  przed `Number(...)`) i odrzuca CAŁY blok (`null`), jeśli którakolwiek
  komórka się nie parsuje — brief określał to wprost. Decyzja dodatkowa:
  pojedynczy końcowy pusty wiersz (typowy artefakt kopiowania z arkusza
  kończącego się `\n`) jest cichy odrzucany przed parsowaniem, żeby nie
  tworzyć widmowego pustego wiersza na końcu wklejanego bloku.
- **`pasteEdits` przycina tylko do granic siatki, nie filtruje
  nie-skończonych komórek docelowych** — brief mówi wyłącznie "clipped to
  grid"; wklejenie na nie-skończoną komórkę docelową nadpisuje ją (paste
  jest jawną akcją użytkownika, inaczej niż smooth/interpolate, które
  działają automatycznie na całym zaznaczeniu) — rozstrzygnięcie przyjęte
  jako spójne z resztą briefu, który nigdzie nie każe blokować paste na
  NaN.
- **Heatmap: `heatColor`/`heatRgb` dzielą jeden `hueOf` po jednym `heatT`**
  — `hue = round(220*(1-t))` (220°=niebieski przy t=0, 0°=czerwony przy
  t=1), stała saturacja/lightness 70%/55% zgodnie z briefem;
  `heatRgb` konwertuje TEN SAM hue przez standardowy 15-liniowy
  `hslToRgb` (h w stopniach, s/l w 0..1), więc `heatColor` (CSS) i
  `heatRgb` (three.js vertex colors) są gwarantowane spójne kolorystycznie
  — jedno źródło prawdy (`hueOf`), nie dwie niezależne implementacje skali.
  Zdegenerowany zakres (`lo >= hi`) zwraca `t=0.5` (środek skali) zamiast
  dzielenia przez zero.
- **Staging jak w Task 1/2/3** — `git add -A` z briefu pominięte (dirty
  `package.json`/`allowScripts` nadal niezwiązane z tym zadaniem); dodane
  tylko `src/components/table-editor/*` + ten wpis w
  `docs/notes/m4-decisions.md`.

## Task 5 — edytor tabel 2D: DOM grid, klawiatura, schowek, wpięcie w store

- **Model a11y — jedna powierzchnia klawiaturowa + roving cell przez
  `aria-activedescendant`** (zarejestrowany wyjątek ARCHITECTURE §3, decyzja 6):
  semantyczny `<table role="grid">` (nagłówki `<th scope="col/row">`,
  `<td role="gridcell" aria-selected>`), owinięty w `div.te-surface` z
  `tabIndex=0`, `onKeyDown` i `aria-activedescendant={gridId}-{index}`.
  Wrapper dostał `role="application"` — brief nie przypisywał mu roli, a
  `aria-activedescendant` jest ważne tylko na rolach kompozytowych;
  `application` jest uczciwym gospodarzem (fokus zostaje na wrapperze,
  aktywna komórka ogłaszana przez id). Komórki NIE są fokusowalne — zero
  roving-tabindex, jedna powierzchnia, jak przypiął brief.
- **Inwersja wierszy wyświetlania: góra = najwyższe obciążenie** (konwencja
  tuningowa) — wiersz wyświetlany `d` mapuje się na wiersz danych
  `rows-1-d` WYŁĄCZNIE w `TableGrid` (renderowanie) ; selection/ops/store/
  indeksy `CellEdit` pozostają w row-major porządku danych 1:1 z
  `CellEditDto.index` Taska 3. Konsekwencja klawiszowa: `ArrowUp` = wiersz
  danych +1 (wizualnie w górę), `ArrowDown` = -1; "Enter = commit + w dół
  ekranu" = wiersz danych -1, z zaciskiem na krawędzi.
- **Keymap — rozstrzygnięcia poza literą briefu:** (1) `+`/`-` mają
  pierwszeństwo przed type-to-edit, więc `-` NIE otwiera draftu ujemnego —
  wartości ujemne wpisuje się przez Enter (draft zasiany bieżącą wartością)
  albo dopisanie `-` w otwartym draftcie (klawisze operacji przepuszczane do
  inputa, gdy draft otwarty lub trzymany modyfikator); (2) `Shift+=` daje
  `+` na układzie US, więc krok przez `+` z Shift = ×10 zgodnie ze
  skeletonem briefu (`step * (e.shiftKey ? 10 : 1)`) — krok ×1 przez `+`
  wymaga klawiatury numerycznej, przyjęte świadomie; (3) strzałki/Tab przy
  otwartym draftcie commitują draft i dopiero przesuwają (zachowanie
  arkuszowe); (4) draft startuje tylko na komórce skończonej — komórki
  NaN/null ("—") nie da się edytować ani Enterem, ani type-to-edit.
- **Polityka paste/NaN (decyzja kontrolera, zaimplementowana w warstwie
  edytora):** `pasteEdits` (Task 4, zamrożone) przycina tylko do granic —
  edytor filtruje edits, których BIEŻĄCA komórka docelowa jest
  nie-skończona (`Number.isFinite` na wartości siatki), zanim wyśle gest.
  Paste jest więc spójny z pięcioma operacjami (nigdy nie edytują komórek
  nie-skończonych). Miły efekt uboczny round-tripu: `toTsv` renderuje NaN
  jako pustą komórkę, `parseTsv` parsuje pustą jako 0 (brief-faithful, bez
  zmian w tsv.ts) — ale skopiowany blok z dziurą NaN wklejony z powrotem w
  to samo miejsce NIE nadpisze dziury zerem, bo filtr odrzuci edit na
  nie-skończonym celu. Zero na skończonym celu z pustej komórki wklejonej
  skądinąd pozostaje możliwy — świadomie, `parseTsv` jest zamrożony.
- **Zakres heatmapy — "both literal":** `low`/`high` stałej Z użyte tylko
  gdy OBA są literalne (bound `{expr}` projektuje się na null), inaczej
  finite min/max danych — dosłownie wg briefu, nie per-bound.
- **Keyed remount zamiast reset-effect:** eslint (`react-hooks/
  set-state-in-effect`, v7) blokuje `setState` w efekcie resetującym stan
  lokalny przy zmianie tabeli. Rozwiązane idiomatycznie: zewnętrzny
  `TableEditor` wybiera tabelę ze store'a i renderuje wewnętrzny `Editor`
  z `key={table.name}` — selection/draft/error/view/scaleFactor resetują
  się przez remount, zero efektów resetujących.
- **Podział plików (limit <400 linii):** kontener urósł do 430 linii, więc
  toolbar wydzielony do prezentacyjnego `TableToolbar.tsx` (title + hint
  `upDownLabel`, operacje, scale factor + Apply, przełącznik 2D/3D, link
  help) — kontener 386, grid 148, toolbar 93. Podział pozostaje uczciwy:
  klawiatura/dane/commit wyłącznie w kontenerze, prezentacja w liściach.
- **Fixture testowe 2×3 zamiast 2×2 z briefu:** w siatce 2×2 każda komórka
  recta pełnego zaznaczenia jest rogiem, więc `interpolateRect` zwraca `[]`
  i asercja (f) briefu ("Interpolate dispatches the Task 4 edits") nie
  miałaby czego obserwować (zamrożony `applyEdits` early-returnuje na
  pustych edits). Minimalny kształt z wnętrzem: 2×3, interpolacja 1×3
  wzdłuż wiersza → dokładnie jeden edit `{index:1, value:61}`.
- **Nawigacja w `TunePanel`:** trzy `<nav.tune-menu>` (menu/tables/curves)
  owinięte w nowy `div.tune-navs` — `.tune-body` to grid 2-kolumnowy,
  więc trzy navy jako bezpośrednie dzieci łamałyby układ; border/padding
  kolumny przeniesione z `.tune-menu` na `.tune-navs` (dialogs.css). Blok
  curves renderuje się dopiero gdy definicja ma krzywe (sim INI: brak —
  Task 6 to podejmie). Content area: `activeTable` wygrywa nad
  `activeDialog` (store i tak gwarantuje wyłączność — settery czyszczą
  pozostałe dwa).
- **`TableField.tsx` usunięty; reguły `.table-*` NIE migrowane** — brief
  każe migrować reguły "still-referenced", a po usunięciu TableFielda
  żaden plik nie używa `.table-field/-title/-grid/-cell/-empty`
  (zweryfikowane grepem) → usunięte z dialogs.css z komentarzem-nagrobkiem.
  Build (tsc) dowodzi braku wiszących importów.
- **Selekcja startowa = komórka danych (0,0)** — wizualnie lewy-dolny róg
  (najniższe obciążenie/RPM), spójne z tym, że indeks 0 jest kanoniczny w
  testach briefu; brief nie przypinał pozycji startowej.
- **Dwa testy dołożone do suite'u Taska 4 (sankcjonowany fold-in, review
  Minor):** `interpolateRect` → `[]` gdy róg recta nie-skończony (guard
  tableOps.ts:46), `setEqualRect` → `[]` gdy WSZYSTKIE komórki zaznaczenia
  nie-skończone (guard :113). Oba przechodzą na niezmienionym tableOps.ts —
  pokrywają istniejące guardy, nie zmieniają semantyki.
- **Staging jak w Task 1-4** — `git add -A` z briefu pominięte (dirty
  `package.json`/`allowScripts` nadal poza zakresem); jawne ścieżki +
  `git rm` na `TableField.tsx`.

## Task 6 — edytory krzywych 1D: ponowne użycie siatki + podgląd SVG

- **Korekta Taska 2: `yBins` krzywej — first-wins, nie last-wins**
  (sankcjonowany fold-in, kontroler, PRZED właściwym Taskiem 6, osobny commit
  `fix(ini):`). Task 2 (patrz wyżej, sekcja "`warmup_analyzer_curve` —
  multi-series curve") świadomie zostawił last-wins dla drugiego `yBins`
  krzywej `warmup_analyzer_curve` (l.4915-4923 realnego pliku): `yBins =
  wueRates` (edytowalna tablica `[Constants]`) nadpisywane przez `yBins =
  wueRecommended` (tylko-do-odczytu wyjście analizatora z `[PcVariables]`).
  To narusza zamrożony kontrakt dokumentacyjny `CurveDef::y_bins` ("the
  editable data array") — edytor krzywych (ten Task) wiązałby edycje z
  polem PC-local, nieedytowalnym przez ECU. Naprawione: `set_curve_bin`
  (`ui_table_curve_parser.rs`) ustawia teraz `y_bins` TYLKO gdy jest wciąż
  puste (first-wins), wyłącznie dla `yBins` — `xBins` i każdy inny atrybut
  pojedynczej wartości w tym module (tabel i krzywych) pozostają last-wins
  bez zmian (nigdy się nie powtarzają w realnym pliku, więc minimalny diff).
  TDD: czerwony test jednostkowy
  (`curve_repeated_y_bins_keeps_the_first_editable_series`, `tests/ui.rs`) +
  rozszerzenie golden-gate (`tests/real_ini.rs`, asercja
  `warmup_analyzer_curve.y_bins == "wueRates"`) przed zmianą w parserze,
  zielone po. `lineLabel`/multi-series nadal poza zakresem (`CurveDef` ma
  jeden slot `y_bins`) — bez zmian względem Taska 2, tylko WYBÓR, która
  wartość ląduje w tym jednym slocie się odwrócił.

- **WRITE FRESH dla `curveMath.ts`** (ADR-0006, jak Task 4) — trzy czyste
  funkcje (`axisRange`/`polylinePoints`/`cursorFraction`) napisane od zera wg
  pinów briefu; zero portowanego kodu. `axisRange` woli literalne granice
  `AxisDto` (oba `min`/`max` nie-null), potem finite extents danych, potem
  `{min:0,max:1}` jako fallback zdegenerowanego przypadku (pusta/wszystko
  nie-skończone tablica) — ten sam duch "fail-closed zamiast NaN/Infinity"
  co heatmapa Taska 4.

- **Ponowne użycie siatki Taska 5 dosłownie — krzywa to `Grid` z `rows: 1`**
  (Task 4 core, przypięte briefem) — `CurveEditor` buduje
  `{rows:1, cols:n, values:ys}` i przepuszcza przez DOKŁADNIE te same moduły
  co `TableEditor`: `selection.ts`/`tableOps.ts`/`tsv.ts`/`TableGrid`. Zero
  nowej logiki zaznaczenia/operacji/schowka — `interpolateRect` na
  pojedynczym wierszu degeneruje się do liniowej interpolacji (h=0 w Task 4
  guard), `smoothRect` do jednowymiarowego jądra (sąsiedzi z rz-1/rz+1 poza
  granicami [0,1) siatki są przycinani, jak każdy brzeg). `yLabels: [""]`
  (jeden pusty nagłówek wiersza, bo `TableGrid` renderuje zawsze jeden
  wiersz `<th scope="row">`); `xLabels` z wartości `curve.x_bins`
  (`binLabels`, wzorowane 1:1 na `TableEditor`), `column_labels` jako
  osobny podpis nad siatką (inny target niż `xLabels` — `columnLabel` w INI
  to opisowe nagłówki kolumn typu "Temp"/"Duty %", nie wartości binów).

- **Podgląd: SVG statyczny + kursor imperatywny w osobnej `<line>`** —
  `<polyline>` przerysowywana normalnym reconciliation Reacta przy KAŻDEJ
  zmianie danych (tanie: krzywe mają rzędu kilkunastu punktów, nie warto
  kanwy/WebGL); wyłącznie żywy kursor (`<line ref={cursorRef}>`) omija Reacta
  całkowicie — `x1`/`x2`/`visibility` ustawiane przez `setAttribute` w pętli
  `requestAnimationFrame`, czytając `useRealtimeStore.getState().getChannel
  (curve.x_channel)` (wzorzec M3 z `GaugeCanvas`: zero stanu Reacta per
  klatka). Brak `curve.x_channel` (pusty string — krzywa bez powiązanego
  kanału live) → efekt wychodzi wcześnie, kursor nigdy się nie renderuje
  (`visibility="hidden"` to stan początkowy znacznika, nigdy nadpisywany).

- **Semantyka kursora: `cursorFraction` zwraca `null` poza zakresem osi X,
  kanał `undefined` (nigdy nie widziany) traktowany identycznie jak poza
  zakresem** — pętla rAF mapuje oba przypadki na `visibility="hidden"`,
  więc brak danych live i wartość poza skalą wyglądają tak samo (kreska
  znika), zamiast np. przypinać się do brzegu — decyzja kontrolera:
  milcząca nieobecność jest bezpieczniejsza niż myląca pozycja brzegowa.

- **Nawigacja `TunePanel`: gałąź `activeCurve` wpięta symetrycznie do
  `activeTable`** — `activeTable ? <TableEditor/> : activeCurve ?
  <CurveEditor/> : <DialogEngine/>` (store i tak gwarantuje wyłączność
  trzech `active*` pól, jak w Task 5). Blok nawigacji krzywych renderowany
  już od Taska 5 (`definition.curves.length > 0`); ten Task wypełnia tylko
  zawartość, żaden nowy JSX w nawigacji.

- **Bug złapany przez test komponentu, nie przez inspekcję: kolejność
  hooków** — pierwsza wersja miała drugi `useEffect` (kursor rAF) PO
  wczesnym `return` gałęzi "wartości jeszcze niewczytane". Przy pierwszym
  renderze (`yArray` jeszcze `null`) React woła tylko 1 efekt przed
  returnem; po zapisaniu wartości przez fetch efekt store'u wywołuje
  ponowny render, `yArray` już istnieje, funkcja przechodzi dalej i woła
  DRUGI efekt po raz pierwszy w tym renderze → "Rendered more hooks than
  during the previous render." `CurveEditor.test.tsx`'s pierwszy test
  złapał to natychmiast (real DOM render + fetch przez zamockowane IPC).
  Naprawione przeniesieniem obliczenia `xr` (i wszystkiego, czego ten hook
  potrzebuje: `xArray`/`xs`) ORAZ samego efektu rAF NAD wczesny return —
  wszystkie hooki wołane bezwarunkowo w każdym renderze, zgodnie z regułami
  Reacta. `TableEditor.tsx` nie miał tego problemu (Task 5 nie dokłada
  żadnego hooka po swoim wczesnym returnie) — to defekt specyficzny dla
  Taska 6, nieprzewidziany przez szkielet briefu.

- **`heatLo`/`heatHi` przekazywane do `TableGrid` = `yr.min`/`yr.max`** —
  `TableGrid` (zamrożony w Tasku 5) wymaga tych dwóch propsów do
  `heatColor`; krzywa nie ma odpowiednika `zConst.low/high` z tabeli w
  sensownej postaci innej niż sama oś Y, więc ponownie użyty jest DOKŁADNIE
  ten sam `yr` (z `axisRange(curve.y_axis, ys)`), który i tak zasila
  podgląd SVG — jedno źródło prawdy dla "zakresu Y", nie druga niezależna
  heurystyka. Degenerację (`min === max`) `heatColor`/`heatT` już
  obsługują (Task 4, zwraca t=0.5).

- **Scale (toolbar-only w Tasku 5) świadomie nieosiągalny w edytorze
  krzywych** — brief nie tworzy `CurveToolbar`, a `scaleRect` w Tasku 5
  jest wołany WYŁĄCZNIE z przycisku toolbara (brak skrótu klawiszowego);
  bez toolbara operacja jest więc nieużywalna z klawiatury. Pozostała
  reszta "tej samej powierzchni klawiaturowej" (strzałki/Tab/Ctrl+A/Enter/
  Esc/type-to-edit/+−/=/// s/Ctrl+C/V) działa bez zmian. Zgłoszone jako
  świadomy brak, nie przeoczenie.

- **Podział pliku pod limit 400 linii: `binValues.ts`** — cztery drobne,
  czyste funkcje odczytu `ConstantDto`/`Value` (`arrayLength`/`arrayOf`/
  `labelsOf`/`numericOf`, odpowiedniki prywatnych helperów `TableEditor.tsx`
  `arrayShape`/`binLabels`) wydzielone do osobnego modułu w
  `src/components/curve-editor/`, żeby `CurveEditor.tsx` (kontener) zmieścił
  się pod limitem (wyszło 376 linii). Nie eksportowane z `TableEditor.tsx`
  (ten task go nie dotyka — poza zakresem plików briefu) — stąd osobna,
  niewielka duplikacja logiki zamiast współdzielenia z Task 5, zaakceptowana
  świadomie; przetestowane osobno (`binValues.test.ts`).

- **`polylinePoints` — brak formatowania (`toFixed`) współrzędnych** —
  przypięty przez brief string `"10,90 190,10"` to gołe liczby całkowite;
  `toFixed(n)` dałoby `"10.00,90.00"` i złamałoby asercję. Współrzędne
  niecałkowite (typowy przypadek) renderują się z pełną precyzją
  zmiennoprzecinkową w atrybucie `points` — tanie, SVG i tak je zaokrągla
  wizualnie, brief nie wymaga zaokrąglania.

- **`axisRange`: pojedyncza `null`-owa granica traktowana jak obie
  `null`** — brief mówi "falls back ... when bounds are null" (liczba
  mnoga); przyjęto, że TYLKO gdy OBA `min` i `max` są nie-`null` wygrywa
  oś literalna, w przeciwnym razie (zero, jedna lub obie granice `null`)
  pada fallback na finite extents danych. Przypięte osobnym przypadkiem
  testowym w `curveMath.test.ts` ("does not fall back when only one bound
  is literal").

- **`cursorFraction`/`polylinePoints` — zdegenerowany zakres (`max <= min`)
  nie dzieli przez zero** — `cursorFraction` zwraca `null` (kursor chowa
  się zamiast przypinać do krawędzi lub rzucać `NaN`);
  `polylinePoints`/wewnętrzny `fractionOf` zwraca `0.5` (środek), lustrzane
  wobec `heatmap.ts`'s `heatT` (Task 4). Żaden z dwóch przypadków nie jest
  dosłownie przypięty przez brief (który testuje tylko przypadki
  skończone/w zakresie) — rozstrzygnięcie kontrolera na rzecz "nigdy
  NaN/Infinity w renderowanym SVG", przetestowane jawnie.

- **Staging jak w Task 1-5** — `git add -A` z briefu pominięte (dirty
  `package.json`/`allowScripts` nadal poza zakresem); dwa commity: fold-in
  `fix(ini):` (parser + oba testy + ten wpis i update nagłówka modułu),
  potem `feat(app):` z jawnymi ścieżkami `src/components/curve-editor/*` +
  `TunePanel.tsx` + i18n + reszta tego wpisu.

## Task 7 — 3D surface: lazy three.js + żywy punkt pracy

- **Granica chunka = `React.lazy` w `TableEditor` (zamrożona decyzja 9)** —
  `SurfaceView.tsx` jest JEDYNYM modułem importującym three (statyczny
  `import * as THREE` + `OrbitControls` z `three/examples/jsm/...` są legalne,
  bo sam moduł jest osiągalny wyłącznie przez `import()`); test smoke importuje
  go statycznie świadomie — testy nie są bundlem produkcyjnym. Pomiar bramki
  7.6: entry `index-*.js` **76038 B gz** (budżet < 128000), lazy
  `SurfaceView-*.js` **129497 B gz** (budżet ≤ 184320); `WebGLRenderer`
  występuje 0 razy w entry, 5 razy w chunku lazy. `@types/three` dobrane do
  wersji runtime (`three@0.182.0`).

- **Normalizacja geometrii: jednostkowa stopa 0..1, wysokość względem
  własnego zakresu danych** — `normalize(bins)` mapuje skończone min..max
  binów na 0..1 (footprint siatki nie zależy od fizycznych jednostek osi);
  zdegenerowany zakres (równe min/max lub zero skończonych) → wszędzie 0.5,
  lustrzane wobec `heatT` Taska 4 ("brak gradientu" zamiast dzielenia przez
  zero). Wysokość = `heightScale·(v-min)/(max-min)` liczona względem
  skończonego zakresu WARTOŚCI tabeli (nie zakresu heat!) — powierzchnia
  zawsze wykorzystuje pełną skalę wysokości 0.5 niezależnie od tego, czy
  `zConst.low/high` obejmuje szerszy zakres. Komórka nieskończona (sentinel
  `null` backendu) → wysokość 0 + szary wierzchołek `[0.5,0.5,0.5]` zamiast
  ekstrapolowanego koloru/wysokości. Indeksy: dwa trójkąty CCW na quad,
  wspólna przekątna (`[v00,v10,v01]`,`[v01,v10,v11]`).

- **Dwie funkcje ponad interfejs briefu: `axisFraction` i `heightOf`
  (eksportowane z `surfaceGeometry.ts`)** — szkic pętli rAF briefu woła
  `fraction(...)` i `heightOf(...)` bez definicji; wyniesione jako czyste,
  testowane funkcje zamiast prywatnych domknięć w komponencie, żeby pozycja
  żywego punktu używała DOKŁADNIE tej samej matematyki co wierzchołki siatki
  (jedno źródło prawdy dla "gdzie na osi" i "jak wysoko"), nie drugiej,
  potencjalnie rozjeżdżającej się formuły.

- **Semantyka żywego punktu** — pętla rAF czyta
  `useRealtimeStore.getState().getChannel(...)` imperatywnie (wzorzec M3 z
  `GaugeCanvas`: zero stanu Reacta na klatkę; jedyny mutowany obiekt to
  wbudowany `Vector3` pozycji kropki przez `position.set`). Zakresy osi/wartości
  (`xBins`/`yBins`/`values`) są PRECOMPUTED w `rangesRef` przez efekt montujący
  i efekt zmiany danych, więc sama pętla rAF już ich nie liczy — patrz korekta
  niżej. Punkt wymaga OBU kanałów (`x_channel` i `y_channel` niepuste i oba
  widziane w store); `bilinearHeight` interpoluje WARTOŚĆ komórki (nie
  znormalizowaną wysokość) i zwraca `null` poza zakresem binów lub gdy
  którykolwiek z czterech narożników klatki jest nieskończony → kropka znika
  zamiast ekstrapolować lub pokazywać zmyśloną pozycję (ta sama decyzja co
  kursor krzywej z Taska 6: milcząca nieobecność > myląca pozycja). Wysokość
  kropki = `heightOfIn(zakres, wartość) + 0.03` (lewituje tuż nad
  powierzchnią).

- **Fail-open WebGL** — `new THREE.WebGLRenderer(...)` w try/catch;
  potwierdzone empirycznie, że pod jsdom rzuca synchronicznie ("Error
  creating WebGL context."), więc smoke test przechodzi dokładnie przez tę
  samą ścieżkę co WKWebView bez WebGL. Komponent renderuje wtedy
  `unavailableLabel` — przetłumaczony string przekazywany propem z
  `TableEditor` (wariant przypięty briefem: chunk lazy pozostaje wolny od
  i18n). W teście `getContext` zamockowane na `null` (bez tego jsdom sypie
  szumem "Not implemented" przez virtual console — wzorzec z
  `Gauges.test.tsx`), a `console.error` wyciszone (three loguje błąd przed
  rzuceniem).

- **Utwardzenie WKWebView wg decyzji 9** — `setPixelRatio(min(dpr, 2))`;
  `webglcontextlost` → `preventDefault()` + stop rAF, `webglcontextrestored`
  → restart pętli; pełny dispose przy odmontowaniu (controls, geometria
  siatki + wireframe + kula, trzy materiały, renderer). `setClearColor(0x0,
  0)` — przezroczyste tło, motyw dostarcza CSS (canvas w ramce `.te-3d`).

- **Aktualizacja danych bez przebudowy sceny** — efekt
  `[values, heatLo, heatHi, xBins, yBins]` przepisuje atrybuty
  `position`/`color` w miejscu (`copyArray` + `needsUpdate`; topologia
  siatki nigdy się nie zmienia dla stałej tabeli). Wyjątek ponad brief:
  `WireframeGeometry` to snapshot z konstrukcji — nie śledzi atrybutów
  bazowej geometrii, więc TYLKO wireframe jest odtwarzany przy edycji
  (dispose starego + nowy z zaktualizowanej geometrii); to per-edycja, nie
  per-klatka.

- **Świeże reguły `react-hooks` (v7) wymusiły dwa odstępstwa od szkicu
  briefu** — (1) `propsRef.current = props` podczas renderu łamie
  `react-hooks/refs`; synchronizacja przeniesiona do gołego `useEffect`
  (kanoniczny "latest ref"), pętla rAF podnosi nową wartość w następnej
  klatce. (2) `setUnavailable(true)` w ciele efektu montującego łamie
  `react-hooks/set-state-in-effect`; sonda WebGL może działać wyłącznie
  po montażu (wymaga prawdziwego canvasu) i odpala się co najwyżej raz —
  celowany `eslint-disable-next-line` z uzasadnieniem (precedens:
  `exhaustive-deps` w `CurveEditor`).

- **Konsolidacja `binValues.ts`: przeniesione z `curve-editor/` do
  `table-editor/`** (git mv, razem z testem) — Task 6 świadomie zduplikował
  prywatne helpery `TableEditor` ("osobna, niewielka duplikacja... bo ten
  task go nie dotyka"); Task 7 dotyka `TableEditor` legalnie, a jego
  dodatki (lazy mount + numeryczne biny dla SurfaceView) wypchnęły plik na
  415 linii, ponad budżet 400. Zamiast trzeciego wariantu odczytu binów:
  `TableEditor` używa teraz `arrayOf`/`labelsOf`/`numericOf` z
  przeniesionego modułu (prywatne `binLabels` + świeży `binValues` usunięte;
  wynik: 397 linii), `CurveEditor` tylko zmienia ścieżkę importu. Kierunek
  zależności pozostaje jednostronny: curve-editor → table-editor, nigdy
  odwrotnie. `arrayShape` (rows/cols, potrzebne tylko tabeli) zostaje
  prywatne w `TableEditor`.

- **Zakres heat przekazany 1:1** — `heatLo`/`heatHi` SurfaceView dostaje
  DOKŁADNIE te same wartości co `TableGrid` (low/high stałej z fallbackiem
  na zakres danych, policzone raz w `TableEditor`); zero trzeciego wariantu
  (pułapka z review Taska 6). Kolory wierzchołków przez `heatRgb` Taska 4 —
  ta sama skala hue co heatmapa DOM, jedno źródło prawdy (`hueOf`).

- **CSS: `.te-3d` ma sztywną wysokość (24rem), nie `min-height`** —
  canvas SurfaceView wypełnia kontener przez 100%/100%, a `clientHeight`
  musi się rozwiązać w momencie montażu (mount-once: rozmiar mierzony raz,
  bez ResizeObservera — YAGNI dla stałego layoutu panelu; fallback 640×360
  na wypadek wyścigu z layoutem). `.te-3d-placeholder` z Taska 5 usunięty
  razem z gałęzią placeholdera.

- **Staging jak w Task 1-6** — `git add -A` z briefu zastąpione jawnymi
  ścieżkami; `package.json`/`package-lock.json` stage'owane w stanie
  zawierającym WYŁĄCZNIE zmiany `three`/`@types/three` (dirty hunk
  `allowScripts` odłożony patchem na czas commita i przywrócony po nim,
  procedura kontrolera).

## Task 8 — owner-side realtime capture ring (`ve_analyze` data seam)

- **Miejsce podpięcia (tap) i inwariant tempa** — `CaptureBuffer::push`
  wołane w `poll_tick` (`owner.rs`) NA zdekodowanej, JUŻ WYEMITOWANEJ ramce
  (`if let Ok(Some(frame)) = r { ... }`), PRZED konwersją na
  `RealtimeFrameEvent` (`frame.channels.into_iter()...`), która konsumuje
  `frame` — więc tap musi (i faktycznie) siedzieć przed nią, zgodnie z
  brief 8.2. Ponieważ owner pollinguje z 25 Hz (40 ms, `POLL_INTERVAL`), a
  bramka koalescencji `RealtimePoller` (`crates/realtime/src/poll.rs`)
  emituje maksymalnie co 33 ms (~30 Hz), 25 Hz < 30 Hz ⇒ DZIŚ każda
  zaakceptowana próbka jest emitowana i capture widzi PEŁNE tempo pollingu
  — nic nie jest koalescowane/tracone między pollem a capture'em. To
  udokumentowane wprost w doc-commencie `CaptureBuffer` (rate note z
  briefu, dosłownie) i sprawdzone zachowaniowo osobnym testem ownera
  (`capture_rate_pins_the_tap_invariant`, `owner_tests.rs`): `StartRealtime` +
  `StartCapture`, poll-until-threshold (limit ~2 s, nie sztywny sleep) musi
  dać `sample_count >= 8`, a po `StopCapture` kolejne okno ticków NIE
  powiększa ani `sample_count`, ani `duration_ms` (zamrożenie, nie zanik).
  **Uczciwe zastrzeżenie (recenzja kontrolera):** poll-until-threshold z
  hojnym limitem 2 s potwierdza PRZEPŁYW ramek do bufora i ZAMROŻENIE po
  stopie, ale nie przypina dosłownie stosunku tempa (8 ramek zdąży przyjść w
  2 s nawet przy o połowę wolniejszym capture'ie) — świadomy trade-off na
  rzecz odporności na wolniejsze CI, przyjęty zamiast sztywnego okna
  "~12 ticków, assert ≥8", które faktycznie przypinałoby tempo≈tempo
  pollingu kosztem większego ryzyka flaky. Jeśli M5 kiedyś podniesie tempo
  pollingu powyżej 30 Hz, capture zacznie realnie gubić ramki na bramce
  koalescencji — trzeba będzie wtedy przenieść tap PONIŻEJ niej (`poll.rs`).
- **Wybór pojemności: `CAPTURE_CAPACITY = 27_000`** — dosłownie z briefu
  (~18 min przy 25 Hz; ~1,1 kB/wiersz dla realnego pliku, 139 kanałów × 8 B
  na f64). Brak dodatkowego uzasadnienia poza tym z briefu — nie
  renegocjowane.
- **Przypięcie kolumn — kolejność deklaracji, linear lookup, brak
  `HashMap`** — `StartCapture` buduje `columns` z
  `session.def.output_channels` (kolejność deklaracji w `[OutputChannels]`,
  deterministyczna), tworzy NOWY `CaptureBuffer` (zastępuje stary — restart
  zawsze zaczyna czystą tablicę kolumn, nawet jeśli definicja się nie
  zmieniła). `push` mapuje `columns.iter()` przez liniowe
  `frame.channels.iter().find(...)` — celowo bez `HashMap`, zgodnie z
  briefem ("no HashMap, deterministic"): przy typowej liczbie kanałów
  (rzędu 139 na realnym pliku) koszt O(kolumny × kanały) na klatkę jest
  pomijalny wobec prostoty/determinizmu, a determinizm jest tu wartością
  samą w sobie (spójne z `opentune-analysis`'s "same input → identical
  output" z Task 0).
- **Polityka NaN — fail-open per pozycja** — brakujący kanał w danej ramce
  (np. `[OutputChannels]` się zmieniło albo dekodowanie częściowo zawiodło)
  daje `f64::NAN` w tej jednej komórce wiersza, nigdy błąd ani odrzucenie
  całego wiersza — zgodnie z dyspozycją "fail-open per item everywhere".
  `to_sample_set` te NaN-y przepisuje 1:1 (Task 11 decyduje, jak je
  traktować w analizie — poza zakresem tego taska).
- **`duration_ms` — decyzja poza literą briefu, podjęta świadomie: zamrożony
  czas ostatniego wiersza, nie zegar ścienny od startu.** `CaptureStatusDto`
  ma pole `duration_ms`, ale brief nie precyzuje jego formuły. Rozważona i
  odrzucona alternatywa: `self.start.elapsed()` (żywy zegar) — rośnie przy
  KAŻDYM wywołaniu `status()`, także PO `StopCapture`, co dawałoby mylące
  wrażenie, że capture nadal coś rejestruje, mimo że flaga jest wyłączona i
  żaden nowy wiersz nie powstaje. Przyjęte: `duration_ms` = `t_ms`
  ostatniego zebranego wiersza (0 gdy pusto) — zamraża się dokładnie w
  momencie `StopCapture` (brak nowych wierszy ⇒ brak nowego `t_ms`) i
  odzwierciedla realny zebrany zakres czasu, nie czas zegara. Przypięte
  drugą połową `capture_rate_pins_the_tap_invariant`: `duration_ms` musi
  być identyczne w dwóch kolejnych odczytach `CaptureStatus` rozdzielonych
  200 ms ciszy po `StopCapture`, obok istniejącej asercji `sample_count`.
- **`StopCapture` czyści WYŁĄCZNIE flagę, `Connect`/`Disconnect` czyszczą
  OBA pola** — zgodnie z brief 4: `stop_capture` zeruje `capturing` ale
  zostawia `self.capture` nietknięte (wiersze przeżywają dla
  `run_ve_analyze`, ponowne uruchomienie z innymi parametrami to cecha, nie
  bug). `connect()`/`Command::Disconnect` (owner.rs) czyszczą OBA
  (`self.capture = None; self.capturing = false;`) w tych samych miejscach,
  gdzie już czyszczą `polling`/`poller` — świeża sesja nigdy nie dziedziczy
  capture'u poprzedniej (ta sama reguła M3 co dla pollingu).
- **Prywatny moduł `mod capture;` (nie `pub mod`)** — w przeciwieństwie do
  `dto`/`events`/`connection`/`owner`/`session` (publiczne, bo używane przez
  testy integracyjne/inne moduły spoza drzewa `src/owner*`), `capture` jest
  wewnętrznym szczegółem implementacyjnym ownera, używanym wyłącznie przez
  `owner.rs` — ten sam wzorzec co `session_diff.rs` (prywatny moduł
  top-level, widoczny w całym drzewie crate'a przez potomków crate-roota,
  bez potrzeby `pub`).
- **`#[allow(dead_code)]` na `CaptureBuffer::to_sample_set`, z
  uzasadnieniem w doc-commencie** — metoda jest seamem Task 0/8 dla Task
  11 (`RunVeAnalyze` wciąż zwraca `Err("not implemented (M4)")` — pozostaje
  nietknięty, zgodnie z brief 8, poza jedną poprawką komentarza, patrz
  niżej); dziś wywołują ją wyłącznie testy jednostkowe `capture.rs`
  (`#[cfg(test)]`), więc zwykły `cargo clippy --workspace -- -D warnings`
  (bez `--tests`) widziałby ją jako martwy kod. Rozważona alternatywa —
  uczynienie modułu `pub` (jak `opentune-analysis` re-eksportuje swoje
  stuby Task 0 z crate-roota) — odrzucona: nadawałaby `capture` status
  publicznego API appki bez realnej potrzeby, tylko po to, by ominąć lint.
- **Komentarz `owner.rs` poprawiony, nie tylko rozszerzony** — stary
  komentarz nad stubami ("seams frozen, handlers stubbed until Task 8 /
  Task 11") stałby się nieprawdą po zaimplementowaniu trzech z czterech
  ramion; przeniesiony i zawężony do WYŁĄCZNIE `RunVeAnalyze` (jedyny
  pozostały stub, Task 11).
- **`opentune-analysis` jako zwykła (nie dev-only) zależność
  `src-tauri/Cargo.toml`** — `to_sample_set` zwraca
  `opentune_analysis::SampleSet` z kodu produkcyjnego (`capture.rs`), więc
  zależność musi żyć w `[dependencies]`, nie `[dev-dependencies]` (w
  przeciwieństwie do np. testowych-only crate'ów).
- **Testy ownera — prawdziwy czas, nie `tokio::time::pause`** —
  `src-tauri/Cargo.toml` (dev-deps) nie ma feature'a `test-util`, więc
  wszystkie testy timingowe w `owner_tests.rs` (M3 i ten) działają na
  realnym zegarze; nowy `wait_for_sample_count` (deterministyczne pollowanie
  co 10 ms, limit ~2 s) jest lustrzany wobec istniejącego wzorca
  `await_frame_since`/`await_frame_where` z Taska 6 — unika sztywnego
  `sleep(N ms)`, więc test nie jest z założenia flaky przy wolniejszym CI.
- **Staging jak w Task 1-7** — `git add -A` z briefu pominięte (dirty
  `package.json`/`allowScripts` nadal poza zakresem); dodane tylko jawne
  ścieżki: `src-tauri/src/capture.rs`, `src-tauri/src/analysis_commands.rs`,
  `src-tauri/src/owner.rs`, `src-tauri/src/owner_tests.rs`,
  `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `src/ipc/bindings.ts`
  (zregenerowany), ten wpis.

- **Korekta Taska 7 (review, finding I-1): pętla rAF jednak ALOKOWAŁA co
  klatkę, mimo że powyższy opis "Semantyka żywego punktu" twierdził "zero
  alokacji"** — to zdanie było nieprawdziwe w pierwszej wersji. Faktyczny
  stan sprzed poprawki: `bilinearHeight` budowało literał tablicy
  `[v00,v01,v10,v11].every(Number.isFinite)` przy każdym wywołaniu, a
  `axisFraction`/`heightOf` (wołane z pętli dla obu osi i wysokości kropki)
  za każdym razem na nowo liczyły `finiteRange` — `bins.filter(Number.isFinite)`
  + `Math.min(...)`/`Math.max(...)` na rozproszonej tablicy + świeży obiekt
  `{min,max}` — czyli przy widocznej kropce operacyjnej silnik JS śmiecił przy
  KAŻDEJ klatce (dla `values` to skan O(cells), np. 400 elementów przy tabeli
  20×20 w 60 fps), łamiąc zamrożoną decyzję 9 ("no per-frame allocation —
  reuse one Vector3"). Naprawione BEZ zmiany istniejących sygnatur
  eksportowanych (`bilinearHeight`/`axisFraction`/`heightOf` testowane i
  używane gdzie indziej, więc nietykalne): (1) `bilinearHeight` zamienione na
  cztery inline'owane `!Number.isFinite(...)` zamiast literału tablicy; (2)
  `surfaceGeometry.ts` eksportuje teraz `finiteRange` (typ `FiniteRange`) oraz
  dwa nowe czyste warianty przyjmujące gotowy zakres —
  `axisFractionIn(range, value)` i `heightOfIn(range, value, heightScale)` —
  a `axisFraction`/`heightOf` stały się cienkimi wrapperami nad nimi (DRY, bez
  duplikacji formuły); (3) `SurfaceView.tsx` liczy zakresy `xBins`/`yBins`/
  `values` RAZ — w efekcie montującym i w efekcie zmiany danych (`[xBins,
  yBins, values, heatLo, heatHi]`) — i chowa je w `rangesRef`; pętla rAF
  czyta wyłącznie ten ref (`axisFractionIn`/`heightOfIn`), nie licząc niczego
  od nowa. Stan po poprawce: krok po kroku w pętli rAF nie ma ani jednego
  literału tablicy/obiektu, spreadu ani domknięcia tworzonego per klatka —
  jedyny mutowany obiekt to nadal wbudowany `Vector3` kropki przez
  `position.set`, dokładnie jak zamrożona decyzja 9 zakładała od początku.
  Pokrycie: nowe testy jednostkowe dla `finiteRange`/`axisFractionIn`/
  `heightOfIn` w `surfaceGeometry.test.ts`, w tym asercje parzystości z
  istniejącymi `axisFraction`/`heightOf` dla tych samych zakresów (ten sam
  wynik = zero regresji zachowania); pełny `npm test` i `npm run lint` +
  `npm run format:check` zielone.

## Task 9 — measured AFR + deliberata VE-error surface (grunt prawdy dla Taska 12)

- **Kształt powierzchni `true_ve` — afiniczna, nie logarytmiczna/tabelaryczna,
  celowo.** Brief przypina wprost `40 + 25·(load/100) + 15·(rpm/6000)`,
  clamp `20..110`. Afiniczność jest kluczowa dla testowalności Taska 12: bo
  `true_ve` jest liniowa w `rpm`/`load`, dwuliniowa interpolacja
  (`VeContext::current_ve`) próbkowana w węzłach siatki jest DOKŁADNA (zero
  błędu interpolacji) — jedyny błąd po korekcie komórek to zaokrąglenie do
  najbliższego bajtu (U08, `veTable` scale=1.0) i kwantyzacja kanału `afr`
  (U08, scale=0.1). To świadomie ułatwia przyszłemu auto-tune demo (Task 12)
  udowodnienie zbieżności "w jednym kroku" bez szumu numerycznego
  maskującego, czy algorytm faktycznie zbiega.
- **Wzór AFR (locked decision 11) i dlaczego zamyka pętlę w jeden krok** —
  `afr = afr_target × true_ve / current_ve`. Tabela VE za nisko (chudo
  zaplanowane paliwo) ⇒ measured `afr` POWYŻEJ `afrTarget` (chudo). Korekta
  `VE_new = VE_old × afr/target = VE_old × true/current` upraszcza się do
  `VE_new = VE_old × (true_ve/current_ve) = true_ve` — algebraicznie zbiega
  w jednym kroku, niezależnie od tego, jak bardzo `current_ve` się myli
  (dopóki `current_ve` nie jest przycięte do 1.0 — patrz niżej). To NIE jest
  fizycznie realistyczny model spalania (naprawdę EGO-korekta zbiega
  iteracyjnie z tłumieniem) — to świadomy skrót projektowy dla deterministycznego,
  jednokrokowego demo.
- **`current_ve.max(1.0)` — nie `.max(0.0)` ani brak clampu** — zerowa/prawie
  zerowa strona pamięci (świeżo zaalokowana, nie napisana) dawałaby
  `current_ve ≈ 0`, więc `true/current` eksplodowałoby do nieskończoności/NaN
  przy dzieleniu. Dolny próg 1.0 (nie 0.0) gwarantuje, że iloraz zostaje
  duży, ale skończony i sensowny (`afr` = kilkukrotność `afrTarget`, nie
  `inf`) — "graceful, never divides by ~0", dosłownie z briefu.
- **Determinizm: `ve_ctx` to czysta funkcja stanu silnika + pamięci strony,
  zero nowego RNG, zero zegara ściennego.** `VeContext` jest dekodowany na
  nowo z bajtów strony (`ve_model::ve_context`) PRZED każdym tickiem
  (`ecu.rs`: `Pipe::auto_tick`/`EcuSimulator::tick_engine`), a `snapshot()`
  liczy `afr` z już wyliczonych `self.rpm`/`self.map_kpa` tego ticku —
  żadnego nowego strumienia losowości, żadnego odczytu `Instant::now()`
  wewnątrz silnika. Test pinujący determinizm z M3
  (`same_tick_sequence_is_deterministic`) pozostaje zielony bez zmian —
  potwierdza, że dodanie `ve_ctx` nie naruszyło kontraktu "ta sama sekwencja
  ticków ⇒ identyczne bajty bloku".
- **Konwencja bajtowa `zBins`: row-major, wiersz = oś Y (load), kolumna = oś
  X (rpm) — własna decyzja modułu, nie z gramatyki INI.** Składnia `[RxC]`
  w `[Constants]` nie przypisuje wierszy/kolumn do konkretnej osi fizycznej
  (parser widzi tylko dwie liczby). `ve_model.rs` przyjmuje
  `ve[load_idx * rpm_bins.len() + rpm_idx]` — zgodne z realną konwencją
  Speeduino/TunerStudio (każdy wiersz to jeden bin obciążenia/MAP na
  wszystkich kolumnach RPM) i z `analysis::grid::TableGrid`'s
  `z[y * x_bins.len() + x]` (Task 0, wciąż stub). Test 9.4 pisze komórki
  `true_ve(rpm_bin, load_bin)` pod dokładnie tym samym adresem
  (`load_idx*16 + rpm_idx`), więc spójność jest wewnętrzna i jawna w
  doc-commencie modułu — gdyby ktoś kiedyś podłączył prawdziwy
  `analysis::grid::TableGrid::lookup` (Task 11), konwencje się zgadzają.
- **`ve_context(def, memory)` liczy resolve+decode za każdym tickiem, bez
  cache'owanego `VeBinding` — zmiana względem pierwszego podejścia.**
  Pierwsza wersja (zgodnie z sugestią briefu "prefer smaller retained
  state") trzymała na `Pipe` tylko 3 rozwiązane `ConstantDef` +
  `Endianness` (`VeBinding`), a osobna funkcja `ve_context` (pełny
  resolve+decode z `&Definition`) istniała wyłącznie dla testu 9.3 —
  co czyniło ją martwym kodem pod zwykłym (bez `--tests`)
  `cargo clippy --workspace -- -D warnings` (nikt w kodzie produkcyjnym jej
  nie wołał). Naprawione przez odwrót do prostszego podejścia: `Pipe`
  trzyma sklonowany `Definition` (`Definition: Clone` już wyprowadzone),
  `ve_context(&definition, &memory)` jest wołane wprost co tick — te same
  liniowe skany po `tables`/`constants` (rząd kilkunastu elementów) są
  wystarczająco tanie, by liczyć je od nowa zamiast cache'ować. Efekt:
  jedna funkcja publiczna zamiast trzech (`VeBinding`/`resolve_ve_binding`/
  `decode_ve_context` usunięte), dokładnie sygnatura z brief interface
  block, zero `dead_code`.
- **`och_codec::width` z prywatnej na `pub(crate)`** — jedyna zmiana w
  istniejącym kodzie poza dodaniem pól: `ve_model::decode_array` potrzebuje
  tej samej tabeli szerokości typów co `och_codec::write_scalar` (odwrotny
  kierunek — bajty → fizyczna wartość), więc reużyta zamiast duplikowana
  (DRY w obrębie crate'a — inaczej niż dwuliniowa interpolacja, która
  celowo NIE jest reużyta z `analysis`, bo `analysis` to osobny, jeszcze
  niegotowy crate, patrz wyżej).
- **Test 9.4 tickuje silnik 8000 ms, nie 500 ms jak sugerował brief.**
  500 ms (10 kroków) zostawia silnik w STARTUP przy rpm≈250 — PONIŻEJ
  najniższego binu `rpmBins` (500), więc `current_ve` przycinałby się do
  krawędzi, podczas gdy `true_ve` liczyłoby się z nieprzyciętego,
  faktycznego rpm — pętla nie zbiegałaby się czysto. 8000 ms sprowadza
  silnik do realnego punktu pracy pod obciążeniem (empirycznie
  zweryfikowane debug-printem podczas TDD: rpm=1073, map=52 kPa — NIE jest
  to ściśle tryb Idle, tylko dowolny stan po STARTUP/WARMUP_IDLE), oba w
  granicach binów, więc interpolacja nigdy się nie przycina. **Poprawka po
  recenzji (advisor):** pierwsza wersja tej notatki błędnie opisywała ten
  punkt jako "ustabilizowany Idle (rpm 700-900, MAP 30-40 kPa)" — nieprawda,
  rpm=1073/map=52 leży poza tym zakresem. Właściwe wyjaśnienie, dlaczego
  test i tak jest poprawny: `flat-50` `veTable` daje "lean" tylko gdy
  `true_ve(rpm,map) > 50`, co przy samym Idle (niskie rpm/load) byłoby
  ledwo prawdziwe (margines rzędu pojedynczego kwantu U08); dłuższy tick
  celowo ląduje w stanie WYŻSZEGO obciążenia (nie idle), gdzie
  `true_ve(1073,52)≈55.7` wyraźnie przewyższa 50 — dokładnie zgodnie z
  własnym sformułowaniem briefu "true VE above 50 at running load", nie z
  założeniem "idle". Zweryfikowane wartości z jednego przebiegu: rpm=1073,
  map=52 kPa, afr=16.4 (target 14.7) przed korektą; afr=14.7=target po
  korekcie.
- **RED przed GREEN zweryfikowane sabotażem, nie tylko przez brak
  implementacji.** Test 9.4 napisany i zaimplementowany niemal równolegle
  (formuły przypięte przez brief, więc TDD "napisz test, zobacz FAIL"
  miałby niewielką wartość diagnostyczną na starcie pustego pliku) — więc
  RED zweryfikowany OSOBNO: `engine.set_ve_context(ctx)` w obu miejscach w
  `ecu.rs` tymczasowo zamienione na `set_ve_context(None)`, test
  `sim_measured_afr_reflects_ve_error` faktycznie się wywalił
  (`afr==afr_target`, asercja "must read lean" nieprawdziwa), potem
  przywrócone do stanu GREEN. Dowód, że test naprawdę coś sprawdza, nie
  jest tautologiczny.
- **Zmiany w `speeduino.sample.ini`** — `nPages` 1→4, `pageSize` `8` →
  `8, 288, 288, 16` (strona 1 nietknięta); nowe strony 2 (`veTable`/
  `rpmBins`/`fuelLoadBins`), 3 (`afrTable`/`rpmBinsAFR`/`loadBinsAFR`), 4
  (`warmupBins`/`warmupValues`); `[OutputChannels]` +4 kanały (`map`, `afr`,
  `egoCorrection`, `afrTarget`) + 1 computed (`fuelLoad`) w wolnych bajtach
  8-11 bloku 16-bajtowego; nowe sekcje `[TableEditor]` (2 tabele),
  `[CurveEditor]` (1 krzywa rozgrzewki), `[VeAnalyze]` (1 mapa + 6
  filtrów) — dosłownie z briefu. Nowy test golden-gate dla sample INI:
  `crates/ini/tests/sample_ini.rs` (żaden istniejący test pod
  `crates/ini/tests/` nie parsował `speeduino.sample.ini` — grep czysty
  przed tym taskiem), pinuje `diagnostics.is_empty()` i
  `ve_analyze.is_some()`; istniejące testy pod `src-tauri/src/*.rs`
  (`session.rs`, `dto.rs`, `session_diff.rs`, `owner_ops.rs`) i
  `src-tauri/tests/{tune_demo,connect_flow}.rs` ładujące
  `BUNDLED_INI`/sample INI przeszły bez zmian — w tym
  `dto.rs::bundled_definition_projects_live_gauges_and_frontpage`, który
  osobno pinuje `def.diagnostics.is_empty()`.
- **Staging jak w Task 1-8** — `git add -A` z briefu zastąpione jawnymi
  ścieżkami; dirty `package.json`/`allowScripts` (poza zakresem tego taska)
  pozostawiony nietknięty i niestage'owany.
