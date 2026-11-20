# RECON

Native, local-first VALORANT match-history viewer built with Rust, egui and SQLite.

**Prototype status:** Windows GUI and HenrikDev integration require real-world testing. This is not an official Riot Games product.

## Download (Windows)

Open [Actions](../../actions/workflows/windows.yml), select the latest successful **Windows build**, and download the **RECON-Windows-x64** artifact. Extract the ZIP, then run RECON.exe.

GitHub requires sign-in to download workflow artifacts. If the build is still running or failed, no executable is available yet.

## Signing and Windows Smart App Control

RECON builds are currently **unsigned**. Smart App Control may block them. The workflow publishes a SHA-256 checksum for artifact integrity, not a trusted publisher identity. See [Code signing policy](CODE_SIGNING.md) for the plan to seek free trusted signing through SignPath Foundation. Do not disable system-wide protections solely to run RECON.

## Setup

1. Open RECON and select **Settings**.
2. Enter your Riot ID name and tag separately, e.g. Player and ABC.
3. Select your region (ap for Asia-Pacific).
4. Enter a HenrikDev API key and press **Save settings**. The key is stored in the OS credential store, not in SQLite or the repository.
5. Press **Sync matches** to fetch your recent match history. The UI remains responsive.
6. Matches remain in a local SQLite database for offline viewing.

The first version shows match results, map, agent, K/D/A, calculated ACS and an overview. It does **not** include a live overlay, opponent scouting, automated capture, Cloudflare, or clip analysis.

## Privacy and storage

Data is stored under your OS's local application-data folder in a \`recon\` directory. Only match data is saved in the local database. The API key is kept in the OS credential store. Nothing is uploaded by RECON to any RECON-operated cloud service.

RECON makes direct HTTPS requests to HenrikDev with your API key when you choose Sync. Only request data you are authorized to access. HenrikDev is an unofficial third-party service; its availability, response formats, and terms may change.

## Build

Install stable Rust with Cargo, then:

\`\`\`sh
cargo run
\`\`\`

The GitHub Actions workflow runs format and tests and creates a release executable on Windows. No API key is required to build.

## Roadmap

- Rich match detail and round timeline
- Verified analytics with source/evidence labels
- Optional clip library and manually selected sharing
- Optional Cloudflare sync/mobile viewer
- SETTLE drill integration

No live tactical advice or prohibited opponent scouting.
