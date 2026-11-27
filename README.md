<p align="right">
  <strong>English</strong> • <a href="README.pt-BR.md">🇧🇷 Versão em Português (pt-BR)</a>
</p>

# Duonn Axios 24 — DS Mixer Pro (Rust Edition & Android) `v0.4.9`

Modern, ultra-lightweight (~2.5 MB), open-source **Rust** (Windows / Linux) and **Native Android** (Kotlin + Jetpack Compose) remote control suite for the **Duonn Axios 24** digital audio mixing console (via LAN/Wi-Fi UDP/TCP or USB Serial). Features a sleek user interface inspired by **DS Mixer**, real-time EQ/Gate/Compressor curves, **Real FFT RTA Spectrum Analyzer**, 100% compatible official `.scn` scene management, an **Integrated Virtual Hardware Emulator**, and **GitHub hot auto-updates**.

> **Vibecoded Project** — Collaboratively developed via *vibecoding*!  
> **Authors**: **Leandro Pinheiro** ([@leandrop2103](https://github.com/leandrop2103)) & **Antigravity** (Google Deepmind AI)  
> **License**: [MIT](LICENSE)

---

## Key Features

- **Automatic Connection & Multi-Protocol**:
  - Auto-discovery across local network (LAN) and factory/Wi-Fi IPs (`172.14.10.1`, `192.168.4.1`, `172.14.11.1`, `172.14.30.1`) on **UDP 8086**.
  - Supports **UDP (8086)**, **TCP (8087)**, **USB Serial (115200 bps)**, and **Offline Demo Mode** with audio and meter simulation.
- **Modern & Customizable UI**:
  - **Normal Mode**, **Analog Mode**, and **Big Desktop** layout with persistent `MAIN L & R` master strip pinned on the right.
  - **Dark and Light Themes**, switchable via `CONFIG / SCENES > SYSTEM` or `F11`.
  - **Name on top and Channel badge at the bottom** on each strip, with double-click inline renaming and **32 customizable colors** per channel.
- **Professional Mixing Capabilities**:
  - **Sends on Fader** (`MAIN L/R`, `AUX 1..8`, `FX 1..2`) with dedicated **`PRE FADER` / `POST FADER`** toggle directly on each channel strip.
  - **4/7-band Parametric EQ + HPF/LPF** with interactive frequency response curves (real DSP biquad filter calculations).
  - **2048-point Real FFT RTA** via soundcard / USB audio interface (`WinMM` on Windows, ALSA on Linux) with peak frequency detection and **Anti-Feedback Notch Filter**.
  - **Interactive Gate & Compressor graphs** with live threshold and gain reduction visualization.
  - **Scene Import & Export (`.scn`)**: 100% binary compatibility with the official Axios 24 software format (including channel names and all 3,413 DSP registers) + 16-scene onboard storage (`preset.bin`).
  - **Safety & Audio Tools**: Fast L/R routing restore, **Unplug Cable Safe Mode** (click-free muting), `+48V` anti-pop protection, **Auto-Gain (-18 dBFS)**, 12 Channel Presets, 4 Mute Groups, 4 +48V Groups, and Live Stream Deck Macros.
- **Hot Auto-Update (GitHub Releases)**:
  - Automatically checks for updates on startup or in `CONFIG / SCENES > SYSTEM`, displays release notes, and updates in 1 click by seamlessly relaunching the application.

---

## How to Build

### 1. Prerequisites (Windows and Linux)
Install **Rust** via [rustup.rs](https://rustup.rs/).

### 2. Build on Windows
1. Install **Visual Studio Build Tools** (select *"Desktop development with C++"* / MSVC + Windows SDK).
2. Open PowerShell or a terminal in the project directory and run:
   ```powershell
   cargo test
   cargo build --release
   ```
3. The optimized executable (~2.5 MB, with Ferris mixer icon embedded) will be generated at:
   ```text
   target\release\axios24.exe
   ```

### 3. Build on Linux (Ubuntu / Debian / Arch / Fedora)
1. Install system dependencies for OpenGL rendering (`eframe`/`winit`), native file dialogs (`rfd` via GTK3), and serial ports (`serialport` via `libudev`):

   **Ubuntu / Debian:**
   ```bash
   sudo apt update
   sudo apt install -y build-essential pkg-config libudev-dev libgtk-3-dev \
     libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
     libxkbcommon-dev libssl-dev libgl1-mesa-dev libasound2-dev curl
   ```

   **Fedora:**
   ```bash
   sudo dnf install -y gcc gcc-c++ pkgconfig systemd-devel gtk3-devel \
     libxcb-devel libxkbcommon-devel mesa-libGL-devel alsa-lib-devel curl
   ```

   **Arch Linux:**
   ```bash
   sudo pacman -S --needed base-devel pkgconf systemd gtk3 libxcb libxkbcommon mesa alsa-lib curl
   ```

2. Clone and build:
   ```bash
   git clone https://github.com/leandrop2103/axios24-rust.git
   cd axios24-rust
   cargo test
   cargo build --release
   ```
3. The binary will be generated at:
   ```bash
   ./target/release/axios24
   ```

*(On Linux, multitrack audio playback, FFT RTA, and USB MIDI operate natively via ALSA / PipeWire through `cpal` and `midir`.)*

### 4. Build the Virtual Hardware Emulator (Windows / Linux)
To compile and run the offline testing emulator:
```powershell
cargo run --release --bin axios24_emulator
# or compile only:
cargo build --release --bin axios24_emulator
```
The optimized executable will be generated at:
```text
target\release\axios24_emulator.exe
```

### 5. Build the Native Android App (Kotlin + Jetpack Compose)
1. Make sure you have **JDK 17** and the **Android SDK** installed.
2. In the project directory, navigate to the `android` folder and build:
```bash
cd android
./gradlew assembleRelease
```
The signed APK will be generated at:
```text
android/app/build/outputs/apk/release/app-release.apk
```

---

## Virtual Hardware Emulator (`axios24_emulator`)

The repository includes a high-fidelity **Virtual Hardware Emulator** developed in pure Rust. It allows developers and sound engineers to design, debug, and test the Desktop Rust app, the Android app, and even the manufacturer's official software (`AX24E`), without having the physical mixer connected.

### Emulator Features:
- **Real-Time Network & Protocols**:
  - Listens simultaneously on **UDP 8086** (responding with broadcast and unicast packets back to local port **1080**) and **TCP 8087**.
  - Optional bridging support with virtual COM serial ports (e.g., via `com0com`).
  - Responds to official auto-discovery commands (`0x1F` / `0x20` / `0x21`) identifying as `AX24E-EMU` running firmware `AX24E.25.11.01.001`.
- **Benchtop Rack Visual Layout**:
  - **70% Left Panel**: 24 circular channel indicators (`CH 1..24`).
    - **Green**: Active / Unmuted channel.
    - **Red**: Muted channel.
    - **Yellow Ring**: Channel in Solo.
    - **Pulsing Halo & Meter**: Immediate real-time visual feedback for fader volume and VU level.
  - **30% Right Panel**: 2 prominent circles for `MASTER L` and `MASTER R` accompanied by a dual graduated stereo VU meter.
  - **Footer Bar**: Horizontal strip with 8 circular indicators dedicated to auxiliary channels (`AUX 1..8`).
- **Integrated 16-Scene Bank**:
  - Full persistence of 16 scenes stored in a local binary file (`emulator_scenes.bin`).
  - Full support for official scene recall (`0x0A`) and scene save (`0x0B`) commands, preserving custom channel names, volumes, mutes, L/R assignments, and DSP parameters.
- **Multi-Client Live Synchronization**:
  - Adjusting any fader, mute, or solo in the Rust application, the Android app, or the manufacturer's official software updates the emulator instantaneously and is broadcast (`0x25`) to all other connected clients on the network.

---

## Getting Started

1. **When launching the application**, the **Connect to Mixer** window appears automatically and searches your local network (LAN) and the mixer's Wi-Fi hotspot (`172.14.10.1` / `192.168.4.1`).
   - If the mixer or emulator is discovered, click **Connect Now**.
   - If you want to prepare or inspect a scene offline, click **Enter DEMO MODE**.
2. **Renaming & Color-Coding Channels**:
   - **Double-click** the channel name above the fader to edit it inline.
   - **Right-click** any channel strip to select from **32 colors**, rename, copy, or paste channel settings.
3. **Pre / Post Fader on Auxiliaries**:
   - In the `FADERS:` selector bar, click `AUX1..AUX8` to switch to *Sends on Fader*. Each strip displays an interactive **`PRE FADER` / `POST FADER`** toggle.
4. **Scenes (`.scn`)**:
   - Navigate to `CONFIG / SCENES > SCENES` to import or export `.scn` files fully compatible with the official software.

---

## Keyboard Shortcuts

You can also inspect this shortcut list inside the application via **`CONFIG / SCENES > SHORTCUTS`**.

### Faders, Channels & Mixing
| Shortcut | Action |
| :--- | :--- |
| `Up / Down Arrow` | Nudge fader on selected channel (`±1.0 dB`) |
| `Shift + Up / Down Arrow` | Fine-precision fader adjustment (`±0.1 dB`) |
| `Left / Right Arrow` | Select previous / next channel |
| `0` or `Home` | Reset selected channel fader to Unity (`0.0 dB`) |
| `M` | Mute / Unmute selected channel |
| `S` | Toggle Solo (`S`) on selected channel |
| `L` | Toggle channel routing to Master `L/R` |
| `P` | Toggle `PRE / POST Fader` (in `AUX` Sends on Fader) |
| `Ctrl + R` | Inline rename selected channel |
| `Ctrl + C` / `Ctrl + V` | Copy / Paste settings of selected channel |
| `Ctrl + D` | Toggle `DIM -20 dB` on Master `L/R` |
| `Ctrl + Shift + S` | Clear all active solos (`CLR SOLO`) |

### Layers & Sends on Fader
| Shortcut | Action |
| :--- | :--- |
| `F1` .. `F7` | Switch Layers: `CH 1-8`, `CH 9-16`, `CH 17-24`, `DIGI/FX`, `AUX 1-8`, `DCA 1-4`, `ALL 1-24` |
| `Alt + 1` .. `Alt + 8` | Sends on Fader: control sends to `AUX 1` .. `AUX 8` |
| `Alt + 0` or `Esc` | Return faders to `MAIN L/R` (and dismiss search/dialogs) |

### Main Tabs & System Controls
| Shortcut | Action |
| :--- | :--- |
| `Ctrl + 1` .. `Ctrl + 6` | Switch main tabs (`Mixer`, `Edit`, `EQ/RTA`, `FX`, `Player`, `Config`) |
| `Ctrl + F` | Open / Close quick channel search |
| `F8` | Cycle View Mode (`Normal` / `Analog` / `Big Desktop`) |
| `F9` | Start / Pause `DEMO` Audio Simulator |
| `F10` | Open **Connect to Mixer / Network** dialog |
| `F11` | Toggle **Dark Theme / Light Theme** |
| `F12` | Lock Console (Password screen) |

---

## Legal Notice and Compatibility (MIT)

This project is an **independent, unofficial** open-source reimplementation (under the **MIT license**) designed for **interoperability** for owners of the Duonn Axios 24 digital audio mixing console.  
- No proprietary binaries, copyrighted firmware images, or original source code from the manufacturer are redistributed within this repository.  
- The virtual hardware emulator (`axios24_emulator`) is an original clean-room implementation in Rust communicating via standard network sockets (UDP/TCP), fully complying with reverse-engineering interoperability standards and GitHub terms of service.  
- "Duonn" and "Axios 24" are registered trademarks of their respective owners, referenced here solely for hardware compatibility purposes.
