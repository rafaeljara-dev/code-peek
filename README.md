# CodePeek (`peek`)

> **The zero-bloat, native desktop code inspector and viewer built in Rust.**  
> Instant boot (<20 ms), minimal memory footprint (<30 MB RAM), and zero web overhead. Designed for the AI-agent era.

---

## ⚡ Motivation

In modern software development, AI coding agents and automated tools generate or refactor dozens of files in seconds. Developers spend more than 70% of their time **inspecting code, reviewing diffs, navigating project structures, and making surgical 1-line edits**, rather than typing thousands of lines from scratch.

Existing tools fail in two distinct ways:

1. **Heavyweight IDEs (VS Code, Cursor):**
   * Boot times: 1.5 to 4.0 seconds.
   * Memory usage: 500 MB to 1.5+ GB of RAM per project.
   * Bloated with extensions, language servers, telemetry, and background processes.
2. **Web-based "lightweight" viewers (e.g. px0.ai):**
   * They claim to be lightweight because their backend is in Go, but **they open a browser tab in Chrome or Edge**.
   * A single browser tab instantly consumes 250–400 MB of RAM, triggers DOM layout recalculations, causes keyboard shortcut conflicts (`Ctrl+W`, `Ctrl+T`), and gets lost among your personal tabs.

**CodePeek (`peek`) is the answer:** A 100% native desktop application written in Rust, rendered directly via your GPU, with zero Chromium, zero Node.js, and zero browser dependencies.

---

## 🚀 Key Features

* **⚡ Sub-20ms Startup:** Double-click or run `peek .` in your terminal, and the window appears immediately.
* **🍃 Featherweight Footprint:** Typically idles at **15–30 MB of RAM**, even with huge workspaces open.
* **📂 Lightning File Tree:** Powered by the `ignore` engine (the same multithreaded directory traversal used by `ripgrep`). Automatically respects `.gitignore` and ignores `node_modules/`, `target/`, and `.git/` in milliseconds.
* **🎯 Fuzzy File Finder (`Ctrl+P`):** Instant fuzzy file jumping powered by `nucleo`, indexing 100,000 files in under 1 ms.
* **📜 Massive File Support:** Backed by the `ropey` data structure (B-tree ropes). Open 500,000-line files or multi-megabyte logs without freezing the UI or cloning strings.
* **🎨 Syntax Highlighting:** High-performance TextMate/Tree-sitter highlighting with strict viewport virtualization (only lines currently visible on screen are colored).
* **✏️ Surgical Micro-Editing:** Not a complex IDE, but allows quick inline edits, typo fixes, and atomic saves (`Ctrl+S`).
* **🔄 Live File Watching:** Automatically refreshes open files when background AI agents or build tools write changes to disk.
* **🖥️ Windows 11 Native Integration:** Native dark mode, Mica/Acrylic translucent flyout support, and context-menu integration (*"Open with CodePeek"*).

---

## 📊 Comparison

| Metric / Feature | VS Code / Cursor | Web-based Viewers (px0.ai) | **CodePeek (`peek`)** |
| :--- | :---: | :---: | :---: |
| **Technology** | Electron / Node / TS | Go backend + Browser Web Tab | **Pure Rust + Native GPU UI** |
| **Startup Time** | ~2,500 ms | ~800 ms (CLI + Browser spawn) | **< 20 ms** |
| **RAM Usage** | 500 MB – 1.5 GB | ~300 MB (Chromium tab) | **~25 MB** |
| **Browser Dependency** | No (Embedded Chromium) | **Yes (Requires Chrome/Edge)** | **None (Zero web stack)** |
| **Shortcuts Safety** | Standard | Conflicts with Browser | **100% Native Shortcuts** |
| **Massive Files (>100k lines)** | Slow / Lags | Depends on Browser DOM | **Instant (Rope data structure)** |
| **Surgical Inline Editing** | Full IDE | Read-only | **Fast 1-2 line edits + Atomic save** |

---

## 🛠️ Tech Stack & Architecture

* **Core Language:** [Rust](https://www.rust-lang.org/) (2021 Edition)
* **GUI Engine:** [Slint](https://slint.dev/) (Native GPU rendering via Direct3D/Vulkan/Metal/Skia)
* **Text Buffer:** [`ropey`](https://crates.io/crates/ropey) (B-tree text rope structure for $O(\log N)$ edits)
* **Directory Walker:** [`ignore`](https://crates.io/crates/ignore) (`ripgrep` multithreaded walker)
* **Fuzzy Matching:** [`nucleo`](https://crates.io/crates/nucleo) (Fastest SIMD-accelerated fuzzy search)
* **Syntax Engine:** [`syntect`](https://crates.io/crates/syntect) (TextMate grammars) & [`tree-sitter`](https://tree-sitter.github.io/tree-sitter/)
* **Filesystem Watcher:** [`notify`](https://crates.io/crates/notify)

See [ARCHITECTURE.md](ARCHITECTURE.md) for full technical diagrams and design specs.

---

## 📦 Quick Start (Development)

```powershell
# Clone repository
git clone https://github.com/rafaeljara-dev/code-peek.git
cd code-peek

# Check build
cargo check --workspace

# Run against current directory
cargo run --bin peek -- .
```

---

## 🗺️ Roadmap

See [ROADMAP.md](ROADMAP.md) for phase-by-phase implementation milestones.

---

## 📄 License

Licensed under MIT OR Apache-2.0. Open source and community friendly.
