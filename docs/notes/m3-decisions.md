# M3 — decyzje i uwagi (notatnik wykonawczy)

Decyzje podjęte w trakcie realizacji M3 (dashboard czasu rzeczywistego) tam,
gdzie plan zostawiał wybór lub rzeczywistość go korygowała. Kryterium jak w M2:
ścieżka optymalna, nieblokująca przyszłego rozwoju. Plan:
`docs/superpowers/plans/2026-07-02-m3-realtime-dashboard.md`; badania:
`docs/notes/m3-research.md`.

## Decyzje

- **`GaugeDef.units: String`, nie `Number` (poprawka w punkcie zamrożenia,
  Task 0):** blok interfejsów planu deklarował `units` jako `Number`, co było
  semantycznie błędne — jednostki to etykiety ("RPM", "%"), a `Number::Expr`
  to ścieżka `eval()`. Poprawione w kodzie, testach i planie (łącznie z
  krokami parsera Task 3 i `GaugeDto` Task 7) zanim cokolwiek zdążyło na tym
  typie zbudować.
- **Odstępstwo §9 z M2 ZAMKNIĘTE (Task 1):** owner-task Tokio + kanał poleceń
  **opakowuje** synchroniczny `Session` (owner *nad* Session, nie przepisanie
  Session na async) — wszystkie testy M2 zielone bez zmian to dowód, że
  migracja nie zmieniła zachowania. Sesja wędruje przez `spawn_blocking` przez
  `Option::take`; każde ramię obsługi wysyła dokładnie jedną odpowiedź; panika
  ownera degraduje do "not connected".
- **Reconnect po serialu KOLEJKUJE komendy za ownerem** zamiast fail-fast
  (intencja §9); do rewizji przy UX realnego serialu.
- **Reboot wykryty przy reconnect ⇒ ponowny odczyt tune** (unieważnienie
  follow-upu M2); glitch (secl idzie do przodu) zachowuje niewypalone edycje.
  Obie ścieżki przetestowane, strażnik glitcha dowiedziony mutacją.
- **Zasięg przesunięcia bitowego (Task 3):** licznik `<<` poza 0..=63 →
  `ExprError::Math` zamiast niestrzeżonego rzutowania z briefu (ryzyko paniki
  w debug). Gramatyka: `or → and → bitand(&) → compare → shift(<<) → additive`.
- **Tolerancja krótkiej ramki tylko na ścieżce CRC (Task 4):** realny rozjazd
  INI/firmware (`ochBlockSize` 139 vs `LOG_ENTRY_SIZE` 138) ⇒ nigdy nie ufamy
  deklarowanej długości; ścieżka CRC zwraca skrócony, CRC-zweryfikowany
  `Vec`. Plain krótka odpowiedź = szybki timeout (pułapka rozjazdu dotyczy
  wyłącznie koperty).
- **Symulator: animacja PORT z `askrejans/speeduino-serial-sim` (MIT), enkodowanie
  FRESH (Task 5):** maszyna stanów i korelacje fizyki przeniesione 1:1
  (poszerzenie `i16`→`i32` jako udokumentowane bezpieczniejsze odstępstwo);
  enkodowanie do bloku och sterowane offsetami z INI (`och_codec.rs`), nie
  sztywną 130-bajtową strukturą referencji. **Lekcja licencyjna:** sam cytat
  "MIT" nie wystarcza — MIT wymaga zachowania linii copyright przy
  redystrybucji (tu pod GPL-3); dopisane
  `Copyright (c) 2026 Arvis Skrējāns` do obu nagłówków port-note.
- **Zerowanie secl przy pierwszym 'r' (Task 5):** wierny port zachowania
  firmware (comms.cpp) — symulator zeruje licznik przy pierwszym żądaniu och;
  `reboot()` ponownie uzbraja. To obnażyło utajone ryzyko M1 (niżej).
- **`opentune-realtime` pozostaje decode-only (Task 6):** jedyna zależność to
  `opentune-ini`; poller bierze odczyt bloku jako domknięcie zamiast uchwytu
  `Protocol`. ~30 linii czytników skalarnych świadomie zduplikowane z
  `opentune-model` zamiast zależności lub trzeciego crate'a na 30 trywialnych
  linii.
- **Fail-open per kanał / per wartość (Task 6):** zły expr lub krótki bufor →
  kanał ląduje w `diagnostics`, ramka nigdy nie jest pusta. `get_values`:
  nierozwiązywalna stała → sentinel `f64::NAN` (serde_json ⇒ `null`; frontend
  renderuje "—", Task 7.6). Świadome poszerzenie: nieznana *nazwa* też
  degraduje do sentinela — literówka z frontendu pokaże "—" zamiast błędu.
