---
layout: home
title: OpenTune documentation
permalink: /
---

OpenTune is open-source, cross-platform engine tuning software built around the
firmware definitions and tune formats already used by TunerStudio-compatible
ECUs.

## Start here

- [Quick start]({{ '/quick-start/' | relative_url }}) — simulator, offline
  projects, and cautious real-hardware setup.
- [Install and update]({{ '/updates/' | relative_url }}) — unsigned platform
  warnings, signed updater behavior, checksums, and recovery.
- [M6 compatibility evidence]({{ '/compatibility/m6/' | relative_url }}) —
  Speeduino, rusEFI, MegaSquirt, and exact reproduction commands.
- [M6 accessibility evidence]({{ '/accessibility/m6/' | relative_url }}) —
  automated and manual results plus known boundaries.
- [v0.2.0 release notes]({{ '/releases/v0.2.0/' | relative_url }}) — M6
  highlights and explicit limitations.
- [Architecture]({{ '/architecture/' | relative_url }}) — component boundaries
  and data flow.
- [MCP server]({{ '/mcp/' | relative_url }}) — connect external AI agents
  (Claude Code, Claude Desktop).
- [ECU protocol]({{ '/protocol/' | relative_url }}) and
  [INI format]({{ '/ini-format/' | relative_url }}) — technical references.
- [Roadmap]({{ '/roadmap/' | relative_url }}) and
  [ADRs]({{ '/adr/' | relative_url }}) — delivery status and design decisions.
- [Contributing](https://github.com/d0pawlus/OpenTune/blob/main/CONTRIBUTING.md)
  — local setup, tests, and pull requests.

## Safety status

OpenTune is pre-1.0 software. M6 validates file interoperability and the
hardware-free simulator, but it does not establish physical-ECU burn safety.
Keep a recoverable tune backup and confirm that the selected INI matches the
exact ECU firmware before writing to real hardware.

macOS and Windows packages are currently unsigned by an operating-system
publisher certificate. Apple notarization and Windows code signing are deferred
until the project owner obtains the required accounts and certificates. Updater
archives use a separate Tauri signature and are verified before installation.
