use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
    Frame, Terminal,
};
use std::{
    fs,
    io::{self, stdout},
    path::PathBuf,
    process::Command,
    time::SystemTime,
};

const SCAN_DIRS: &[&str] = &["Documents/app", "Documents/playground"];
const OBSIDIAN_DOCS: &str = "Library/Mobile Documents/iCloud~md~obsidian/Documents/NV/Personal/App";

struct App {
    projects: Vec<Project>,
    list_state: ListState,
    searching: bool,
    filter: String,
    quit: bool,
}

struct Project {
    name: String,
    path: PathBuf,
    source: String,
    modified: Option<SystemTime>,
    has_doc: bool,
    git_branch: Option<String>,
    git_dirty: bool,
    config_labels: Vec<String>,
}

/// Normalize a name for fuzzy matching: lowercase, strip hyphens/spaces/underscores
fn normalize(name: &str) -> String {
    name.to_lowercase()
        .replace(['-', '_', ' '], "")
}

/// Find the matching Obsidian doc for a project by fuzzy name matching.
/// e.g. project "daily-digest" matches doc "Daily Digest.md"
fn find_obsidian_doc(project_name: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let docs_dir = home.join(OBSIDIAN_DOCS);
    let normalized_project = normalize(project_name);

    let entries = fs::read_dir(&docs_dir).ok()?;
    for entry in entries.flatten() {
        let filename = entry.file_name().to_string_lossy().to_string();
        if let Some(stem) = filename.strip_suffix(".md") {
            if normalize(stem) == normalized_project {
                return Some(entry.path());
            }
        }
    }
    None
}

fn format_relative_time(time: Option<SystemTime>) -> String {
    let Some(t) = time else { return "—".to_string() };
    let Ok(elapsed) = t.elapsed() else { return "—".to_string() };
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        86400..=2591999 => format!("{}d ago", secs / 86400),
        2592000..=31535999 => format!("{}mo ago", secs / 2592000),
        _ => format!("{}y ago", secs / 31536000),
    }
}

fn scan_projects() -> Vec<Project> {
    let home = dirs::home_dir().expect("Cannot find home directory");
    let mut projects = Vec::new();

    for dir in SCAN_DIRS {
        let full_path = home.join(dir);
        let source = dir.rsplit('/').next().unwrap_or(dir);

        if let Ok(entries) = fs::read_dir(&full_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with('.') && name != "claude-tui" {
                        let has_doc = find_obsidian_doc(&name).is_some();
                        let is_git = path.join(".git").exists();

                        // Git info
                        let (git_branch, git_dirty) = if is_git {
                            let branch = Command::new("git")
                                .args(["-C", &path.to_string_lossy(), "branch", "--show-current"])
                                .output()
                                .ok()
                                .and_then(|o| {
                                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                                    if s.is_empty() { None } else { Some(s) }
                                });
                            let dirty = Command::new("git")
                                .args(["-C", &path.to_string_lossy(), "status", "--porcelain"])
                                .output()
                                .map(|o| !o.stdout.is_empty())
                                .unwrap_or(false);
                            (branch, dirty)
                        } else {
                            (None, false)
                        };

                        // Modified time: git log for repos, smart mtime for non-git
                        let modified = if is_git {
                            Command::new("git")
                                .args(["-C", &path.to_string_lossy(), "log", "-1", "--format=%ct"])
                                .output()
                                .ok()
                                .and_then(|o| {
                                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                                    s.parse::<u64>().ok()
                                })
                                .map(|ts| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(ts))
                        } else {
                            // Scan direct children, skip .DS_Store and hidden files
                            fs::read_dir(&path).ok().and_then(|entries| {
                                entries.flatten()
                                    .filter(|e| {
                                        let name = e.file_name().to_string_lossy().to_string();
                                        !name.starts_with('.') && name != ".DS_Store"
                                    })
                                    .filter_map(|e| e.metadata().ok()?.modified().ok())
                                    .max()
                            })
                        };

                        // Claude config labels
                        let mut config_labels = Vec::new();
                        if path.join("CLAUDE.md").exists() {
                            config_labels.push("claude.md".to_string());
                        }
                        let skill_count = path.join(".claude/commands").read_dir()
                            .map(|d| d.flatten().count())
                            .unwrap_or(0);
                        if skill_count > 0 {
                            config_labels.push(format!("{}skills", skill_count));
                        }
                        if path.join(".mcp.json").exists() {
                            let mcp_count = fs::read_to_string(path.join(".mcp.json"))
                                .ok()
                                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                                .and_then(|v| v.get("mcpServers").and_then(|m| m.as_object()).map(|o| o.len()))
                                .unwrap_or(1);
                            config_labels.push(format!("{}mcp", mcp_count));
                        }

                        projects.push(Project {
                            name,
                            path,
                            source: source.to_string(),
                            modified,
                            has_doc,
                            git_branch,
                            git_dirty,
                            config_labels,
                        });
                    }
                }
            }
        }
    }

    // Sort by most recently modified first
    projects.sort_by(|a, b| b.modified.cmp(&a.modified));
    projects
}

