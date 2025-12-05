use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use serde::Deserialize;
use std::{env, fs, io};

// =============================================================================
// Data Model (mirrors stable_mir_json::explore)
// Some fields are unused but needed for deserialization.
// =============================================================================

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExplorerData {
    name: String,
    functions: Vec<ExplorerFunction>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExplorerFunction {
    name: String,
    short_name: String,
    blocks: Vec<ExplorerBlock>,
    locals: Vec<ExplorerLocal>,
    entry_block: usize,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExplorerBlock {
    id: usize,
    statements: Vec<ExplorerStmt>,
    terminator: ExplorerTerminator,
    predecessors: Vec<usize>,
    role: BlockRole,
    summary: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExplorerStmt {
    source: String,
    mir: String,
    annotation: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExplorerTerminator {
    source: String,
    kind: String,
    mir: String,
    annotation: String,
    edges: Vec<ExplorerEdge>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExplorerEdge {
    target: usize,
    label: String,
    kind: EdgeKind,
    annotation: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum EdgeKind {
    Normal,
    Cleanup,
    Otherwise,
    Branch,
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
enum BlockRole {
    Entry,
    Exit,
    BranchPoint,
    MergePoint,
    Linear,
    Cleanup,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExplorerLocal {
    name: String,
    ty: String,
    source_name: Option<String>,
    assignments: Vec<ExplorerAssignment>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExplorerAssignment {
    block_id: usize,
    value: String,
}

// =============================================================================
// App State
// =============================================================================

#[derive(PartialEq, Clone, Copy)]
enum Focus {
    Functions,
    Graph,
}

struct App {
    data: ExplorerData,
    current_fn: usize,
    current_block: usize,
    selected_edge: usize,
    path: Vec<usize>,
    focus: Focus,
    fn_list_state: ListState,
    should_quit: bool,
}

impl App {
    fn new(data: ExplorerData) -> Self {
        let mut fn_list_state = ListState::default();
        fn_list_state.select(Some(0));

        let entry = data.functions.first().map(|f| f.entry_block).unwrap_or(0);

        Self {
            data,
            current_fn: 0,
            current_block: entry,
            selected_edge: 0,
            path: vec![entry],
            focus: Focus::Graph,
            fn_list_state,
            should_quit: false,
        }
    }

    fn current_function(&self) -> Option<&ExplorerFunction> {
        self.data.functions.get(self.current_fn)
    }

    fn current_block_data(&self) -> Option<&ExplorerBlock> {
        self.current_function()
            .and_then(|f| f.blocks.get(self.current_block))
    }

    fn edges(&self) -> &[ExplorerEdge] {
        self.current_block_data()
            .map(|b| b.terminator.edges.as_slice())
            .unwrap_or(&[])
    }

    fn go_back(&mut self) {
        if self.path.len() > 1 {
            self.path.pop();
            self.current_block = *self.path.last().unwrap();
            self.selected_edge = 0;
        }
    }

    fn follow_edge(&mut self) {
        let edges = self.edges();
        if let Some(edge) = edges.get(self.selected_edge) {
            let target = edge.target;
            self.current_block = target;
            self.path.push(target);
            self.selected_edge = 0;
        }
    }

    fn select_next_edge(&mut self) {
        let len = self.edges().len();
        if len > 0 {
            self.selected_edge = (self.selected_edge + 1) % len;
        }
    }

    fn select_prev_edge(&mut self) {
        let len = self.edges().len();
        if len > 0 {
            self.selected_edge = self.selected_edge.checked_sub(1).unwrap_or(len - 1);
        }
    }

    fn jump_to_edge(&mut self, n: usize) {
        if n < self.edges().len() {
            self.selected_edge = n;
        }
    }

    fn go_to_entry(&mut self) {
        let entry = self
            .data
            .functions
            .get(self.current_fn)
            .map(|f| f.entry_block)
            .unwrap_or(0);
        self.current_block = entry;
        self.path = vec![entry];
        self.selected_edge = 0;
    }

    fn select_next_fn(&mut self) {
        let len = self.data.functions.len();
        if len > 0 {
            self.current_fn = (self.current_fn + 1) % len;
            self.fn_list_state.select(Some(self.current_fn));
            self.go_to_entry();
        }
    }

    fn select_prev_fn(&mut self) {
        let len = self.data.functions.len();
        if len > 0 {
            self.current_fn = self.current_fn.checked_sub(1).unwrap_or(len - 1);
            self.fn_list_state.select(Some(self.current_fn));
            self.go_to_entry();
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Functions => Focus::Graph,
            Focus::Graph => Focus::Functions,
        };
    }
}

// =============================================================================
// UI Rendering
// =============================================================================

fn ui(frame: &mut Frame, app: &mut App) {
    // Main layout: left sidebar (30%) | right content (70%)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(frame.area());

    // Left sidebar: functions (60%) | locals (40%)
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(main_chunks[0]);

    // Right side: block content (70%) | navigation (30%)
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(main_chunks[1]);

    render_functions(frame, app, left_chunks[0]);
    render_locals(frame, app, left_chunks[1]);
    render_block(frame, app, right_chunks[0]);
    render_navigation(frame, app, right_chunks[1]);
}

fn render_functions(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .data
        .functions
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let style = if i == app.current_fn {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(f.short_name.clone()).style(style)
        })
        .collect();

    let border_style = if app.focus == Focus::Functions {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(format!(" Functions ({}) ", app.data.functions.len())),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    frame.render_stateful_widget(list, area, &mut app.fn_list_state);
}

fn render_locals(frame: &mut Frame, app: &App, area: Rect) {
    let locals = app
        .current_function()
        .map(|f| &f.locals[..])
        .unwrap_or(&[]);

    let items: Vec<ListItem> = locals
        .iter()
        .map(|l| {
            let name = if let Some(ref src) = l.source_name {
                format!("{} ({})", l.name, src)
            } else {
                l.name.clone()
            };
            ListItem::new(format!("{}: {}", name, l.ty))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Locals "),
    );

    frame.render_widget(list, area);
}

fn render_block(frame: &mut Frame, app: &App, area: Rect) {
    let block_data = app.current_block_data();

    let title = if let Some(b) = block_data {
        format!(" bb{} ({:?}) ", b.id, b.role)
    } else {
        " Block ".to_string()
    };

    let border_color = block_data
        .map(|b| role_color(b.role))
        .unwrap_or(Color::White);

    let mut lines: Vec<Line> = Vec::new();

    if let Some(b) = block_data {
        // Summary
        if !b.summary.is_empty() {
            lines.push(Line::from(Span::styled(
                &b.summary,
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )));
            lines.push(Line::from(""));
        }

        // Statements
        for stmt in &b.statements {
            // Source line (if available)
            if !stmt.source.is_empty() {
                lines.push(Line::from(Span::styled(
                    &stmt.source,
                    Style::default().fg(Color::Cyan),
                )));
            }
            // MIR
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(&stmt.mir, Style::default().fg(Color::White)),
            ]));
            // Annotation
            if !stmt.annotation.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("    // {}", stmt.annotation),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            lines.push(Line::from(""));
        }

        // Terminator
        // Source line (if available)
        if !b.terminator.source.is_empty() {
            lines.push(Line::from(Span::styled(
                &b.terminator.source,
                Style::default().fg(Color::Cyan),
            )));
        }
        // MIR
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                &b.terminator.mir,
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
        ]));
        // Annotation
        if !b.terminator.annotation.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("    // {}", b.terminator.annotation),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(title),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn render_navigation(frame: &mut Frame, app: &App, area: Rect) {
    // Split into edges (top) and path+help (bottom)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(area);

    // Edges
    let edges = app.edges();
    let items: Vec<ListItem> = edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let marker = if i == app.selected_edge { ">" } else { " " };
            let kind_indicator = match e.kind {
                EdgeKind::Cleanup => " [cleanup]",
                EdgeKind::Otherwise => " [else]",
                _ => "",
            };
            let style = if i == app.selected_edge {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if e.kind == EdgeKind::Cleanup {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };
            ListItem::new(format!(
                "{} [{}] {} → bb{}{}",
                marker,
                i + 1,
                e.label,
                e.target,
                kind_indicator
            ))
            .style(style)
        })
        .collect();

    let border_style = if app.focus == Focus::Graph {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let edges_list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Edges "),
    );

    frame.render_widget(edges_list, chunks[0]);

    // Path and help
    let path_str: String = app
        .path
        .iter()
        .map(|b| format!("bb{}", b))
        .collect::<Vec<_>>()
        .join(" → ");

    let help = " [h/←]back [j/k]select [l/→]follow [g]entry [Tab]focus [q]quit";

    let info = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("Path: {}", path_str),
            Style::default().fg(Color::Green),
        )),
        Line::from(Span::styled(help, Style::default().fg(Color::DarkGray))),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Navigation "));

    frame.render_widget(info, chunks[1]);
}

fn role_color(role: BlockRole) -> Color {
    match role {
        BlockRole::Entry => Color::Green,
        BlockRole::Exit => Color::Red,
        BlockRole::BranchPoint => Color::Yellow,
        BlockRole::MergePoint => Color::Cyan,
        BlockRole::Linear => Color::White,
        BlockRole::Cleanup => Color::Magenta,
    }
}

// =============================================================================
// Input Handling
// =============================================================================

fn handle_input(app: &mut App, event: Event) {
    if let Event::Key(key) = event {
        match app.focus {
            Focus::Graph => handle_graph_input(app, key.code, key.modifiers),
            Focus::Functions => handle_fn_input(app, key.code),
        }
    }
}

fn handle_graph_input(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    match code {
        // Quit
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => app.should_quit = true,

        // Navigation - back
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => app.go_back(),

        // Navigation - select edge
        KeyCode::Char('j') | KeyCode::Down => app.select_next_edge(),
        KeyCode::Char('k') | KeyCode::Up => app.select_prev_edge(),

        // Navigation - follow edge
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => app.follow_edge(),

        // Jump to edge by number
        KeyCode::Char(c @ '1'..='9') => {
            if let Some(n) = c.to_digit(10) {
                app.jump_to_edge((n - 1) as usize);
            }
        }

        // Go to entry
        KeyCode::Char('g') => app.go_to_entry(),

        // Toggle focus
        KeyCode::Tab => app.toggle_focus(),

        _ => {}
    }
}

fn handle_fn_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.select_next_fn(),
        KeyCode::Char('k') | KeyCode::Up => app.select_prev_fn(),
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            app.focus = Focus::Graph;
        }
        KeyCode::Tab => app.toggle_focus(),
        _ => {}
    }
}

// =============================================================================
// Main
// =============================================================================

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: mir-tui <file.explore.json>");
        eprintln!();
        eprintln!("Generate the JSON with:");
        eprintln!("  cargo run -- --explore-json -Zno-codegen <file.rs>");
        std::process::exit(1);
    }

    let path = &args[1];
    let content = fs::read_to_string(path).expect("Failed to read file");
    let data: ExplorerData = serde_json::from_str(&content).expect("Failed to parse JSON");

    if data.functions.is_empty() {
        eprintln!("No functions found in {}", path);
        std::process::exit(1);
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(data);

    // Main loop
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            let evt = event::read()?;
            handle_input(&mut app, evt);
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
