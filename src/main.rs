use std::{io, thread, time::Duration, collections::HashMap};
use std::sync::{Arc, Mutex};
use std::process::Command;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Modifier, Color};
// ADDED: List, ListItem, ListState for the Kill Menu
use ratatui::widgets::{Block, Borders, Row, Table, TableState, Gauge, Paragraph, Cell, Clear, List, ListItem, ListState};
use ratatui::Terminal;
use sysinfo::{System, LoadAvg, ProcessStatus, Cpu};
use users::get_user_by_uid;

// Enums: SortOrder, SortBy
#[derive(Clone, Copy)]
enum SortOrder {
    Asc,
    Desc,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    PID,
    User,
    CPU,
    MEM,
    Time,
    Command,
}

// MODIFIED: Added KillMenu mode
#[derive(PartialEq)]
enum InputMode {
    Normal,
    Search,
    KillMenu,
}

// Struct: App - Modified to add tree view and kill menu state
struct App {
    processes: Vec<ProcessInfo>,
    state: TableState,
    sort_by: SortBy,
    sort_order: SortOrder,
    cpus: Vec<f32>,
    mem_usage: f64,
    total_mem: u64,
    used_mem: u64,
    swap_usage: f64,
    total_swap: u64,
    used_swap: u64,
    message: Option<String>,
    uptime: u64,
    load_avg: LoadAvg,
    input_mode: InputMode,
    search_query: String,
    active_filter: Option<String>,
    tree_view: bool, // ADDED
    kill_menu_state: ListState, // ADDED
    kill_signals: Vec<(&'static str, i32)>, // ADDED
}

// Struct: ProcessInfo - No changes
struct ProcessInfo {
    pid: u32,
    ppid: u32,
    user: String,
    status: String,
    cpu: f32,
    mem: f32,
    virtual_mem: u64,
    cpu_time: u64,
    command: String,
}

// impl App - Modified to handle new state and logic
impl App {
    fn new() -> Self {
        // These are common signals. 15 is polite, 9 is forceful.
        let signals = vec![
            (" 1 SIGHUP", 1), (" 2 SIGINT", 2), (" 9 SIGKILL", 9),
            ("15 SIGTERM", 15), ("20 SIGTSTP", 20), ("24 SIGXCPU", 24),
        ];
        let mut kill_menu_state = ListState::default();
        kill_menu_state.select(Some(0)); // Select the first signal by default

        Self {
            processes: Vec::new(),
            state: TableState::default(),
            sort_by: SortBy::CPU,
            sort_order: SortOrder::Desc,
            cpus: Vec::new(),
            mem_usage: 0.0,
            total_mem: 0,
            used_mem: 0,
            swap_usage: 0.0,
            total_swap: 0,
            used_swap: 0,
            message: None,
            uptime: 0,
            load_avg: LoadAvg { one: 0.0, five: 0.0, fifteen: 0.0 },
            input_mode: InputMode::Normal,
            search_query: String::new(),
            active_filter: None,
            tree_view: false,
            kill_menu_state,
            kill_signals: signals,
        }
    }

