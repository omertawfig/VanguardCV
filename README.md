<p align="center">
  <img src="assets/branding/logo.png" alt="VanguardCV logo" width="140" height="140" />
</p>

<h1 align="center">VanguardCV</h1>

<p align="center">
  Native, GPU-accelerated desktop AI CV studio written in Rust with <a href="https://gpui.rs">GPUI</a>.<br/>
  Non-destructive Master → Variation CV trees, zero-trust PII air-gap, embedded Typst PDF engine, live Job Discovery, and automatic Windows updates.
</p>

---

## Features

- **Master → Variation CV Lineage:** Keep protected master CVs in a local SQLite database and create tailored job-specific variations without touching your master record.
- **Master CV Import (.docx & .pdf):** Import existing Word or PDF CVs with automatic structure detection (including VanguardCV's own PDF layout), strict contact-header PII exclusion, and an interactive line-by-line verification screen before saving.
- **Zero-Trust PII Air-Gap:** Your name, email, phone, location, and personal links stay strictly local on your device and are stripped before any AI request is sent.
- **Flexible AI Providers:** Connect using official **Sign in with ChatGPT** (OAuth 2.0 + PKCE) or your own **Google Gemini API** key stored securely in the Windows Credential Manager (never in SQLite).
- **Job Discovery Tab:** Generate PII-stripped role search profiles per CV, browse live job postings filtered by country, freshness, and remote options, and launch one-click AI CV tailoring.
- **Inline AI Review & Diffs:** Review, edit, accept, or revert AI-proposed rewrites and visibility toggles with full rationale and ATS alignment scoring.
- **Embedded Typst PDF Rendering:** Instant live split preview and clean, ATS-friendly PDF export with bundled Libertinus Serif fonts.
- **Custom AI Skills (`omp`-style):** Enable, disable, import, or author reusable `SKILL.md` tailoring directives.
- **Clean First Launch:** Every shipped Windows release starts with an empty local SQLite database — zero sample CVs or bundled personal data.
- **Built-in Windows Auto-Updater:** Checks official GitHub Releases automatically, verifies SHA-256 checksums against `SHA256SUMS.txt`, and installs updates smoothly with an on/off toggle in **Settings → Software updates**.

## Download (Windows x64)

Download the latest signed-checksum Windows installer from [**GitHub Releases**](https://github.com/omertawfig/VanguardCV/releases/latest):

1. Run `VanguardCV-Setup-0.0.2-x64.exe` (per-user installer; installs to `%LOCALAPPDATA%\Programs\VanguardCV`).
2. Verify checksum (optional):
   ```powershell
   Get-FileHash .\VanguardCV-Setup-0.0.2-x64.exe -Algorithm SHA256
   ```
3. Future updates can be checked, downloaded, and applied automatically from **Settings → Software updates** (toggle **Automatic updates** on or off at any time).

## Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+N` | New job variation wizard |
| `Ctrl+E` | Export the current CV to PDF |
| `Ctrl+P` | Open AI Studio / toggle the AI review panel |
| `Ctrl+Enter` | Accept all pending AI proposals |

## Building from Source (Windows)

Requires Rust (stable MSVC toolchain) and Windows SDK (`rc.exe`).

```powershell
cargo clippy --features ui --all-targets -- -D warnings
cargo test --features ui
cargo build --release --features ui
```

The compiled executable is written to `target\release\vanguard-cv.exe`.

Local application data is stored in `%APPDATA%\vanguard-cv\vanguard.db` and exported PDFs default to `Documents\VanguardCV\`.

## Project Layout

```text
src/
  contracts.rs   shared domain model, service traits, settings keys, AI events
  credentials.rs Windows Credential Manager / private local token storage
  updater.rs     GitHub Releases auto-updater with SHA-256 checksum verification
  db/            SQLite storage, lineage diffing, debounced sync, snapshots
  importer/      DOCX/PDF text extraction, No-PII header exclusion, line classifier
  privacy/       Aho-Corasick + regex PII scrubber
  ai/            ChatGPT OAuth & Google Gemini clients, skills, proposal diffs
  jobs/          PII-stripped search profiles, Freehire provider, external board links
  pdf/           embedded Typst compiler, raster cache, PDF exporter
  ui/            GPUI workspace, sidebar, library, editor, jobs, preview, settings
assets/
  branding/      VanguardCV logo PNG and Windows multi-resolution ICO
  templates/     classic_serif.typ
  fonts/         bundled Libertinus Serif (SIL OFL 1.1)
installer/       Inno Setup 6 Windows x64 installer script
skills/          bundled omp-style skill manifests
```

## License

Copyright © 2026 **Omer Tawfig**. All rights reserved.

VanguardCV is released under the **VanguardCV Source-Available License (All Rights Reserved)** — see [`LICENSE`](LICENSE) for full terms. Source code is publicly viewable for inspection and auditing, and unmodified official releases may be used for personal, non-commercial use. Copying, redistribution, modification, commercial use, or publishing forks/derivative versions without prior written permission from the author is strictly prohibited.
