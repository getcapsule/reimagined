<div align="center">

# Capsule

**All your music, in one place.**

[![CI/CD Status](https://img.shields.io/github/actions/workflow/status/getcapsule/reimagined/ci.yml?branch=master&label=CI&labelColor=18181b&color=238636&style=flat-square&logo=githubactions&logoColor=white)](https://github.com/getcapsule/reimagined/actions)
[![Latest Release](https://img.shields.io/github/v/release/getcapsule/reimagined?label=release&labelColor=18181b&color=1f6feb&style=flat-square&logo=github&logoColor=white)](https://github.com/getcapsule/reimagined/releases)
[![Discord](https://img.shields.io/badge/Discord-chat-3b429f?labelColor=18181b&logo=discord&logoColor=white&style=flat-square)](https://discord.gg/77np9hVV7M)
[![Platform Support](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-27272a?labelColor=18181b&style=flat-square&logo=linux&logoColor=white)](#)
[![License: GPL v3](https://img.shields.io/github/license/getcapsule/reimagined?label=license&labelColor=18181b&color=3f3f46&style=flat-square&logo=gnu&logoColor=white)](https://www.gnu.org/licenses/gpl-3.0)

Capsule is a music player built with Rust and Iced.

[Getting Started](#getting-started) ✦ [Preview](#preview) ✦ [Architecture](#architecture) ✦ [Philosophy](#philosophy) ✦ [Key Highlights](#keypoints) ✦ [Build Yourself](#build) 

</div>



<div align="center">

## Preview 
![preview](./assets/preview.png)

</div>

## Getting started

Download a Windows or Linux build from [Releases](https://github.com/getcapsule/reimagined/releases) or the latest successful [workflow run](https://github.com/getcapsule/reimagined/actions/workflows/github.yml). Extract the archive and launch `capsule.exe` on Windows or `./capsule` on Linux.

1. Browse **Discover** for songs from YouTube Music, or use **Ctrl+K** to search YouTube and your local library.
2. Use **Open files** to play individual files, or choose your music folder in **Library**.
3. Right-click tracks to add them to the queue or a playlist. Double-click a playlist track to play it.
4. Open **Settings** to adjust glass opacity and background blur. The sidebar and queue can be shown or hidden from their buttons.

Internet access is required for online discovery and playback. Capsule sets up yt-dlp and a JavaScript runtime when needed.

## Architecture

`src/main.rs` starts the app. Core modules are exposed through `src/lib.rs`, with the interface in `src/player_ui/`.

| Module | Responsibility |
| --- | --- |
| `src/audio/` | Symphonia/libopus decoding, Rubato resampling, and buffered playback through ringbuf and CPAL. |
| `src/providers/` | YouTube metadata, discovery, and stream resolution through the yt-dlp 1.4.6 crate. |
| `src/library/` | Local file indexing, metadata, and saved playlists in SQLite. |
| `src/queue.rs` | Playback order, queue navigation, and shuffle. |
| `src/player_ui/` | Iced views, iced_glass surfaces, artwork, search, and desktop window controls. |

YouTube audio streams directly into the playback pipeline without saving the audio to disk. Local files use the same decoding and output pipeline.

## Philosophy

- **Privacy:** No Capsule telemetry. Library and playlist data stay on your device; online features connect directly to their providers.
- **Efficiency:** Keep resource use low with native playback, bounded buffers, and work that runs only when needed.
- **Modularity:** Keep playback, providers, library storage, and the interface separate.
- **Extensibility:** Make providers and configuration straightforward to extend without rewriting the player.

<a name="keypoints"></a>

## Key Highlights

- Local playback for MP3, FLAC, WAV, Vorbis, AAC, and Opus.
- Music-only YouTube Music discovery, in-app YouTube search, and direct streaming.
- Saved playlists, queue management, shuffle, repeat, seeking, and volume control.
- Ctrl+K search and commands, track context menus, and double-click playlist playback.
- Cover-tinted glass, adjustable opacity and blur, a collapsible sidebar, a toggleable queue, and loading skeletons.

<a name="build"></a>

## Build Yourself

Install Rust, Git, CMake, and a C++ compiler. CI uses Rust **1.94.1**, with the `rustfmt` and `clippy` components.

**Windows:** Install Visual Studio Build Tools with **Desktop development with C++**, the Windows SDK, and CMake tools. Run the build from an x64 developer terminal.

**Linux (Ubuntu 22.04):** Install the native build dependencies:

```bash
sudo apt-get update
sudo apt-get install --no-install-recommends -y \
  build-essential cmake ninja-build pkg-config \
  libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev \
  libegl1-mesa-dev libgl1-mesa-dev fonts-dejavu-core
```

Clone and build:

```sh
git clone https://github.com/getcapsule/reimagined.git
cd reimagined
cargo build --locked --release --bin capsule
```

The executable is `target/release/capsule.exe` on Windows or `target/release/capsule` on Linux. Keep `vendor/iced_wgpu` in the checkout; Cargo uses it through a path patch.

Run the checks:

```sh
cargo fmt --all --check
cargo clippy --locked --release --all-targets -- -D warnings
cargo test --locked --release
```

Tests requiring an audio output device or live YouTube access are ignored by default.