    fn update_data(&mut self, sys: &mut System) {
        // This method remains the same
        sys.refresh_all();
        sys.refresh_cpu();
        sys.refresh_memory();

        self.uptime = sysinfo::System::uptime();
        self.load_avg = sysinfo::System::load_average();
        self.cpus = sys.cpus().iter().map(Cpu::cpu_usage).collect();
        self.total_mem = sys.total_memory();
        self.used_mem = sys.used_memory();
        self.total_swap = sys.total_swap();
        self.used_swap = sys.used_swap();
        self.mem_usage = if self.total_mem > 0 { (self.used_mem as f64 / self.total_mem as f64) * 100.0 } else { 0.0 };
        self.swap_usage = if self.total_swap > 0 { (self.used_swap as f64 / self.total_swap as f64) * 100.0 } else { 0.0 };

        let num_cpus = self.cpus.len() as f32;
        let mut procs: Vec<ProcessInfo> = sys.processes().values().map(|p| {
            ProcessInfo {
                pid: p.pid().as_u32(),
                ppid: p.parent().map(|pid| pid.as_u32()).unwrap_or(0),
                user: p.user_id().and_then(|uid| get_user_by_uid(**uid)).map(|u| u.name().to_string_lossy().into_owned()).unwrap_or_else(|| "?".to_string()),
                status: status_to_string(p.status()),
                cpu: p.cpu_usage() / num_cpus.max(1.0),
                mem: (p.memory() as f64 / self.total_mem as f64 * 100.0) as f32,
                virtual_mem: p.virtual_memory(),
                cpu_time: p.run_time(),
                command: if !p.cmd().is_empty() { p.cmd().join(" ") } else { p.name().to_string() },
            }
        }).collect();

        procs.sort_by(|a, b| {
            let ordering = match self.sort_by {
                SortBy::PID => a.pid.cmp(&b.pid),
                SortBy::User => a.user.cmp(&b.user),
                SortBy::CPU => a.cpu.partial_cmp(&b.cpu).unwrap(),
                SortBy::MEM => a.mem.partial_cmp(&b.mem).unwrap(),
                SortBy::Time => a.cpu_time.cmp(&b.cpu_time),
                SortBy::Command => a.command.cmp(&b.command),
            };
            match self.sort_order {
                SortOrder::Asc => ordering,
                SortOrder::Desc => ordering.reverse(),
            }
        });
        self.processes = procs;
    }

    // ADDED BACK: Methods for tree view
    fn tree_ordered_processes(&self) -> Vec<(usize, &ProcessInfo)> {
        let mut pid_map: HashMap<u32, Vec<&ProcessInfo>> = HashMap::new();
        let mut root_procs: Vec<&ProcessInfo> = Vec::new();

        // Create a set of all PIDs for quick lookups
        let all_pids: HashMap<u32, ()> = self.processes.iter().map(|p| (p.pid, ())).collect();

        for proc in &self.processes {
            // A process is a root if its parent ID is 0, or if its parent ID does not exist in our list of processes.
            if proc.ppid == 0 || !all_pids.contains_key(&proc.ppid) {
                root_procs.push(proc);
            } else {
                pid_map.entry(proc.ppid).or_default().push(proc);
            }
        }

        root_procs.sort_by_key(|p| p.pid);
        let mut ordered_list = Vec::new();
        for root in root_procs {
            self.add_tree_children(root, 0, &pid_map, &mut ordered_list);
        }
        ordered_list
    }

    fn add_tree_children<'a>(
        &self,
        proc: &'a ProcessInfo,
        depth: usize,
        pid_map: &HashMap<u32, Vec<&'a ProcessInfo>>,
        ordered_list: &mut Vec<(usize, &'a ProcessInfo)>,
    ) {
        ordered_list.push((depth, proc));
        if let Some(children) = pid_map.get(&proc.pid) {
            let mut sorted_children = children.clone();
            sorted_children.sort_by_key(|c| c.pid);
            for child in sorted_children {
                self.add_tree_children(child, depth + 1, pid_map, ordered_list);
            }
        }
    }

    fn filtered_processes(&self) -> Vec<&ProcessInfo> {
        if let Some(ref filter) = self.active_filter {
            let filter_lower = filter.to_lowercase();
            self.processes.iter().filter(|p| p.command.to_lowercase().contains(&filter_lower)).collect()
        } else {
            self.processes.iter().collect()
        }
    }

    fn selected_pid(&self) -> Option<u32> {
        let idx = self.state.selected()?;
        if self.tree_view {
            // In tree view, filtering is tricky. For now, we get from the full list.
            // A more advanced implementation would filter the tree itself.
            let tree_list = self.tree_ordered_processes();
            tree_list.get(idx).map(|(_, p)| p.pid)
        } else {
            self.filtered_processes().get(idx).map(|p| p.pid)
        }
    }

    fn get_list_length(&self) -> usize {
        if self.tree_view {
            self.processes.len() // Tree view shows all processes
        } else {
            self.filtered_processes().len()
        }
    }

