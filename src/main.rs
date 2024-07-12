use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use num_cpus;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{CpuExt, System, SystemExt};
use tui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Span, Spans},
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, List, ListItem, Paragraph},
    Terminal,
};

const TARGET_OPERATIONS: u64 = 100_000_000_000; // 1 trillion operations

struct AppState {
    total_operations: u64,
    elapsed_time: Duration,
    cpu_usage: f32,
    memory_usage: f64,
    cpu_usage_history: Vec<(f64, f64)>,
    memory_usage_history: Vec<(f64, f64)>,
    cpu_details: Vec<(String, f32, u64)>,
    system_info: Vec<(String, String)>,
}

fn main() -> Result<(), io::Error> {
    // Terminal initialization
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let num_cores = num_cpus::get();
    let total_operations = Arc::new(AtomicU64::new(0));
    let start_time = Instant::now();

    let mut handles = vec![];

    for _ in 0..num_cores {
        let total_operations = total_operations.clone();
        let handle = thread::spawn(move || {
            let mut n: u64 = 0;
            loop {
                n = n.wrapping_add(1);
                if n % 1_000_000 == 0 {
                    total_operations.fetch_add(1_000_000, Ordering::Relaxed);
                    if total_operations.load(Ordering::Relaxed) >= TARGET_OPERATIONS {
                        break;
                    }
                }
            }
        });
        handles.push(handle);
    }

    let mut sys = System::new_all();
    sysinfo::get_current_pid().expect("Failed to get current PID");

    let mut app_state = AppState {
        total_operations: 0,
        elapsed_time: Duration::new(0, 0),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        cpu_usage_history: vec![],
        memory_usage_history: vec![],
        cpu_details: vec![],
        system_info: vec![],
    };

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Length(8),
                        Constraint::Min(0),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            render_header(f, chunks[0], &app_state);
            render_charts(f, chunks[1], &app_state);
            render_details(f, chunks[2], &app_state);
        })?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if let KeyCode::Char('q') = key.code {
                    break;
                }
            }
        }

        update_app_state(
            &mut app_state,
            &mut sys,
            total_operations.load(Ordering::Relaxed),
            start_time,
        );

        if app_state.total_operations >= TARGET_OPERATIONS {
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

    for handle in handles {
        handle.join().unwrap();
    }

    let total_time = start_time.elapsed();
    let operations_per_second = TARGET_OPERATIONS as f64 / total_time.as_secs_f64();

    println!("Stress test completed");
    println!("Total operations: {}", TARGET_OPERATIONS);
    println!("Total time: {:.2?}", total_time);
    println!("Operations per second: {:.2}", operations_per_second);
    println!(
        "Score: {:.2} million ops/sec",
        operations_per_second / 1_000_000.0
    );

    Ok(())
}

fn render_header(
    f: &mut tui::Frame<CrosstermBackend<io::Stdout>>,
    area: Rect,
    app_state: &AppState,
) {
    let progress = app_state.total_operations as f64 / TARGET_OPERATIONS as f64;
    let gauge = Gauge::default()
        .block(Block::default().title("Progress").borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent((progress * 100.0) as u16);
    f.render_widget(gauge, area);
}

fn create_filled_dataset<'a>(
    data: &'a [(f64, f64)],
    name: String,
    color: Color,
    filled_data: &'a mut Vec<(f64, f64)>,
) -> Dataset<'a> {
    filled_data.clear();
    for &(x, y) in data {
        filled_data.push((x, y));
        for fill_y in (0..=y as u64).step_by(5) {
            filled_data.push((x, fill_y as f64));
        }
    }

    Dataset::default()
        .name(name)
        .marker(symbols::Marker::Braille)
        .style(Style::default().fg(color))
        .graph_type(tui::widgets::GraphType::Scatter)
        .data(&filled_data[..])
}

fn render_charts(
    f: &mut tui::Frame<CrosstermBackend<io::Stdout>>,
    area: Rect,
    app_state: &AppState,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(area);

    let mut cpu_filled_data = Vec::new();
    let cpu_dataset = create_filled_dataset(
        &app_state.cpu_usage_history,
        "CPU Usage".to_string(),
        Color::Cyan,
        &mut cpu_filled_data,
    );
    let binding = [cpu_dataset];
    let cpu_chart = create_chart(&binding, "CPU Usage", [0.0, 60.0], [0.0, 100.0]);
    f.render_widget(cpu_chart, chunks[0]);

    let mut memory_filled_data = Vec::new();
    let memory_dataset = create_filled_dataset(
        &app_state.memory_usage_history,
        "Memory Usage".to_string(),
        Color::Magenta,
        &mut memory_filled_data,
    );
    let binding = [memory_dataset];
    let memory_chart = create_chart(&binding, "Memory Usage", [0.0, 60.0], [0.0, 100.0]);
    f.render_widget(memory_chart, chunks[1]);
}

