<p align="right">
  <strong>🇧🇷 Português (Brasil)</strong> • <a href="README.md">🇺🇸 Switch to English</a>
</p>

# Duonn Axios 24 — DS Mixer Pro (Rust Edition & Android) `v0.4.9`

Interface moderna, ultra-leve (~2,5 MB) e de código aberto em **Rust** (Windows / Linux) e **Android Nativo** (Kotlin + Jetpack Compose) para controle remoto da mesa de som digital **Duonn Axios 24** (via Rede LAN/Wi-Fi UDP/TCP ou USB Serial), com visual inspirado no **DS Mixer**, curvas de EQ/Gate/Compressor em tempo real, Analisador de Espectro **RTA FFT Real**, suporte completo a cenas oficiais `.scn`, **Emulador de Hardware Virtual Integrado** e **Auto-Update a quente via GitHub**.

> **Projeto Vibecodado** — Desenvolvido de forma colaborativa via *vibecoding*!  
> **Autores**: **Leandro Pinheiro** ([@leandrop2103](https://github.com/leandrop2103)) & **Antigravity** (Google Deepmind AI)  
> **Licença**: [MIT](LICENSE)

---

## Principais Recursos

- **Conexão Automática & Multi-Protocolo**:
  - Busca automática na rede interna (LAN) e nos IPs Wi-Fi/fábrica da mesa (`172.14.10.1`, `192.168.4.1`, `172.14.11.1`, `172.14.30.1`) na porta **UDP 8086**.
  - Suporte a **UDP (8086)**, **TCP (8087)**, **USB Serial (115200 bps)** e **Modo Demo Offline** com simulador de áudio e medidores.
- **Interface Moderna & Personalizável**:
  - **Modo Normal**, **Modo Analógico** e **Big Desktop** com `MAIN L & R` sempre fixos na lateral direita.
  - **Tema Escuro (Dark) e Claro (Light)** alternável em `CONFIG / CENAS > SISTEMA` ou tecla `F11`.
  - **Nome em cima e Canal em baixo** em cada strip, com duplo clique para renomear inline e **32 cores** personalizáveis por canal.
- **Recursos Profissionais de Mixagem**:
  - **Sends on Fader** (`MAIN L/R`, `AUX 1..8`, `FX 1..2`) com botão **`PRE FADER` / `POST FADER`** direto no canal.
  - **PEQ Paramétrico de 4/7 bandas + HPF/LPF** com gráfico interativo de resposta em frequência (biquads DSP reais).
  - **RTA FFT Real** de 2048 pontos via placa de som / interface USB (`WinMM` no Windows) + detecção de pico de frequência e **Notch Anti-Microfonia**.
  - **Gate & Compressor** com curvas gráficas interativas.
  - **Importação e Exportação de Cenas `.scn`** 100% compatível com o formato oficial da Axios 24 (incluindo nomes dos canais e todos os 3.413 registradores) + banco de 16 cenas (`preset.bin`).
  - **Proteções & Correções**: Correção rápida de envio `L/R`, Modo **Desplugar Cabo** (silencia sem estalo), proteção anti-estalo `+48V`, **Auto-Gain (-18 dBFS)**, 12 Presets de Canal, 4 Grupos de Mute, 4 Grupos +48V, Macros Live (Stream Deck).
- **Auto-Update a Quente (GitHub Releases)**:
  - Ao abrir o programa (ou em `CONFIG / CENAS > SISTEMA`), verifica automaticamente se há nova versão no GitHub, exibe o resumo das novidades e permite atualizar com 1 clique reiniciando automaticamente.

---

## Como Compilar

### 1. Pré-requisitos (Windows e Linux)
Instale o **Rust** via [rustup.rs](https://rustup.rs/).

### 2. Compilar no Windows
1. Instale o **Visual Studio Build Tools** (carga de trabalho *"Desenvolvimento para desktop com C++"* / MSVC + Windows SDK).
2. Abra o PowerShell ou terminal na pasta do projeto e execute:
   ```powershell
   cargo test
   cargo build --release
   ```
3. O executável otimizado (~2,5 MB, já com o ícone do Ferris embutido) será gerado em:
   ```text
   target\release\axios24.exe
   ```

### 3. Compilar no Linux (Ubuntu / Debian / Arch / Fedora)
1. Instale as dependências de sistema para janela OpenGL (`eframe`/`winit`), diálogo de arquivos (`rfd` via GTK3) e porta serial (`serialport` via `libudev`):

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

2. Clone e compile:
   ```bash
   git clone https://github.com/leandrop2103/axios24-rust.git
   cd axios24-rust
   cargo test
   cargo build --release
   ```
3. O binário será gerado em:
   ```bash
   ./target/release/axios24
   ```

*(No Linux, o áudio do Multitrack, o RTA FFT e o MIDI USB funcionam nativamente via ALSA / PipeWire através de `cpal` e `midir`.)*

### 4. Compilar o Emulador de Hardware Virtual (Windows / Linux)
Para compilar e executar o emulador de bancada para testes offline:
```powershell
cargo run --release --bin axios24_emulator
# ou apenas compilar:
cargo build --release --bin axios24_emulator
```
O executável otimizado será gerado em:
```text
target\release\axios24_emulator.exe
```

### 5. Compilar o App Android (Kotlin + Jetpack Compose)
1. Certifique-se de ter o **JDK 17** e o **Android SDK** configurados.
2. Na raiz do projeto, acesse a pasta `android` e execute:
```bash
cd android
./gradlew assembleRelease
```
O APK final assinado será gerado em:
```text
android/app/build/outputs/apk/release/app-release.apk
```

---

## Emulador de Hardware Virtual (`axios24_emulator`)

O repositório inclui um **Emulador de Hardware Virtual** de alta fidelidade desenvolvido em Rust puro. Ele permite desenvolver, depurar e testar tanto os nossos aplicativos (Desktop Rust e Android) quanto o próprio software oficial do fabricante, sem necessidade de estar fisicamente conectado à mesa de som.

### Recursos do Emulador:
- **Rede e Protocolos em Tempo Real**:
  - Escuta simultaneamente na porta **UDP 8086** (com respostas broadcast e unicast para a porta local padrão **1080**) e **TCP 8087**.
  - Suporte opcional a bridge via porta serial COM virtual (ex.: `com0com`).
  - Responde ao comando oficial de auto-descoberta (`0x1F` / `0x20` / `0x21`) identificando-se como `AX24E-EMU` com firmware `AX24E.25.11.01.001`.
- **Layout Visual de Bancada**:
  - **70% à esquerda**: 24 bolinhas de canais de entrada (`CH 1..24`).
    - **Verde**: Canal ativo / desmutado.
    - **Vermelho**: Canal mutado.
    - **Anel Amarelo**: Canal em Solo.
    - **Halo Pulsante e Nível**: Indicação visual imediata do fader e VU.
  - **30% à direita**: 2 bolinhas de destaque para `MASTER L` e `MASTER R` com medição VU estéreo ao vivo em barra graduada.
  - **Rodapé**: Barra horizontal com 8 bolinhas dedicadas aos canais auxiliares (`AUX 1..8`).
- **Banco de 16 Cenas Integrado**:
  - Gerenciamento completo de 16 cenas persistentes em arquivo local (`emulator_scenes.bin`).
  - Suporta operações de recall de cena (`0x0A`) e salvamento de cena (`0x0B`), preservando nomes de canais, volumes, mutes, envios L/R e equalizações.
- **Sincronização Multi-Cliente**:
  - Qualquer fader, botão de mudo ou solo acionado no software em Rust, no app Android ou no software oficial da mesa atualiza o emulador instantaneamente e é propagado via broadcast (`0x25`) para os demais dispositivos conectados na rede local.

---

## Como Usar

1. **Ao abrir o aplicativo**, a janela **Conectar ao Mixer** aparece automaticamente e busca na sua rede interna (LAN) e no Wi-Fi da mesa (`172.14.10.1` / `192.168.4.1`).
   - Se a mesa for encontrada, clique em **Conectar Agora**.
   - Se quiser testar ou preparar uma cena offline, clique em **Entrar em MODO DEMO**.
2. **Renomear e Colorir Canais**:
   - Dê **duplo clique** no nome no topo do fader para editar na hora.
   - Clique com o **botão direito** no canal para escolher entre as **32 cores**, renomear, copiar ou colar.
3. **Pré / Pós Fader nos Auxiliares**:
   - Na barra `FADERS:`, clique em `AUX1..AUX8` para entrar em *Sends on Fader*. Cada canal mostrará o botão **`PRE FADER` / `POST FADER`**.
4. **Cenas `.scn`**:
   - Vá em `CONFIG / CENAS > CENAS` para importar ou exportar arquivos `.scn` compatíveis com o software original.

---

## Atalhos de Teclado

Você também pode consultar esta lista dentro do aplicativo em **`CONFIG / CENAS > ATALHOS`**.

### Faders, Canais e Mixagem
| Atalho | Ação |
| :--- | :--- |
| `Seta Cima / Baixo` | Subir / Descer fader do canal selecionado (`±1.0 dB`) |
| `Shift + Seta Cima / Baixo` | Ajuste fino de precisão no fader (`±0.1 dB`) |
| `Seta Esquerda / Direita` | Selecionar canal anterior / próximo |
| `0` ou `Home` | Resetar fader selecionado para Unity (`0.0 dB`) |
| `M` | Mutar / Desmutar canal selecionado |
| `S` | Ativar / Desativar Solo (`S`) no canal selecionado |
| `L` | Ligar / Desligar envio do canal para o Master `L/R` |
| `P` | Alternar `PRE / POST Fader` (em Sends on Fader `AUX`) |
| `Ctrl + R` | Renomear canal selecionado inline |
| `Ctrl + C` / `Ctrl + V` | Copiar / Colar configurações do canal selecionado |
| `Ctrl + D` | Ativar / Desativar `DIM -20 dB` no Master `L/R` |
| `Ctrl + Shift + S` | Limpar todos os Solos ativos (`CLR SOLO`) |

### Camadas (Layers) e Sends on Fader
| Atalho | Ação |
| :--- | :--- |
| `F1` .. `F7` | Camadas: `CH 1-8`, `CH 9-16`, `CH 17-24`, `DIGI/FX`, `AUX 1-8`, `DCA 1-4`, `TODOS 1-24` |
| `Alt + 1` .. `Alt + 8` | Sends on Fader: controlar envio para `AUX 1` .. `AUX 8` |
| `Alt + 0` ou `Esc` | Voltar faders para `MAIN L/R` (e fechar busca/modal) |

### Abas Principais e Sistema
| Atalho | Ação |
| :--- | :--- |
| `Ctrl + 1` .. `Ctrl + 6` | Navegar entre as abas (`Mixer`, `Editar`, `EQ/RTA`, `Efeitos`, `Player`, `Config`) |
| `Ctrl + F` | Abrir / Fechar busca rápida de canais |
| `F8` | Alternar Modo de Visão (`Normal` / `Analógico` / `Big Desktop`) |
| `F9` | Ligar / Pausar Simulador `DEMO` |
| `F10` | Abrir janela **Conectar ao Mixer / Rede** |
| `F11` | Alternar **Tema Escuro (Dark) / Claro (Light)** |
| `F12` | Bloquear Console (Tela de Senha) |

---

## Aviso Legal e Compatibilidade (MIT)

Este projeto é uma reimplementação **independente e não oficial** em código aberto (licença **MIT**) desenvolvida com foco em **interoperabilidade** para proprietários do hardware Duonn Axios 24.  
- Nenhum binário proprietário, código-fonte original ou marca registrada da fabricante é redistribuído neste repositório.  
- O emulador de hardware virtual (`axios24_emulator`) é uma implementação original "clean-room" em Rust baseada em comunicação de rede (UDP/TCP), atuando estritamente em conformidade com as normas de interoperabilidade e termos do GitHub.  
- "Duonn" e "Axios 24" são marcas registradas de seus respectivos proprietários, citadas apenas para fins de compatibilidade de hardware.