    fn next(&mut self) {
        let len = self.get_list_length();
        if len == 0 { return; }
        let i = match self.state.selected() {
            Some(i) => if i >= len - 1 { 0 } else { i + 1 },
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let len = self.get_list_length();
        if len == 0 { return; }
        let i = match self.state.selected() {
            Some(i) => if i == 0 { len - 1 } else { i - 1 },
            None => len - 1,
        };
        self.state.select(Some(i));
    }

    fn page_down(&mut self, page_size: usize) {
        let len = self.get_list_length();
        if len == 0 { return; }
        let i = self.state.selected().unwrap_or(0);
        let new_i = (i + page_size).min(len - 1);
        self.state.select(Some(new_i));
    }

    fn page_up(&mut self, page_size: usize) {
        let len = self.get_list_length();
        if len == 0 { return; }
        let i = self.state.selected().unwrap_or(0);
        let new_i = i.saturating_sub(page_size);
        self.state.select(Some(new_i));
    }

    fn home(&mut self) {
        if self.get_list_length() > 0 { self.state.select(Some(0)); }
    }

    fn end(&mut self) {
        let len = self.get_list_length();
        if len > 0 { self.state.select(Some(len - 1)); }
    }

    fn set_sort_by(&mut self, sort_by: SortBy) {
        if self.sort_by == sort_by {
            self.sort_order = match self.sort_order {
                SortOrder::Asc => SortOrder::Desc,
                SortOrder::Desc => SortOrder::Asc,
            }
        } else {
            self.sort_by = sort_by;
            self.sort_order = SortOrder::Desc;
        }
        self.state.select(Some(0));
    }

    // ADDED: Kill menu navigation
    fn next_kill_signal(&mut self) {
        let i = match self.kill_menu_state.selected() {
            Some(i) => if i >= self.kill_signals.len() - 1 { 0 } else { i + 1 },
            None => 0,
        };
        self.kill_menu_state.select(Some(i));
    }

    fn previous_kill_signal(&mut self) {
        let i = match self.kill_menu_state.selected() {
            Some(i) => if i == 0 { self.kill_signals.len() - 1 } else { i - 1 },
            None => 0,
        };
        self.kill_menu_state.select(Some(i));
    }
}

// Helper functions - No changes
fn status_to_string(s: ProcessStatus) -> String {
    match s {
        ProcessStatus::Run => "R".to_string(),
        ProcessStatus::Sleep => "S".to_string(),
        ProcessStatus::Idle => "D".to_string(),
        ProcessStatus::Zombie => "Z".to_string(),
        _ => format!("{:?}", s),
    }
}

fn format_time(secs: u64) -> String {
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;
    if days > 99 { format!("{}d", days) }
    else if days > 0 { format!("{:02}d{:02}h", days, hours % 24) }
    else { format!("{:02}:{:02}:{:02}", hours, mins % 60, secs % 60) }
}

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    format!("{} days, {:02}:{:02}", days, hours, mins)
}

fn kill_process(pid: u32, signal: i32) -> Result<(), String> {
    let output = Command::new("kill").arg(format!("-{}", signal)).arg(pid.to_string()).output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => Err(e.to_string()),
    }
}