fn create_chart<'a>(
    datasets: &'a [Dataset],
    title: &'a str,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
) -> Chart<'a> {
    Chart::new(datasets.to_vec())
        .block(Block::default().title(title).borders(Borders::ALL))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds(x_bounds)
                .labels(
                    [" ", " ", " ", " ", " "]
                        .iter()
                        .map(|&s| s.into())
                        .collect(),
                ),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds(y_bounds)
                .labels(
                    [" ", " ", " ", " ", " "]
                        .iter()
                        .map(|&s| s.into())
                        .collect(),
                ),
        )
}

fn render_details(
    f: &mut tui::Frame<CrosstermBackend<io::Stdout>>,
    area: Rect,
    app_state: &AppState,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ]
            .as_ref(),
        )
        .split(area);

    render_stats(f, chunks[0], app_state);
    render_cpu_details(f, chunks[1], app_state);
    render_system_info(f, chunks[2], app_state);
}

fn render_stats(
    f: &mut tui::Frame<CrosstermBackend<io::Stdout>>,
    area: Rect,
    app_state: &AppState,
) {
    let stats = vec![
        Spans::from(vec![
            Span::raw("Time: "),
            Span::styled(
                format!("{:.2?}", app_state.elapsed_time),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Spans::from(vec![
            Span::raw("Operations: "),
            Span::styled(
                format!("{}", app_state.total_operations),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Spans::from(vec![
            Span::raw("CPU Usage: "),
            Span::styled(
                format!("{:.2}%", app_state.cpu_usage),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Spans::from(vec![
            Span::raw("Memory Usage: "),
            Span::styled(
                format!("{:.2}%", app_state.memory_usage),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(stats)
        .block(Block::default().title("Statistics").borders(Borders::ALL))
        .style(Style::default().fg(Color::White));
    f.render_widget(paragraph, area);
}

fn render_cpu_details(
    f: &mut tui::Frame<CrosstermBackend<io::Stdout>>,
    area: Rect,
    app_state: &AppState,
) {
    let items: Vec<ListItem> = app_state
        .cpu_details
        .iter()
        .map(|(name, usage, freq)| {
            ListItem::new(Spans::from(vec![
                Span::raw(format!("{}: ", name)),
                Span::styled(
                    format!("{:.2}% @ {} MHz", usage, freq),
                    Style::default().fg(Color::Cyan),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title("CPU Details").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    f.render_widget(list, area);
}

fn render_system_info(
    f: &mut tui::Frame<CrosstermBackend<io::Stdout>>,
    area: Rect,
    app_state: &AppState,
) {
    let items: Vec<ListItem> = app_state
        .system_info
        .iter()
        .map(|(key, value)| {
            ListItem::new(Spans::from(vec![
                Span::raw(format!("{}: ", key)),
                Span::styled(value.clone(), Style::default().fg(Color::Yellow)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title("System Information")
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    f.render_widget(list, area);
}

fn update_app_state(
    app_state: &mut AppState,
    sys: &mut System,
    total_operations: u64,
    start_time: Instant,
) {
    sys.refresh_all();

    app_state.total_operations = total_operations;
    app_state.elapsed_time = start_time.elapsed();

    let cpu_usage_all: f32 =
        sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / sys.cpus().len() as f32;
    app_state.cpu_usage = cpu_usage_all;

    let total_memory = sys.total_memory();
    let used_memory = sys.used_memory();
    app_state.memory_usage = (used_memory as f64 / total_memory as f64) * 100.0;

    let elapsed_seconds = app_state.elapsed_time.as_secs_f64();
    let cpu_usage = (app_state.cpu_usage as f64 * 100.0).round() / 100.0; // Round to nearest percentage
    let memory_usage = (app_state.memory_usage * 100.0).round() / 100.0; // Round to nearest percentage

    app_state
        .cpu_usage_history
        .push((elapsed_seconds, cpu_usage));
    app_state
        .memory_usage_history
        .push((elapsed_seconds, memory_usage));

    if app_state.cpu_usage_history.len() > 240 {
        app_state.cpu_usage_history.remove(0);
        app_state.memory_usage_history.remove(0);
    }

    app_state.cpu_details = sys
        .cpus()
        .iter()
        .enumerate()
        .map(|(i, cpu)| (format!("CPU {}", i), cpu.cpu_usage(), cpu.frequency()))
        .collect();

    app_state.system_info = vec![
        ("OS".to_string(), sys.name().unwrap_or_default()),
        (
            "OS Version".to_string(),
            sys.os_version().unwrap_or_default(),
        ),
        (
            "Kernel".to_string(),
            sys.kernel_version().unwrap_or_default(),
        ),
        ("Host Name".to_string(), sys.host_name().unwrap_or_default()),
        (
            "Total Memory".to_string(),
            format!("{} MB", sys.total_memory() / 1024 / 1024),
        ),
        (
            "Total Swap".to_string(),
            format!("{} MB", sys.total_swap() / 1024 / 1024),
        ),
    ];
}
