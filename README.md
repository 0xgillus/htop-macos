# htop-macos

A modern, interactive, htop-style system monitor for macOS in the terminal.

## Features

- Live, sortable, scrollable process table
- Per-core CPU and memory usage bars (auto-detects your Mac's CPU count)
- Tree view, search/filter, process kill, color themes, and more
- Works on Apple Silicon and Intel Macs

## Install

### With Cargo (recommended)

```
cargo install --git https://github.com/0xgillus/htop-macos
```

Or, after publishing to crates.io:

```
cargo install htop-macos
```

## Usage

```
htop-macos
```

## Requirements

- Rust (install with `brew install rust` or from [rustup.rs](https://rustup.rs))
- macOS 11+ (Apple Silicon and Intel supported)

## Notes

- The number of CPU bars matches your Mac’s CPU core count.
- For best results, run in a large terminal window.
- Use arrow keys to scroll, `/` to search, F5 for tree view, F9 to kill, F10 or q to quit.

## License

MIT 
