# M2 — decyzje i uwagi (notatnik wykonawczy)

Decyzje podjęte w trakcie realizacji M2 tam, gdzie plan zostawiał wybór.
Kryterium: ścieżka optymalna, nieblokująca przyszłego rozwoju.

## Decyzje

- **Mapowanie pól dialogów (Task 3):** `slider`/`displayOnlyField` →
  `FieldKind::Constant` (wiernie — to afordancje nad tą samą związaną stałą);
  `commandButton`/`settingSelector` → `Diagnostic` + pominięcie (brak wiernej
  reprezentacji w zamrożonym typie); nieznane słowa kluczowe → `Diagnostic`.
  `std_separator` pomijany cicho (separator to nie błąd parsowania).
- **Ewaluator wyrażeń (Task 2): `&&`/`||` liczone gorliwie, nie
  short-circuit** — parser jednoprzebiegowy; konsumenci degradują błędy do
  diagnostyk. Ograniczenie: warunek odwołujący się do zmiennej za bramką
  preprocesora za `&&` da diagnostykę tam, gdzie TS by go odciął.
- **Wykonanie na gałęzi `m2-read-edit-burn` w głównym checkout** (bez osobnego
  worktree): istniejący `target/` i `node_modules` skracają cykl build/test;
  brak równoległych implementerów ⇒ brak konfliktów.
- **Zadania sekwencyjnie, nie równolegle** — mimo że plan dopuszcza równoległość
  po Task 0. Wspólny `Cargo.lock`/workspace i tani koszt sekwencji < ryzyko
  konfliktów między podagentami.
- **Task 0.4:** test kontraktowy wymaga działających `Tune::new` /
  `is_dirty` / `page_bytes` — implementowane minimalnie (zerowane strony),
  reszta metod `todo!()`, zgodnie z literą "compiles + passes against stub `new`".

- **Warunki `visible`/`enable` (Task 7): ewaluacja po stronie backendu**
  (`eval_conditions`) zamiast portu gramatyki do TS — jedno źródło prawdy
  (ewaluator z Task 2), fail-open przy niesparsowalnym wyrażeniu (pole
  widoczne zamiast cicho ukryte).
- **Undo/redo sięgają drutu (Task 7):** cofnięcie zapisuje odwrócone bajty
  do ECU; przy błędzie zapisu stosowana jest operacja odwrotna, żeby Tune
  nigdy nie rozjechał się z ECU. `set_value`: walidacja na klonie → zapis
  protokołem → commit do Tune dopiero po Ok.
- **`DefinitionDto` przez IPC** zamiast surowej `Definition` — specta 0.0.12
  zabrania `usize`; DTO z typami JS-safe to zarazem poprawna granica API
  (frontend nie potrzebuje offsetów bajtowych).
- **Zapis na żywo przez serial odłożony do M3** — `ConnectionManager` nie
  trzyma trwałego uchwytu `Protocol`; M2 działa wyłącznie na symulatorze,
  operacje stron na serialu zwracają jasny błąd.
- **`diff` (Task 8): pomijanie stałej, gdy `Tune::get` zwraca błąd po
  którejkolwiek stronie** — pod udokumentowanym warunkiem wstępnym (oba tune
  zbudowane z tej samej `Definition`) błąd nierozwiązywalnego wyrażenia
  występuje identycznie po obu stronach, więc nie ma sensownego "przed/po"
  do pokazania; błąd tylko po jednej stronie nie powinien wystąpić przy
  spełnionym warunku wstępnym, ale i tak degraduje bezpiecznie (pomiń,
  nigdy panic). `cells` w `FieldDiff` niesie wyłącznie różniące się indeksy
  (nie pełny zrzut tablicy) — `a`/`b` na poziomie pola i tak niosą pełne
  tablice.
- **`merge` (model) fail-open per-pick** — zamrożona sygnatura `merge()` nie
  zwraca nic, więc odrzucony pick (nieznana stała, błąd `Tune::set`) jest
  cicho pomijany; wywołujący, któremu zależy na wyniku, może wywołać `diff`
  ponownie. Sesyjny `Session::merge_tune` (Task 8) **nie** używa
  `opentune_model::merge` wprost — stosuje wariant per-pick
  walidacja-na-klonie → zapis do ECU → commit (ten sam wzorzec co
  `set_value`), bo pojedyncze wywołanie `merge` mogłoby zapisać kilka stron
  jedną paczką i przy błędzie zapisu w środku rozjechać Tune z ECU; przy
  błędzie zapisu pojedynczego picka przerywa całą operację, zostawiając
  wcześniej zatwierdzone picki nietknięte (Tune == ECU dla nich).
- **`snapshot_tune` (Task 8): baseline tylko w pamięci** — `Session.snapshot:
  Option<Tune>` to klon bieżącego tune w danym momencie; brak importu z
  pliku `.msq` (M6). Diff/merge działają wyłącznie względem tego zapisanego
  stanu, nie względem dowolnego pliku.

## Uwagi do dyskusji (nieblokujące)

- **Aliasowane tabele w prawdziwym speeduino.ini** (Task 1): `lambdaTable`/
  `afrTable` współdzielą bajty na stronie 5, co wyzwala nakazany planem twardy
  błąd przekroczenia rozmiaru strony. Trimowane fixtury M2 tego nie dotykają,
  ale pełna ingestia prawdziwego INI (cel: zamiennik TunerStudio) wymaga
  dopuszczenia aliasowania (jawny offset wstecz ≠ błąd; licznik `lastOffset`
  nie powinien podwójnie liczyć aliasu). Do rozwiązania najpóźniej przy
  pełnym wczytaniu speeduino.ini (M2 Task 7 lub follow-up).
- **`groupMenu`/`groupChildMenu` nieobsłużone** (Task 3): zamrożony
  `MenuDef`/`MenuItem` nie ma miejsca na poziom grupowania. Realne
  speeduino.ini używa tego (np. "Engine Protection"). Wymaga decyzji przy
  przyszłej zmianie kształtu `Definition` (M3+): dodatkowy poziom drzewa albo
  jawne spłaszczenie z diagnostyką.
- **Protokół (Task 5) — luki świadomie odłożone do pracy z realnym sprzętem:**
  (a) brak chunkowania przy `blockingFactor` — pojedynczy transfer na całą
  stronę działa z symulatorem, realne strony Speeduino (>121/251 B) wymagają
  multi-block; (b) bajt zwrotny write/burn jest weryfikowany CRC, ale nie
  dekodowany semantycznie (sukces burn = 0x04, nie 0x00; odrzucenie zakresu
  przez firmware wygląda jak Ok) — `Ok(())` znaczy "wysłano + poprawny CRC
  ack", nie "potwierdzono zastosowanie"; (c) `$tsCanId` na sztywno 0.
- **Prawdziwy speeduino.ini rozprasza klucze comms do `[Constants]`**
  (per-page listy `pageReadCommand` itd.) — M1 `parse_comms` czyta tylko
  `[MegaTune]`/`[TunerStudio]`; pełna ingestia realnego pliku wymaga
  rozszerzenia parsera comms (razem z aliasowaniem tabel powyżej).
