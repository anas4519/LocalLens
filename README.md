# LocalLens

Private, on-device memory for the sessions **you** choose to record.

Hit **Start** before a learning or coding session. LocalLens captures your screen, and afterwards you can ask *"where did I see that Spring retry config?"* and get an answer with citations that jump to the exact frame and timestamp.

## Principles

- **User-started sessions only.** No always-on capture.
- **Ask and jump to the moment.** Answers cite the frames they came from; click to open that moment on the session timeline.
- **Text stays, pictures expire.** OCR text (and later transcripts and embeddings) is kept forever; screenshots are pruned by age/size.
- **Fully local.** No telemetry, no accounts, no network calls. Data is a plain SQLite file in your app data folder.

## Architecture

```
React + TS (Vite)  ── Tauri IPC ──  Rust core (src-tauri)
  Start/Stop, timer,                 commands: start_session / stop_session / list_sessions
  session list                       db.rs: SQLite (sessions, frames, frames_fts FTS5)
                                     capture/: ScreenCapturer + OcrEngine traits
                                       ├─ macos.rs   ScreenCaptureKit + Apple Vision (TODO)
                                       └─ windows.rs Graphics.Capture + Media.Ocr (later)
```

Pipeline (planned): capture → skip near-duplicate frames → OCR → store timestamped text → FTS5 + local embeddings → local LLM answers with citations → jump-to-moment.

Storage lives at the Tauri app data dir, e.g. `~/Library/Application Support/dev.anas4519.locallens/locallens.db`.

## Roadmap

1. Capture (ScreenCaptureKit, change-detection) — *scaffold done, capture TODO*
2. OCR (Apple Vision) + full-text search
3. Playback: results open exact frame; timeline scrubber
4. Semantic search + cited Q&A (local embeddings + local LLM)
5. Optional audio (local Whisper)

## Run on macOS

Prerequisites:
- Xcode Command Line Tools: `xcode-select --install`
- Rust (stable, 1.88+): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Node.js 20+ (`brew install node`)

```bash
npm install
npm run tauri dev
```

Rust tests: `cd src-tauri && cargo test`

Build a release app: `npm run tauri build`

## License

MIT © 2026 Anas Nadeem
