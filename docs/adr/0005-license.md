# 0005 — Open-source license (GPL-3.0-or-later)

- **Status:** Accepted
- **Date:** 2026-06-21

## Context

OpenTune is explicitly an **open-source** project meant to be owned and evolved by
the community, replacing a closed-source incumbent. We must pick a license. Key
forces:

- **Ecosystem norms.** The open ECU firmware this project serves — **Speeduino**
  and **rusEFI** — is licensed under the **GPL**. Aligning with that copyleft ethos
  is natural and keeps derived improvements open.
- **Keeping it open.** A core motivation is that the existing tool is *not* open; a
  copyleft license guards against a closed-source fork capturing the work.
- **Adoption & contribution.** A permissive license can lower barriers to broad
  use/embedding, at the cost of allowing closed derivatives.

No license file is committed yet; this ADR records the recommendation and the
trade-offs so the owner can decide deliberately.

## Decision

**Recommended: GPL-3.0-or-later.** It matches the surrounding open ECU ecosystem,
keeps the application and its derivatives open, and reflects the project's reason
for existing. *Final choice rests with the project owner.*

## Consequences

**If GPL-3.0 (recommended)**

- Derivatives must remain open under GPL — protects the commons.
- Strong fit with Speeduino/rusEFI community expectations.
- Some companies avoid GPL for proprietary integration; that's an accepted
  trade-off here.

**If a permissive license instead (MIT / Apache-2.0)**

- Maximizes adoption and allows embedding in closed products.
- Apache-2.0 adds an explicit patent grant (nice for a hardware-adjacent tool).
- Risk: a closed-source fork could diverge from the community version.

## Alternatives considered

- **GPL-3.0-or-later** — *recommended*; copyleft, ecosystem-aligned.
- **MPL-2.0** — file-level copyleft; a middle ground allowing easier mixing with
  other licenses while keeping touched files open. Reasonable compromise.
- **Apache-2.0** — permissive with patent grant; best for maximum adoption.
- **MIT** — simplest permissive option; least protection for the commons.

## Action required

- [x] Project owner selects a license — **GPL-3.0-or-later**.
- [x] Add the corresponding `LICENSE` file at the repo root (full GPL-3.0 text).
- [x] Update `README.md`.
- [ ] Add `SPDX-License-Identifier: GPL-3.0-or-later` headers in source files once
      code lands (M0).
