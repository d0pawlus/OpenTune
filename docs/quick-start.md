---
layout: page
title: Quick start
permalink: /quick-start/
---

## 1. Explore with the simulator

1. Start OpenTune and choose **Use simulator** in **Connect to ECU**.
2. Select **Connect to simulator**.
3. Explore the dashboard, tune dialogs, tables, reconnect behavior, and datalog
   tools without an ECU attached.

The simulator is the safest first run and is also the supported contributor
workflow when physical hardware is unavailable.

## 2. Open and save a tune offline

Use one of the actions under **Offline tune**:

- **Open project** selects a TunerStudio project directory containing
  `projectCfg/mainController.ini` and `CurrentTune.msq`.
- **Open tune** selects an INI and MSQ separately.
- **New tune** starts from an INI definition without importing an existing MSQ.

Review the load report. `Failed to load: 0` is the compatibility gate. Skipped
settings are names present in the MSQ but absent from the active definition;
clamped settings were outside the INI's declared physical bounds. Save to a new
`.msq` path so the source tune remains recoverable.

## 3. Connect real hardware carefully

Before connecting:

1. confirm the INI signature/version matches the ECU firmware exactly;
2. save the current known-good tune somewhere outside the working project;
3. test reading and editing without burning first;
4. keep a vendor-supported recovery path available.

M6 has file- and simulator-level evidence for Speeduino, rusEFI, and MegaSquirt
MS3. It does not claim physical-hardware burn validation.

## Language, contrast, and welcome guide

The first-run guide lets you select English or Polish and the default or
high-contrast theme. OpenTune stores those choices locally. Use **Show welcome
guide** in the application footer to reopen it at any time.

Continue with [install and update guidance]({{ '/updates/' | relative_url }}) or
review the [M6 compatibility matrix]({{ '/compatibility/m6/' | relative_url }}).
