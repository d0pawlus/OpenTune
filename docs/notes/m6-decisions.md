# M6 — Interop, polish & first release: notatnik decyzji

> Kickoff 2026-07-12. Stan wejściowy: M0–M5 ✅ na `main` (ostatni merge: PR #12,
> `89b429d`). Brak otwartych PR-ów. Pierwszy zewnętrzny feedback: issue #13.

## Dekompozycja M6 na wycinki (każdy = osobny plan, osobne PR)

| # | Wycinek | Stan | Blokery |
| --- | --- | --- | --- |
| 1 | **Artefakty release'owe (unsigned)** — CI buduje draft pre-release na tag `v*` | plan gotowy: [2026-07-12-m6-release-artifacts](../superpowers/plans/2026-07-12-m6-release-artifacts.md) | — |
| 2 | Naprawy przed-publikacyjne: **3D surface nie renderuje** (M4 smoke OPEN #1) + keymap (OPEN #2) | nietriażowane — najpierw systematic-debugging / repro | autoryzacja użytkownika (jawnie odłożone po smoke M4); scope keymap niedoprecyzowany |
| 3 | `.msq` import/export zweryfikowany z TunerStudio | do researchu (golden files z realnego TS) | dostęp do TunerStudio do wygenerowania wzorców |
| 4 | Walidacja na wielu firmware'ach (Speeduino ✅ sample, rusEFI, MS-family; **+ Honda OBD1 / MS4x** — oferta pomocy z issue #13) | do researchu | realne INI; ew. współpraca z autorem issue #13 |
| 5 | Podpisy: macOS notarization, Windows signing | — | **konto Apple Developer + cert Windows — decyzja/zakup użytkownika** |
| 6 | Auto-update (`tauri-plugin-updater`) + onboarding/first-run | — | wymaga wycinka 5 (klucze podpisu updatera) |
| 7 | A11y + i18n (PL/EN) pass | — | decyzja o zakresie |
| 8 | Strona dokumentacji | — | wybór stacku (decyzja użytkownika) |

Kolejność rekomendowana: 1 → 2 → (3 ∥ 4) → 5 → 6 → 7 → 8. Wycinek 1 nie ma
blokerów i odblokowuje testerów (issue #13 wprost prosi o buildy).

## Issue #13 — pierwszy feedback społeczności (Ferenc / HondaRulez)

Zgłoszone 2026-07-12. Autor: tuner z doświadczeniem Honda OBD1 + BMW MS4x,
robi hardware/firmware na tych platformach, oferuje kontrybucję wsparcia.

Postulaty UX (wszystkie z perspektywy „tuning w aucie, w słońcu, na wyboistej
drodze"):

1. **Touch-first UI** — tuning w ciasnym aucie = obsługa palcem, nie myszą.
2. **Lock screen** — blokada dotyku (deszcz/muchy generują fałszywe tapnięcia);
   odblokowanie długim przyciśnięciem ikony kłódki.
3. **Konfigurowalny „paged grid" dashboard** — użytkownik sam składa strony
   z równomiernej siatki gauge'ów/przycisków; item może span-ować wiersze/kolumny;
   wszystko skalowane do maksimum dostępnego miejsca (duże fonty, zero menu
   podczas jazdy); strony = przełączanie między widokiem prostym a szczegółowym.
4. Prośba (komentarz): **screenshoty / publiczne buildy** — nie może skompilować
   projektu (starsza dystrybucja, zależności). → wycinek 1 (AppImage z bazą
   ubuntu-22.04 = starsze glibc, adresuje to wprost).

Mapowanie: (1)–(3) to kandydaci na backlog UX po 1.0 lub do wycinka 7 —
**decyzja użytkownika**; istniejący dashboard M3 ma już persystowany layout,
więc „paged grid" to ewolucja, nie rewolucja. (4) załatwia wycinek 1. Oferta
platformowa (Honda OBD1/MS4x) zasila wycinek 4.

### Szkic odpowiedzi na issue #13 (do wysłania przez właściciela repo)

> Hi Ferenc, thanks — this is exactly the kind of field feedback we need.
>
> **Builds:** you're in luck — unsigned pre-release builds (including a Linux
> AppImage built on an older glibc baseline, so it should run on your distro)
> are the very next thing on the roadmap. I'll post here when the first one
> is up on the Releases page.
>
> **Touch/lock/grid ideas:** all three resonate — the current dashboard
> already has a persisted, editable gauge layout, and an evenly-sized paged
> grid with spannable cells is a natural evolution of it. The
> lock-with-long-press idea is noted (rain and flies are real). I've logged
> these against the UX backlog; I'd keep them separate from this issue so
> each can be tracked — feel free to open one issue per idea if you like.
>
> **Honda OBD1 / MS4x support:** genuinely interested, especially since you
> do hardware/firmware work on these. The whole core is INI-driven, so the
> first practical step would be: can you share the INI definitions (and
> ideally a short serial capture) for the platforms you have? That will tell
> us how much of the protocol layer generalizes.

## Decyzje otwarte (czekają na użytkownika)

- **D-1:** Autoryzacja naprawy 3D surface + doprecyzowanie zakresu keymap
  (M4 smoke OPEN #1/#2) — rekomendacja: naprawić surface **przed publikacją**
  pierwszego draftu (pierwsze wrażenie testera), keymap może poczekać.
- **D-2:** Tag `v0.1.0` — pchnięcie taga uruchamia build; release i tak
  powstaje jako **draft** (nic nie jest publiczne bez ręcznej publikacji).
- **D-3:** Konto Apple Developer (99 USD/rok) + cert Windows — potrzebne
  dopiero od wycinka 5; bez nich 1.0 „signed" z ROADMAP nie istnieje.
- **D-4:** Odpowiedź na issue #13 — szkic powyżej; wysyłka = właściciel repo.
- **D-5:** Postulaty UX z issue #13: backlog po-1.0 czy część M6-7?