impl App {
    fn new() -> Self {
        let projects = scan_projects();
        let mut list_state = ListState::default();
        if !projects.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            projects,
            list_state,
            searching: false,
            filter: String::new(),
            quit: false,
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let query = self.filter.to_lowercase();
        self.projects
            .iter()
            .enumerate()
            .filter(|(_, p)| query.is_empty() || p.name.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect()
    }

    fn move_selection(&mut self, delta: i32) {
        let filtered = self.filtered_indices();
        if filtered.is_empty() {
            self.list_state.select(None);
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let new = if delta > 0 {
            (current + 1).min(filtered.len() - 1)
        } else {
            current.saturating_sub(1)
        };
        self.list_state.select(Some(new));
    }

    fn selected_project(&self) -> Option<&Project> {
        let filtered = self.filtered_indices();
        let selected = self.list_state.selected()?;
        let index = *filtered.get(selected)?;
        self.projects.get(index)
    }

    fn launch_claude(&self) {
        if let Some(project) = self.selected_project() {
            disable_raw_mode().ok();
            stdout().execute(LeaveAlternateScreen).ok();

            let status = Command::new("claude")
                .current_dir(&project.path)
                .arg("--continue")
                .status();

            match status {
                Ok(s) if s.success() => {}
                Ok(s) => eprintln!("Claude exited with: {}", s),
                Err(e) => eprintln!("Failed to launch claude: {}", e),
            }

            stdout().execute(EnterAlternateScreen).ok();
            enable_raw_mode().ok();
        }
    }

    fn open_finder(&self) {
        if let Some(project) = self.selected_project() {
            Command::new("open").arg(&project.path).spawn().ok();
        }
    }

    fn open_doc(&self) {
        if let Some(project) = self.selected_project() {
            if let Some(doc_path) = find_obsidian_doc(&project.name) {
                // Get the filename without .md extension for the Obsidian URI
                let file_stem = doc_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                let uri = format!(
                    "obsidian://open?vault=NV&file=Personal%2FApp%2F{}",
                    file_stem.replace(' ', "%20")
                );
                Command::new("open").arg(uri).spawn().ok();
            }
        }
    }
}

fn draw(frame: &mut Frame, app: &App) {
    let [header_area, main_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .areas(frame.area());

    // Header
    let title = if app.searching {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(Color::Yellow).bold()),
            Span::styled(&app.filter, Style::default().fg(Color::White)),
            Span::styled("▌", Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" claude-tui ", Style::default().fg(Color::Cyan).bold()),
            Span::styled(
                format!(" {} projects", app.filtered_indices().len()),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };

    let header = Paragraph::new(title).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(header, header_area);

    // Project list
    let filtered = app.filtered_indices();
    let list_width = main_area.width as usize;
    let items: Vec<ListItem> = filtered
        .iter()
        .map(|&i| {
            let p = &app.projects[i];
            let time_str = format_relative_time(p.modified);

            // Build the left-side content to measure its width
            let source_col = format!(" {:>10} ", p.source);
            let mut left_len = source_col.len() + p.name.len();

            let branch_str = match (&p.git_branch, p.git_dirty) {
                (Some(b), true) => { let s = format!("  {}*", b); left_len += s.len(); Some(s) }
                (Some(b), false) => { let s = format!("  {}", b); left_len += s.len(); Some(s) }
                _ => None,
            };

            let config_str = if !p.config_labels.is_empty() {
                let s = format!("  {}", p.config_labels.join(" "));
                left_len += s.len();
                Some(s)
            } else {
                None
            };

            if p.has_doc { left_len += 4; } // " doc"

            let padding = list_width.saturating_sub(left_len + time_str.len() + 6);

            let mut spans = vec![
                Span::styled(source_col, Style::default().fg(Color::DarkGray)),
                Span::styled(&p.name, Style::default().fg(Color::White)),
            ];

            if let Some(ref b) = branch_str {
                spans.push(Span::styled(b.clone(), Style::default().fg(Color::Magenta)));
            }

            if let Some(ref c) = config_str {
                spans.push(Span::styled(c.clone(), Style::default().fg(Color::DarkGray)));
            }

            if p.has_doc {
                spans.push(Span::styled(" doc", Style::default().fg(Color::Green)));
            }

            spans.push(Span::raw(" ".repeat(padding)));
            spans.push(Span::styled(time_str, Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw("  "));
            let line = Line::from(spans);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().padding(Padding::new(1, 1, 1, 0)))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, main_area, &mut app.list_state.clone());

    // Footer
    let help = if app.searching {
        Line::from(vec![
            Span::styled(" esc ", Style::default().fg(Color::Cyan)),
            Span::styled("clear  ", Style::default().fg(Color::DarkGray)),
            Span::styled("enter ", Style::default().fg(Color::Cyan)),
            Span::styled("open  ", Style::default().fg(Color::DarkGray)),
            Span::styled("↑↓ ", Style::default().fg(Color::Cyan)),
            Span::styled("navigate", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ↑↓/jk ", Style::default().fg(Color::Cyan)),
            Span::styled("navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("enter ", Style::default().fg(Color::Cyan)),
            Span::styled("open claude  ", Style::default().fg(Color::DarkGray)),
            Span::styled("f ", Style::default().fg(Color::Cyan)),
            Span::styled("finder  ", Style::default().fg(Color::DarkGray)),
            Span::styled("d ", Style::default().fg(Color::Cyan)),
            Span::styled("docs  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/ ", Style::default().fg(Color::Cyan)),
            Span::styled("search  ", Style::default().fg(Color::DarkGray)),
            Span::styled("q ", Style::default().fg(Color::Cyan)),
            Span::styled("quit", Style::default().fg(Color::DarkGray)),
        ])
    };

    let footer = Paragraph::new(help).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(footer, footer_area);
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new();

    while !app.quit {
        terminal.draw(|frame| draw(frame, &app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            if app.searching {
                match key.code {
                    KeyCode::Esc => {
                        app.searching = false;
                        app.filter.clear();
                        app.list_state.select(Some(0));
                    }
                    KeyCode::Backspace => {
                        app.filter.pop();
                        if app.filter.is_empty() {
                            app.searching = false;
                        }
                        app.list_state.select(Some(0));
                    }
                    KeyCode::Enter => app.launch_claude(),
                    KeyCode::Up => app.move_selection(-1),
                    KeyCode::Down => app.move_selection(1),
                    KeyCode::Char(c) => {
                        app.filter.push(c);
                        app.list_state.select(Some(0));
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Char('q') => app.quit = true,
                    KeyCode::Char('f') => app.open_finder(),
                    KeyCode::Char('d') => app.open_doc(),
                    KeyCode::Char('/') => app.searching = true,
                    KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                    KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                    KeyCode::Enter => app.launch_claude(),
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
