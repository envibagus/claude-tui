# claude-tui

A minimal TUI project picker for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) sessions. Scan your project directories, see git status and Claude config at a glance, and launch straight into a session.

```
 claude-tui  12 projects
─────────────────────────────────────────────────────
         app  my-api  main*  claude.md 2skills       3h ago
         app  webapp  main   1mcp                     1d ago
  playground  sketch                                   5d ago
─────────────────────────────────────────────────────
 ↑↓/jk navigate  enter open claude  f finder  d docs  / search  q quit
```

## Features

- Scans configurable project directories and lists them sorted by last modified
- Git branch and dirty status per project (uses last commit time, not filesystem mtime)
- Claude Code config detection — shows which projects have `CLAUDE.md`, custom skills (`.claude/commands/`), or MCP servers (`.mcp.json`)
- Fuzzy search with `/`
- Launch `claude --continue` directly into any project
- Open project folder in Finder with `f`
- Open matched Obsidian doc with `d` (optional, requires config)

## Install

Requires Rust 1.85+.

```sh
git clone https://github.com/envibagus/claude-tui.git
cd claude-tui
cargo build --release
cp target/release/claude-tui ~/.local/bin/
```

## Config

Create `~/.config/claude-tui/config.toml`:

```toml
# Directories to scan for projects (relative to home)
scan_dirs = ["Projects", "Code/experiments"]

# Project folders to skip
exclude = ["node_modules", "archive"]

# Optional: Obsidian integration for project docs
[obsidian]
docs_path = "path/to/vault/folder"  # relative to home
vault = "MyVault"
file_prefix = "Projects"            # subfolder inside vault
```

Without a config file, it defaults to scanning `~/Documents/app` and `~/Documents/playground`.

## Keybindings

| Key | Action |
|---|---|
| `j` / `k` / `↑` / `↓` | Navigate |
| `Enter` | Open Claude Code session (`claude --continue`) |
| `f` | Open in Finder |
| `d` | Open matching Obsidian doc |
| `/` | Search / filter |
| `Esc` | Clear search |
| `q` | Quit |

## How it works

**Modified time** — For git repos, uses `git log -1 --format=%ct` (last commit time). For non-git projects, scans direct children excluding `.DS_Store` and hidden files.

**Config labels** — Detects Claude Code configuration per project:
- `claude.md` — project has a `CLAUDE.md` file
- `Nskills` — number of files in `.claude/commands/`
- `Nmcp` — number of servers in `.mcp.json`

**Obsidian docs** — Fuzzy-matches project names to markdown files in your configured Obsidian folder (e.g. project `my-app` matches `My App.md`).

## License

MIT