- **Utrata danych przez fałszywy reboot — ZAMKNIĘTE dwuczęściowo (Task 6):**
  reguła M1 `new_secl < last_secl` bez strażnika + zerowanie secl przy
  pierwszym 'r' = po starcie pollingu zwykły glitch wyglądałby jak reboot ⇒
  cichy ponowny odczyt tune ⇒ **utrata stanu dirty** (wartość przeżywa glitch
  w RAM; ginie fałszywe "czysto", które maskuje niewypalone edycje przed
  późniejszym realnym rebootem). Poprawka: (1) `ConnectionManager::note_secl` —
  każdy udany poll karmi bazę bajtem 0 bloku; (2) `read_secl` świadomy
  szablonu okienkowego — bo samo (1) na realnym INI albo timeoutuje (Plain
  czeka na 6 brakujących bajtów żądania), albo czyta status `0x00` jako
  fałszywy secl i **powoduje** dokładnie tę utratę, którą miało zamykać.
  Ścieżka jednobajtowa (`"A"`) zachowana bajt-w-bajt (test przypinający).
  Test e2e zweryfikowany kontrfaktycznie (wyłączenie note_secl = czerwony).
- **Nowa własność systemu (zaakceptowana):** na INI z szablonem okienkowym
  odczyt secl przy connect *sam jest* pierwszym żądaniem och i konsumuje
  zerowanie — baza startuje od 0, więc detekcja rebootu wymaga wcześniejszego
  polla, by podnieść bazę. Zgodne z ruchem realnego TunerStudio.
- **Poll 25 Hz / emisja ≤30 Hz; jawny start/stop:** `start_realtime`/
  `stop_realtime` bez auto-startu; Connect i Disconnect czyszczą polling
  (świeża sesja nigdy nie dziedziczy). Komendy zawsze wywłaszczają poll
  (`tokio::select!` z `biased`). Nieudane polle połykane per tick (fail-open)
  — zatrzymanie to jawna komenda użytkownika, nie decyzja pętli.
- **`loop.rs` → `poll.rs`:** `loop` to słowo kluczowe Rusta; brief nazwał plik
  nie do użycia.

- **Zegary canvas ręcznie, zero nowych zależności (Task 7):** rdzeń to pętla
  `requestAnimationFrame` czytająca `useRealtimeStore.getState()` — 30 Hz
  nigdy nie wchodzi w reconciliation Reacta (żadnego stanu React per ramka,
  żadnych subskrypcji selektorów na gorącej ścieżce); przemalowanie tylko
  przy zmianie wartości, pierwsze malowanie gwarantowane.
- **Field: wzorzec draft-null zamiast `useEffect`-reset z briefu (Task 7):**
  wymuszony przez regułę eslint `react-hooks/set-state-in-effect`, ale
  recenzent uznał go za *lepszy* — w trakcie edycji draft wygrywa, więc
  zmiana propa nie nadpisuje pisanego tekstu; poza edycją wartość backendu
  prześwituje naturalnie. `null` Scalar (sentinel NaN z Task 6) renderuje
  "—", nigdy 0.
- **Styl zegara per slot (round/bar/digital) w JSON layoutu (Task 7):** brief
  nakazywał trzy komponenty, ale nie mówił, gdzie Bar/Digital są osiągalne;
  selektor stylu w GaugeBinderze czyni każdy deliverable osiągalnym.
  Backend layoutu = nieprzezroczysty blob (walidacja po stronie frontendu
  przy wczytaniu; `parseLayout` nigdy nie ufa zawartości pliku).
- **Reaktywność motywu przez `useLayoutEffect` w App (poprawka po review,
  Task 7):** samo przewleczenie propa `theme` łapało motyw *wychodzący* —
  React opróżnia efekty pasywne dziecko-przed-rodzicem, więc canvas czytał
  `data-theme` zanim App go zapisał. Awans setter-a atrybutu w App do
  `useLayoutEffect` (efekty layoutu flushują przed wszystkimi pasywnymi);
  canvas zostaje pasywny. Odrzucona alternatywa: mapowanie propa wprost na
  kolory w JS — dublowałoby paletę oklch i łamało tokens.css jako jedyne
  źródło prawdy o kolorze. Asymetria jest nośna i skomentowana w App.tsx.

## Uwagi do dyskusji (nieblokujące)

- **Bundlowany `speeduino.sample.ini` nie ma `[OutputChannels]` /
  `[GaugeConfigurations]` / `[FrontPage]`** — domyślny connect do symulatora
  nie wyemituje żadnej ramki (`start_realtime` zwraca Ok, polle cicho
  zawodzą 25 Hz). Do rozszerzenia w Task 8 (gotowy wzorzec:
  `src-tauri/tests/fixtures/realtime-owner.ini`).
- **Serial nadal nie może pollować** — uchwyty protokołu per-operacja z M2
  zwracają `SERIAL_UNSUPPORTED`; realtime na sprzęcie czeka na "trwały
  `MsProtocol` w `ConnectionManager`" (follow-up M3-serial).
- **Brak backoffu nieudanych polli** — na martwym łączu / INI bez och pętla
  próbuje 25 Hz w nieskończoność; tanie, ale oczywisty follow-up to mały
  backoff błędów w `poll_tick`.
- **`ochGetCommand` z `[OutputChannels]` nadpisuje `[MegaTune]`** w
  `parse_definition` (parse_comms nietknięty — kontrakt M1); realne
  speeduino.ini trzyma gołe `"r"` w `[MegaTune]`, a szablon okienkowy w
  `[OutputChannels]`. Escape `\$` w `expand_template` naprawiony (emitował
  zabłąkany bajt 0x5C ⇒ 8-bajtowe żądanie).
