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
- **Odstępstwo od ARCHITECTURE §9 (owner task + kanał poleceń) — świadome,
  do rewizji w M3:** §9 nakazuje async runtime (Tokio) z jednym
  owner-taskiem trzymającym transport i serializacją dostępu przez kanał
  poleceń. M2 implementuje ten sam niezmiennik ("jedna konwersacja naraz",
  bo serial jest z natury single-conversation) synchronicznym
  `Mutex<Option<Session>>` i synchronicznymi komendami Tauri
  (`std::thread::sleep` na `interWriteDelay`/`pageActivationDelay` wewnątrz
  blokady) — bez Tokio i bez kanału. Niezmiennik jest zachowany (każdy
  dotyk drutu przechodzi przez jedną blokadę `Session`), a odstępstwo jest
  celowe: skala M2 (jedna sesja, brak strumieniowania w czasie
  rzeczywistym) nie uzasadnia kosztu kanału. M3 (strumieniowanie realtime,
  ARCHITECTURE §5.5/§9) wymusi przebudowę na model owner-task + kanał
  poleceń — wtedy do zrewidowania, nie wcześniej.

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

## Remediacja z review M2/M3 (2026-07-11)

Poprawki z przeglądu M2/M3 dotykające kodu M2 (gałąź `review/m2-m3`).

- **Dirty = różnica bajtów względem bazy flash (zmiana zachowania).**
  `Tune` śledził dirty jako „strony dotknięte od ostatniego burna" (lepka
  flaga), przez co `load → edit → undo` zostawiał stronę na stałe brudną,
  mimo że bajty wróciły do wartości wczytanej. Zamiast tego trzymamy
  `baseline: Vec<Vec<u8>>` (kopia bajtów flash, ustawiana przy `load_page`
  i `mark_burned`), a `is_dirty`/`dirty_pages` **wyprowadzamy** przez
  porównanie bajtów — nie da się rozjechać ze stanem. Skutek uboczny:
  burn pomija stronę zedytowaną-i-cofniętą (bardziej poprawne). Bezpieczne,
  bo `is_dirty == false` przy RAM≠flash nie może wystąpić: baza jest
  ustawiana wyłącznie przy odczycie (założenie RAM==flash) i burnie
  (RAM→flash) — te same założenia co dotąd. Test regresji:
  `undo_back_to_loaded_baseline_clears_dirty` (tune_state.rs); odwrócono
  jawnie lepki `undo_and_redo_reach_the_wire` (session.rs). **Sufit
  (ponytail):** `load_page` resetuje bazę tylko dlatego, że wołany jest
  wyłącznie z pełnego odczytu świeżego `Tune`; częściowy refresh RAM
  wymagałby jawnego `is_baseline: bool`.
- **Merge komórkowy (`MergePick`).** Diff/merge dostał wybór per-komórka:
  `MergePick::{All, Cells}` + `merge_picks` w modelu; IPC `MergePickDto`
  (`{type:"all"|"cells"}`); `mergeTune(picks)` w owner-tasku woła
  `Session::merge_picks`. Zachowany kontrakt M2: **każdy pick to osobny
  commit na drut** (merge może przerwać się w połowie po zapisaniu
  wcześniejszych picków) — nie batch'ujemy delt wielostronicowych.
- **Utwardzenie parsera INI (nieblokujące, ale realne paniki).** Odrzucanie
  niezadeklarowanych stron; sprawdzana arytmetyka offsetów/rozmiarów zamiast
  `usize *` bez kontroli; pozycyjne `enable`/`visible` w parserze dialogów
  poprawione (3. token = enable, 4. = visible wg gramatyki TunerStudio);
  puste pola liczbowe → wartości domyślne zamiast `Number::Expr("")`.
  `protocol/pages.rs`: kontrolowane błędy zamiast obcinania konwersji
  rozmiar/id-strony.
- **Odzysk po panice/reconnekcie w owner-tasku.** Panika w operacji sesji
  czyści sesję, rozbraja realtime i emituje `Disconnected` (koniec fałszywego
  „Connected"); po reconnekcie z rebootem snapshot jest czyszczony przed
  ponownym odczytem tune, a nieudany re-read zachowuje link, ale unieważnia
  tune+snapshot.
