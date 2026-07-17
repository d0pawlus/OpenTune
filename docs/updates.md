---
layout: page
title: Install, update, and recover
permalink: /updates/
---

## Installers

Published installers are on the
[GitHub Releases page](https://github.com/d0pawlus/OpenTune/releases). Each
stable release includes a `SHA256SUMS` file.

- **macOS:** packages are not Apple-notarized yet. macOS may require
  **System Settings → Privacy & Security → Open Anyway**. Alternatively run
  `xattr -cr /Applications/OpenTune.app` after verifying the checksum.
- **Windows:** packages are not Authenticode-signed yet. SmartScreen reports an
  unknown publisher; verify the checksum before choosing **More info → Run
  anyway**.
- **Linux:** use the AppImage, deb, or rpm package. AppImage users must run
  `chmod +x OpenTune_*.AppImage`. Serial access may require membership in the
  distribution's serial-port group (commonly `dialout`).

Apple Developer ID/notarization and Windows publisher signing will be added
after the project owner obtains the required account and certificates.

Release CI verifies the supported installer families, updater manifest entries,
at least one downloaded artifact against its Tauri signature, and produces
`SHA256SUMS` before a draft is published.

## Application updates

OpenTune checks the stable GitHub release endpoint after startup without
blocking offline work or ECU features. If a newer version exists, the app shows
its version and release notes. It downloads, installs, and restarts only after
the user chooses **Install and restart**.

Updater archives carry a Tauri updater signature. The public key is embedded in
the application, and installation stops if the manifest, download, or signature
is invalid. This signature protects the update chain but is separate from the
deferred macOS and Windows publisher certificates.

Use **Check for updates** for a manual retry. Network errors remain visible and
retryable; they never prevent local tuning.

## Verify and recover

To verify a downloaded file:

```bash
shasum -a 256 OpenTune_downloaded_file
grep 'OpenTune_downloaded_file' SHA256SUMS
```

The values must match exactly. To roll back, uninstall the current app while
keeping your projects, download the required older release from GitHub, verify
its checksum, and reinstall it. Never replace or discard your only known-good
ECU tune during an application update.