// ADDED: Helper to create a centered popup area
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// main() - Significant changes to rendering and input handling
fn main() -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = Arc::new(Mutex::new(App::new()));
    let running = Arc::new(Mutex::new(true));

    {
        let app = Arc::clone(&app);
        let running = Arc::clone(&running);
        thread::spawn(move || {
            let mut sys = System::new_all();
            while *running.lock().unwrap() {
                app.lock().unwrap().update_data(&mut sys);
                thread::sleep(Duration::from_secs(2));
            }
        });
    }
    thread::sleep(Duration::from_millis(100));

    loop {
        let mut app_guard = app.lock().unwrap();
        let mut table_height = 0;

        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(5), Constraint::Min(10), Constraint::Length(3)])
                .split(size);

            // --- HEADER ---
            let header_chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(chunks[0]);

            let num_cpus = app_guard.cpus.len();
            if num_cpus > 0 {
                let cpu_constraints: Vec<Constraint> = (0..num_cpus).map(|_| Constraint::Ratio(1, num_cpus as u32)).collect();
                let cpu_chunks = Layout::default().direction(Direction::Horizontal).constraints(cpu_constraints).split(header_chunks[0]);
                for (i, &cpu_usage) in app_guard.cpus.iter().enumerate() {
                    let gauge = Gauge::default().block(Block::default().title(format!("CPU{}", i+1))).percent(cpu_usage as u16).gauge_style(Style::default().fg(Color::Green));
                    f.render_widget(gauge, cpu_chunks[i]);
                }
            }

            let right_header_chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(3)]).split(header_chunks[1]);

            let mem_text = format!("Mem[{} / {}MiB]", app_guard.used_mem / 1024 / 1024, app_guard.total_mem / 1024 / 1024);
            f.render_widget(Paragraph::new(mem_text).style(Style::default().fg(Color::Cyan)), right_header_chunks[0]);

            let swp_text = format!("Swp[{} / {}MiB]", app_guard.used_swap / 1024 / 1024, app_guard.total_swap / 1024 / 1024);
            f.render_widget(Paragraph::new(swp_text).style(Style::default().fg(Color::Magenta)), right_header_chunks[1]);

            let tasks_text = format!("Tasks: {}, Load Avg: {:.2} {:.2} {:.2}", app_guard.processes.len(), app_guard.load_avg.one, app_guard.load_avg.five, app_guard.load_avg.fifteen);
            let uptime_text = format!("Uptime: {}", format_uptime(app_guard.uptime));
            f.render_widget(Paragraph::new(format!("{}\n{}", tasks_text, uptime_text)), right_header_chunks[2]);

            // --- TABLE ---
            table_height = chunks[1].height as usize - 2;
            let header_cells = ["PID", "USER", "VIRT", "S", "CPU%", "MEM%", "TIME+", "COMMAND"].iter().map(|h| Cell::from(*h).style(Style::default().fg(Color::Red)));
            let header = Row::new(header_cells).style(Style::default().bg(Color::Blue)).height(1);

            let rows: Vec<Row> = if app_guard.tree_view {
                let tree_items = app_guard.tree_ordered_processes();
                tree_items.iter().map(|(depth, p)| {
                    let mut command = " ".repeat(*depth * 2);
                    if *depth > 0 { command.push_str("└─ "); }
                    command.push_str(&p.command);

                    Row::new(vec![
                        Cell::from(p.pid.to_string()), Cell::from(p.user.clone()), Cell::from(format!("{}M", p.virtual_mem / 1024 / 1024)),
                        Cell::from(p.status.clone()), Cell::from(format!("{:.1}", p.cpu)), Cell::from(format!("{:.1}", p.mem)),
                        Cell::from(format_time(p.cpu_time)), Cell::from(command),
                    ])
                }).collect()
            } else {
                let procs = app_guard.filtered_processes();
                procs.iter().map(|p| {
                    Row::new(vec![
                        Cell::from(p.pid.to_string()), Cell::from(p.user.clone()), Cell::from(format!("{}M", p.virtual_mem / 1024 / 1024)),
                        Cell::from(p.status.clone()), Cell::from(format!("{:.1}", p.cpu)), Cell::from(format!("{:.1}", p.mem)),
                        Cell::from(format_time(p.cpu_time)), Cell::from(p.command.clone()),
                    ])
                }).collect()
            };

            let table = Table::new(rows, [Constraint::Length(6), Constraint::Length(9), Constraint::Length(7), Constraint::Length(2), Constraint::Length(5), Constraint::Length(5), Constraint::Length(9), Constraint::Min(20)])
                .header(header).block(Block::default().borders(Borders::ALL).title("Processes"))
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED)).highlight_symbol(">> ");
            f.render_stateful_widget(table, chunks[1], &mut app_guard.state);

            // --- FOOTER ---
            let footer_area = chunks[2];
            if app_guard.input_mode == InputMode::Search {
                let search_text = format!("/{}", app_guard.search_query);
                let search_bar = Paragraph::new(search_text.clone()).style(Style::default().fg(Color::Yellow)).block(Block::default().borders(Borders::ALL).title("Search (Esc to cancel, Enter to apply)"));
                f.render_widget(Clear, footer_area);
                f.render_widget(search_bar, footer_area);
                f.set_cursor(footer_area.x + search_text.len() as u16 + 1, footer_area.y + 1);
            } else {
                let footer_chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(2)]).split(footer_area);
                let help_text = "F5 Tree  F9 Kill  F10 Quit  '/' Search  'I' Invert";
                f.render_widget(Paragraph::new(help_text), footer_chunks[1]);
                let dynamic_text = if let Some(filter) = &app_guard.active_filter {
                    format!("[Filter: {}] (Esc to clear)", filter)
                } else if let Some(msg) = &app_guard.message { msg.clone() } else { "".to_string() };
                f.render_widget(Paragraph::new(dynamic_text), footer_chunks[0]);
            }

            // --- POPUPS (drawn last to be on top) ---
            if app_guard.input_mode == InputMode::KillMenu {
                let items: Vec<ListItem> = app_guard.kill_signals.iter().map(|(s, _)| ListItem::new(*s)).collect();
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("Select signal"))
                    .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
                    .highlight_symbol(">> ");

                let area = centered_rect(20, 30, size);
                f.render_widget(Clear, area);
                f.render_stateful_widget(list, area, &mut app_guard.kill_menu_state);
            }
        })?;

        let page_size = table_height;
        drop(app_guard);

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                let mut app = app.lock().unwrap();
                match app.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char('q') | KeyCode::F(10) => { *running.lock().unwrap() = false; break; }
                        KeyCode::Char('/') => { app.input_mode = InputMode::Search; app.message = None; }
                        KeyCode::Char('I') | KeyCode::Char('i') => { let s = app.sort_by; app.set_sort_by(s); }
                        KeyCode::Char('P') | KeyCode::Char('p') => app.set_sort_by(SortBy::PID),
                        KeyCode::Char('U') | KeyCode::Char('u') => app.set_sort_by(SortBy::User),
                        KeyCode::Char('M') | KeyCode::Char('m') => app.set_sort_by(SortBy::MEM),
                        KeyCode::Char('T') | KeyCode::Char('t') => app.set_sort_by(SortBy::Time),
                        KeyCode::Char('C') | KeyCode::Char('c') => app.set_sort_by(SortBy::Command),
                        KeyCode::Down => app.next(),
                        KeyCode::Up => app.previous(),
                        KeyCode::PageDown => app.page_down(page_size),
                        KeyCode::PageUp => app.page_up(page_size),
                        KeyCode::Home => app.home(),
                        KeyCode::End => app.end(),
                        KeyCode::F(5) => app.tree_view = !app.tree_view,
                        KeyCode::F(9) => { if app.selected_pid().is_some() { app.input_mode = InputMode::KillMenu; } }
                        KeyCode::Esc => {
                            if app.active_filter.is_some() {
                                app.active_filter = None;
                                app.search_query.clear();
                                app.state.select(Some(0));
                            }
                            app.message = None;
                        }
                        _ => {}
                    },
                    InputMode::Search => match key.code {
                        KeyCode::Enter => {
                            app.input_mode = InputMode::Normal;
                            app.active_filter = if app.search_query.is_empty() { None } else { Some(app.search_query.clone()) };
                            app.state.select(Some(0));
                        }
                        KeyCode::Char(c) => app.search_query.push(c),
                        KeyCode::Backspace => { app.search_query.pop(); },
                        KeyCode::Esc => { app.input_mode = InputMode::Normal; app.search_query.clear(); }
                        _ => {}
                    },
                    InputMode::KillMenu => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.input_mode = InputMode::Normal,
                        KeyCode::Down => app.next_kill_signal(),
                        KeyCode::Up => app.previous_kill_signal(),
                        KeyCode::Enter => {
                            if let (Some(pid), Some(selected_signal_idx)) = (app.selected_pid(), app.kill_menu_state.selected()) {
                                let signal = app.kill_signals[selected_signal_idx].1;
                                match kill_process(pid, signal) {
                                    Ok(_) => app.message = Some(format!("Sent signal {} to PID {}", signal, pid)),
                                    Err(e) => app.message = Some(format!("Error killing {}: {}", pid, e)),
                                }
                            }
                            app.input_mode = InputMode::Normal;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}
