# Koe (声) — Project Reference

## What
Cross-platform background voice input tool. Hold hotkey → speak → ASR streaming → LLM correction → auto-paste into active app. Supports macOS and Windows 11.

## Architecture

```
┌─ macOS: Objective-C (KoeApp/Koe/) ──────────────┐
│  Hotkey · Audio · Clipboard · Paste · Overlay    │
│  StatusBar · History(SQLite) · Permissions        │
│                    │ C FFI                        │
├────────────────────┼─────────────────────────────┤
│  Rust (koe-core + koe-asr)                       │
│  Session State Machine · ASR WebSocket            │
│  LLM HTTP · Config · Dictionary · Prompts        │
├────────────────────┼─────────────────────────────┤
│  Windows: Rust (koe-win)                         │
│  WH_KEYBOARD_LL · WASAPI · SendInput · Overlay   │
│  Shell_NotifyIcon (system tray)                  │
│         Direct Rust library call (no FFI)         │
└──────────────────────────────────────────────────┘
```

## Tech Stack

| Layer | Tech | Purpose |
|-------|------|---------|
| macOS Shell | Obj-C + AppKit | System integration (hotkey, audio, clipboard, UI) |
| Windows Shell | Rust + windows-rs | Win32 API (WASAPI, keyboard hooks, tray, overlay) |
| Core | Rust + Tokio | Async business logic (ASR, LLM, state machine) |
| ASR | Doubao / Qwen | WebSocket streaming speech recognition |
| LLM | OpenAI-compatible | Text correction (capitalization, punctuation, terminology) |
| macOS IPC | C FFI (cbindgen) | ObjC ↔ Rust via function pointers + GCD |
| Windows IPC | Direct Rust call | koe-win calls koe-core as library, callbacks via PostMessageW |
| Config | YAML + txt | Config files, hot-reloaded per session |
| DB | SQLite | Usage statistics (macOS only) |

## Data Flow
```
Hotkey → Audio Capture (16kHz PCM) → ASR WebSocket (streaming)
  → TranscriptAggregator (interim → definite → final)
  → LLM Correction (OpenAI-compatible API)
  → Clipboard Write → Simulate Cmd+V → Restore Clipboard
```

## Session State Machine
```
Idle → HotkeyDecisionPending → ConnectingAsr
  → RecordingHold / RecordingToggle → FinalizingAsr
  → Correcting → PreparingPaste → Pasting
  → RestoringClipboard → Completed / Failed
```

## Directory Layout

| Path | Contents |
|------|----------|
| `KoeApp/Koe/` | 16 ObjC components (macOS): AppDelegate, Bridge, Audio, Hotkey, Clipboard, Paste, Overlay, StatusBar, History, Permissions, Accessibility, Feedback, Update, SetupWizard |
| `koe-win/src/` | Windows shell (Rust): main.rs, bridge.rs, hotkey.rs, audio.rs, clipboard.rs, paste.rs, overlay.rs, tray.rs |
| `koe-core/src/` | Rust core: lib.rs (FFI entry), session.rs (state machine), config.rs (#[cfg] for Win/Mac), ffi.rs, llm/, prompt.rs, dictionary.rs |
| `koe-asr/src/` | ASR providers: provider.rs (trait), doubao.rs, qwen.rs, transcript.rs (aggregator), event.rs |
| `docs/` | Update feed JSON for in-app updates |
| `skills/koe-setup/` | Claude Code interactive setup wizard |

## Build

**macOS:**
```bash
make build          # Full: xcodegen → cargo build (staticlib) → xcode build
make build-rust     # Rust only → libkoe_core.a + koe_core.h (cbindgen)
make build-xcode    # Xcode only
make build-x86_64   # Intel Mac target
make run            # Launch built app
```

**Windows:**
```bash
cargo build --release -p koe-win    # → target/release/koe.exe
```

- macOS: Rust → staticlib `libkoe_core.a` + cbindgen C header, linked into Xcode
- Windows: koe-win depends on koe-core as Rust library, single `koe.exe` binary

## Runtime Config

- macOS: `~/.koe/`
- Windows: `%LOCALAPPDATA%\koe\`

| File | Purpose |
|------|---------|
| `config.yaml` | ASR provider, LLM endpoint, hotkey, feedback settings |
| `dictionary.txt` | Hotwords for ASR + context for LLM correction |
| `system_prompt.txt` | LLM system prompt (correction rules) |
| `user_prompt.txt` | LLM user prompt template (`{{asr_text}}`, `{{dictionary_entries}}`, `{{interim_history}}`) |
| `history.db` | SQLite: sessions table (timestamp, duration, text, counts) |

## Key Abstractions

- **`AsrProvider` trait** (Rust) — async streaming ASR interface: `connect()`, `send_audio()`, `finish_input()`, `next_event()`
- **`TranscriptAggregator`** — merges interim/definite/final ASR results, tracks revision history
- **`SPRustBridge`** (ObjC) — C FFI wrapper, routes Rust callbacks to main thread via GCD
- **`SPCallbacks`** — 7 function pointers: `on_session_ready`, `on_final_text_ready`, `on_state_changed`, `on_interim_text`, `on_session_error/warning`, `on_log_event`
- **`SPHotkeyMonitor`** — dual-mode hotkey: hold (>=180ms) vs tap (<180ms)

## App Identity
- Bundle ID: `nz.owo.koe`
- LSUIElement: true (no Dock icon, menu bar only)
- Deployment target: macOS 13.0+
- Permissions: Microphone, Accessibility, Input Monitoring
- Version: managed in `KoeApp/project.yml` (`MARKETING_VERSION` + `CURRENT_PROJECT_VERSION`)

## Distribution
- Homebrew: `owo-network/brew/koe`
- GitHub Releases: arm64 + x86_64 zip
- In-app update check via `docs/update-feed.json`

## Tests
```bash
cargo test --manifest-path koe-asr/Cargo.toml    # ASR unit tests
```
Test file: `koe-asr/tests/api_test.rs` (config, provider, transcript aggregator, events)