- **Lampka wskaźnikowa: wyrażenia inne niż goła nazwa bitu cicho gasną**
  (fail-open do OFF) — dla lampki ostrzegawczej w realnym aucie to groźny
  kierunek degradacji; przyszła poprawka to stan "nieznany"/tooltip albo
  ewaluacja wskaźników po stronie backendu. Para z tym: `applyFrame` pomija
  `null` (trzyma ostatnią dobrą wartość) bez sygnalizacji nieświeżości —
  jedna wspólna poprawka "age-out + diagnostyka staleness" w M4+.
- **`useLayoutEffect` w App strzeżony komentarzem, nie testem** — harness
  testowy dubluje okablowanie App, więc "uproszczenie" hooka z powrotem do
  `useEffect` przywróciłoby błąd bez czerwonego testu; test na poziomie App
  wymagałby mockowania IPC (decyzja odłożona).
- **Finalny przegląd gałęzi (fable) — wnioski:** panel Dashboard odmontowywał
  się na `Reconnecting`, choć backend celowo utrzymuje polling przez glitch —
  zegary animowały się pod przyciskiem "Start live" (naprawione wspólnym
  predykatem `isLinkAlive`; pierwsza poprawka była martwa w złożonej
  aplikacji, bo TunePanel zerował współdzieloną `definition` — domknięte
  testem integracyjnym montującym oba panele nad realnymi store'ami).
  **Ryzyko I-2 (bilet, doprecyzowanie wpisu wyżej):** na okienkowym INI
  reboot jest niewidoczny przez CAŁĄ nie-pollującą część sesji (użytkownik,
  który nie wciska "Start live"), nie tylko "do pierwszego polla" — realny
  TS polluje stale, OpenTune ma jawny start. Follow-up: reconnect z bazą 0
  traktować jako możliwy reboot (tania weryfikacja odczytem strony) albo
  niski keepalive secl. **NaN może wejść do kanałów** (F32 ze śmieciowych
  bajtów, inf−inf w wyrażeniu) — kontrakt domknięty: serde→`null`, store
  pomija + strażnik `isFinite`. **Field/diff/merge NIE są bramkowane
  podczas reconnectu** — owner kolejkuje ich komendy za reconnectem
  (bezpieczne, opóźnione); bramkowanie jak burn/undo/redo to follow-up.
  Reboot w trakcie glitcha nie odświeża już wartości tune automatycznie
  (cena "przeżycia glitcha" — notatka, nie defekt).
- **Odłożone drobiazgi do finalnego review gałęzi:** nieaktualny doc-comment
  `events.rs` ("Not yet registered" — już zarejestrowane, propaguje do
  bindings.ts); brak bezpośredniego testu przewymiarowanego `%2c` (bezpieczne
  konstrukcyjnie); podwójny `strip_inline_comment` w gauges_parser (no-op);
  bajt statusu zdejmowany bez sprawdzenia wartości (wzorzec w całym crate);
  `constants_fields.rs` 504 linie / `pages.rs` równe 400 linii (dzielić przy
  następnym dodatku, nie przycinać doków).

## Remediacja z review M2/M3 (2026-07-11)

Poprawki z przeglądu M2/M3 dotykające kodu M3 (gałąź `review/m2-m3`).
Warstwa M2 (dirty/flash, merge komórkowy, utwardzenie INI, odzysk owner-taska)
opisana w `m2-decisions.md`.

- **Zakresy zegarów rozwiązywane w backendzie, bez mylącego 0–100.** Zegary
  o granicach wyrażeniowych (`Number::Expr`) waliły w twardy fallback 0–100 na
  froncie. Nowa komenda `resolveGaugeBounds` → `Session::resolve_gauge_bounds`
  używa `Tune::resolve_number` (ten sam ewaluator co reszta), zwraca `None` dla
  nieobsługiwanej/nieznanej granicy niezależnie na pole (fail-open per-pole).
  Front (`useResolvedGauge`, Round/Bar/Digital) renderuje geometrię zależną od
  zakresu neutralnie, gdy zakres nieznany — zamiast wymyślać skalę 0–100.
  `TunePanel` woła `resolveGaugeBounds` przy każdym refreshu i cache'uje w
  store; ramki realtime nadal omijają React.
- **Błąd częściowego merge'a nie jest kasowany przez auto-rediff.** `TuneDiff`
  po merge'u woła `onAfterMerge` i re-diff z zachowaniem błędu (`loadDiff` nie
  czyści błędu na re-diffie), więc komunikat o przerwaniu w połowie zapisu
  przeżywa odświeżenie tabeli. Izolacja mocków w teście
  (`beforeEach(vi.clearAllMocks)`) — bez tego liczniki wywołań `diffTune`
  kumulowały się między testami.
- **Dashboard: strażniki async + stany ładowania.** `Dashboard` dostał
  `realtimePending`/`savePending` (brak podwójnych startów), stan ładowania
  layoutu i obsługę błędów; `GaugeCanvas` drobne utwardzenie rysowania.
