use std::{
    collections::VecDeque,
    env,
    io::{self, Read, Write},
    panic,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, size, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use install_split_pane::{pane_terminal_size, screen_layout};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

const PTY_SCROLLBACK: usize = 10_000;
const POLL_INTERVAL: Duration = Duration::from_millis(16);
const SMOKE_TIMEOUT: Duration = Duration::from_secs(8);
const SMOKE_LEFT_MARKER: &str = "__LEVITATE_SPLIT_LEFT_OK__";
const SMOKE_RIGHT_MARKER: &str = "__LEVITATE_SPLIT_RIGHT_OK__";

fn main() -> Result<()> {
    let args = Args::parse()?;
    if args.smoke {
        return run_smoke(&args);
    }
    run_ui(&args)
}

/// `install-split [--smoke] --left PROGRAM [ARGS...] --right PROGRAM [ARGS...]`
#[derive(Debug, Clone)]
struct Args {
    smoke: bool,
    left: Vec<String>,
    right: Vec<String>,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut smoke = false;
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--smoke" => smoke = true,
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "--left" => {
                    for word in args.by_ref() {
                        if word == "--right" {
                            right = args.by_ref().collect();
                            break;
                        }
                        left.push(word);
                    }
                }
                other => bail!("unknown option '{}'. Use --help for usage.", other),
            }
        }
        if left.is_empty() || right.is_empty() {
            bail!("give both panes a command: --left PROGRAM [ARGS...] --right PROGRAM [ARGS...]");
        }
        Ok(Self { smoke, left, right })
    }
}

fn print_help() {
    println!("Usage:");
    println!("  install-split --left PROGRAM [ARGS...] --right PROGRAM [ARGS...]");
    println!("  install-split --smoke --left PROGRAM [ARGS...] --right PROGRAM [ARGS...]");
    println!();
    println!("Runs the left command (the shell) beside the right one (the docs).");
    println!();
    println!("Options:");
    println!("  --smoke   Non-interactive split-pane smoke check for install UX tests.");
}

fn run_smoke(args: &Args) -> Result<()> {
    let right_program = &args.right[0];

    let (tx, rx) = mpsc::channel::<PtyEvent>();
    let pty_size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let left_cmd = pane_command(&args.left);
    let right_probe = format!(
        "if command -v \"$1\" >/dev/null 2>&1; then printf '{SMOKE_RIGHT_MARKER}\\n'; else printf '__LEVITATE_SPLIT_RIGHT_MISSING__\\n'; fi; sleep 0.2"
    );
    let right_cmd = pane_command(&[
        "/bin/sh".to_string(),
        "-c".to_string(),
        right_probe,
        "_".to_string(),
        right_program.clone(),
    ]);

    let mut left = spawn_raw_pane(PaneId::Left, left_cmd, pty_size, tx.clone())?;
    let mut right = spawn_raw_pane(PaneId::Right, right_cmd, pty_size, tx)?;

    write_to_pty(
        &left.writer,
        format!("printf '{}\\n'\\r", SMOKE_LEFT_MARKER).as_bytes(),
    )?;

    let deadline = Instant::now() + SMOKE_TIMEOUT;
    let mut left_seen = false;
    let mut right_seen = false;
    let mut left_out = String::new();
    let mut right_out = String::new();

    while Instant::now() < deadline && !(left_seen && right_seen) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(PtyEvent::Output(PaneId::Left, bytes)) => {
                let chunk = String::from_utf8_lossy(&bytes);
                left_out.push_str(&chunk);
                if left_out.contains(SMOKE_LEFT_MARKER) {
                    left_seen = true;
                }
            }
            Ok(PtyEvent::Output(PaneId::Right, bytes)) => {
                let chunk = String::from_utf8_lossy(&bytes);
                right_out.push_str(&chunk);
                if right_out.contains(SMOKE_RIGHT_MARKER) {
                    right_seen = true;
                }
            }
            Ok(PtyEvent::Closed(_)) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = left.child.kill();
    let _ = left.child.wait();
    let _ = right.child.kill();
    let _ = right.child.wait();

    if left_seen && right_seen {
        println!(
            "split-smoke:ok left={} right={}",
            args.left[0], right_program
        );
        return Ok(());
    }

    bail!(
        "split-smoke: failed (left_ok={}, right_ok={})\nleft_output:\n{}\nright_output:\n{}",
        left_seen,
        right_seen,
        truncate_for_error(&left_out),
        truncate_for_error(&right_out)
    )
}

fn truncate_for_error(s: &str) -> String {
    const MAX: usize = 300;
    if s.len() <= MAX {
        return s.to_string();
    }
    format!("{}...(truncated)", &s[s.len() - MAX..])
}

fn run_ui(args: &Args) -> Result<()> {
    let _restore = TerminalRestore;
    install_panic_hook();

    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide).context("enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal backend")?;
    terminal.clear().context("clear terminal")?;

    let (cols, rows) = size().context("read terminal size")?;
    let pane_rects = screen_layout(Rect::new(0, 0, cols, rows)).panes;
    let left_size = pty_size_for_area(pane_rects[0]);
    let right_size = pty_size_for_area(pane_rects[1]);

    let (tx, rx) = mpsc::channel::<PtyEvent>();

    let left = spawn_raw_pane(
        PaneId::Left,
        pane_command(&args.left),
        left_size,
        tx.clone(),
    )?;
    let right = spawn_raw_pane(PaneId::Right, pane_command(&args.right), right_size, tx)?;

    let mut app = App::new(
        args,
        PaneState::new("Shell", left, left_size),
        PaneState::new("Docs", right, right_size),
    );

    event_loop(&mut terminal, &mut app, &rx)
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    pty_rx: &mpsc::Receiver<PtyEvent>,
) -> Result<()> {
    while !app.should_quit {
        while let Ok(event) = pty_rx.try_recv() {
            app.on_pty_event(event);
        }

        terminal.draw(|frame| draw(frame, app)).context("draw ui")?;

        if event::poll(POLL_INTERVAL).context("poll terminal events")? {
            match event::read().context("read terminal event")? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Release {
                        continue;
                    }

                    if is_ctrl_char(&key, 'q') {
                        app.should_quit = true;
                        continue;
                    }
                    if is_ctrl_char(&key, 'g') {
                        app.focus = app.focus.toggle();
                        continue;
                    }

                    if let Some(bytes) = key_event_to_bytes(key) {
                        app.write_to_focused(&bytes)?;
                    }
                }
                Event::Paste(data) => {
                    app.write_to_focused(data.as_bytes())?;
                }
                Event::Resize(cols, rows) => {
                    let pane_rects = screen_layout(Rect::new(0, 0, cols, rows)).panes;
                    app.resize(pane_rects[0], pane_rects[1])?;
                }
                _ => {}
            }
        }
    }

    let _ = app.left.raw.child.kill();
    let _ = app.left.raw.child.wait();
    let _ = app.right.raw.child.kill();
    let _ = app.right.raw.child.wait();
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusPane {
    Left,
    Right,
}

impl FocusPane {
    fn toggle(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

struct App {
    focus: FocusPane,
    left: PaneState,
    right: PaneState,
    should_quit: bool,
    logs: VecDeque<String>,
}

impl App {
    fn new(args: &Args, left: PaneState, right: PaneState) -> Self {
        let mut logs = VecDeque::new();
        logs.push_back(format!("left={}", args.left.join(" ")));
        logs.push_back(format!("right={}", args.right.join(" ")));
        Self {
            focus: FocusPane::Left,
            left,
            right,
            should_quit: false,
            logs,
        }
    }

    fn on_pty_event(&mut self, event: PtyEvent) {
        match event {
            PtyEvent::Output(PaneId::Left, bytes) => {
                self.left.parser.process(&bytes);
            }
            PtyEvent::Output(PaneId::Right, bytes) => {
                self.right.parser.process(&bytes);
            }
            PtyEvent::Closed(PaneId::Left) => {
                self.left.closed = true;
                self.logs.push_back("left pane exited".to_string());
            }
            PtyEvent::Closed(PaneId::Right) => {
                self.right.closed = true;
                self.logs.push_back("right pane exited".to_string());
            }
        }
        while self.logs.len() > 20 {
            self.logs.pop_front();
        }
    }

    fn write_to_focused(&mut self, bytes: &[u8]) -> Result<()> {
        match self.focus {
            FocusPane::Left => write_to_pty(&self.left.raw.writer, bytes),
            FocusPane::Right => write_to_pty(&self.right.raw.writer, bytes),
        }
    }

    fn resize(&mut self, left_area: Rect, right_area: Rect) -> Result<()> {
        self.left.resize(left_area)?;
        self.right.resize(right_area)?;
        Ok(())
    }
}

struct PaneState {
    title: &'static str,
    raw: RawPane,
    parser: vt100::Parser,
    closed: bool,
}

impl PaneState {
    fn new(title: &'static str, raw: RawPane, size: PtySize) -> Self {
        Self {
            title,
            parser: vt100::Parser::new(size.rows as u16, size.cols as u16, PTY_SCROLLBACK),
            raw,
            closed: false,
        }
    }

    fn resize(&mut self, area: Rect) -> Result<()> {
        let new_size = pty_size_for_area(area);
        self.raw
            .master
            .resize(new_size)
            .with_context(|| format!("resize {} PTY", self.title))?;
        self.parser
            .set_size(new_size.rows as u16, new_size.cols as u16);
        Ok(())
    }
}

struct RawPane {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneId {
    Left,
    Right,
}

enum PtyEvent {
    Output(PaneId, Vec<u8>),
    Closed(PaneId),
}

fn spawn_raw_pane(
    id: PaneId,
    cmd: CommandBuilder,
    size: PtySize,
    tx: mpsc::Sender<PtyEvent>,
) -> Result<RawPane> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(size).context("open PTY pair")?;

    let child = pair
        .slave
        .spawn_command(cmd)
        .context("spawn child in PTY")?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().context("clone PTY reader")?;
    let writer = pair.master.take_writer().context("take PTY writer")?;
    let writer = Arc::new(Mutex::new(writer));

    thread::spawn(move || {
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(PtyEvent::Closed(id));
                    break;
                }
                Ok(n) => {
                    if tx.send(PtyEvent::Output(id, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    let _ = tx.send(PtyEvent::Closed(id));
                    break;
                }
            }
        }
    });

    Ok(RawPane {
        master: pair.master,
        writer,
        child,
    })
}

fn pane_command(argv: &[String]) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(&argv[0]);
    cmd.args(&argv[1..]);
    apply_common_env(&mut cmd);
    cmd
}

fn apply_common_env(cmd: &mut CommandBuilder) {
    let mut term = env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string());
    let normalized = term.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized == "dumb"
        || normalized == "vt100"
        || normalized == "vt102"
        || normalized == "linux"
    {
        term = "xterm-256color".to_string();
    }
    cmd.env("TERM", term);
    let colorterm = env::var("COLORTERM").unwrap_or_else(|_| "truecolor".to_string());
    cmd.env("COLORTERM", colorterm);
    let force_color = env::var("FORCE_COLOR").unwrap_or_else(|_| "3".to_string());
    cmd.env("FORCE_COLOR", force_color);
    // Ensure NO_COLOR from host/session cannot silently downgrade docs rendering.
    cmd.env("NO_COLOR", "");
}

fn draw(frame: &mut Frame, app: &App) {
    let layout = screen_layout(frame.area());
    let panes = layout.panes;
    render_pane(frame, panes[0], &app.left, app.focus == FocusPane::Left);
    render_pane(frame, panes[1], &app.right, app.focus == FocusPane::Right);

    let hint = format!(
        "focus={} | Ctrl-g toggle pane | Ctrl-q quit",
        app.focus.label()
    );
    let footer = Paragraph::new(hint).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, layout.footer);
}

fn render_pane(frame: &mut Frame, area: Rect, pane: &PaneState, focused: bool) {
    let border_style = if focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let mut title = pane.title.to_string();
    if pane.closed {
        title.push_str(" (exited)");
    }

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    let rows = render_styled_rows(&pane.parser, inner.height, inner.width);
    let paragraph = Paragraph::new(Text::from(rows)).block(block);
    frame.render_widget(paragraph, area);
}

fn render_styled_rows(parser: &vt100::Parser, max_rows: u16, max_cols: u16) -> Vec<Line<'static>> {
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let rows_to_render = rows.min(max_rows);
    let cols_to_render = cols.min(max_cols);
    let start_row = rows.saturating_sub(rows_to_render);

    let mut lines = Vec::with_capacity(rows_to_render as usize);
    for row in start_row..rows {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut run_text = String::new();
        let mut run_style = Style::default();
        let mut have_run = false;

        for col in 0..cols_to_render {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }

            let mut text = cell.contents();
            if text.is_empty() {
                text.push(' ');
            }
            let style = style_for_cell(cell);

            if have_run && style == run_style {
                run_text.push_str(&text);
                continue;
            }

            if !run_text.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run_text), run_style));
            }
            run_style = style;
            have_run = true;
            run_text.push_str(&text);
        }

        if !run_text.is_empty() {
            spans.push(Span::styled(run_text, run_style));
        }
        lines.push(Line::from(spans));
    }

    lines
}

fn style_for_cell(cell: &vt100::Cell) -> Style {
    let mut fg = ratatui_color(cell.fgcolor());
    let mut bg = ratatui_color(cell.bgcolor());
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }

    let mut style = Style::default();
    if let Some(color) = fg {
        style = style.fg(color);
    }
    if let Some(color) = bg {
        style = style.bg(color);
    }
    if cell.bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.italic() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

fn ratatui_color(color: vt100::Color) -> Option<Color> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(idx) => Some(Color::Indexed(idx)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

fn pty_size_for_area(area: Rect) -> PtySize {
    let (cols, rows) = pane_terminal_size(area);
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn write_to_pty(writer: &Arc<Mutex<Box<dyn Write + Send>>>, bytes: &[u8]) -> Result<()> {
    let mut guard = writer
        .lock()
        .map_err(|_| anyhow!("PTY writer lock poisoned"))?;
    guard.write_all(bytes).context("write to PTY")?;
    guard.flush().context("flush PTY")?;
    Ok(())
}

fn is_ctrl_char(key: &KeyEvent, c: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char(k) if k.eq_ignore_ascii_case(&c))
}

fn key_event_to_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let is_alt = key.modifiers.contains(KeyModifiers::ALT);
    let is_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if is_alt {
        out.push(0x1b);
    }

    if is_ctrl {
        match key.code {
            KeyCode::Char(c) => {
                let c = c.to_ascii_lowercase();
                if c.is_ascii() {
                    out.push((c as u8) & 0x1f);
                    return Some(out);
                }
            }
            KeyCode::Left => {
                out.extend_from_slice(b"\x1b[1;5D");
                return Some(out);
            }
            KeyCode::Right => {
                out.extend_from_slice(b"\x1b[1;5C");
                return Some(out);
            }
            KeyCode::Up => {
                out.extend_from_slice(b"\x1b[1;5A");
                return Some(out);
            }
            KeyCode::Down => {
                out.extend_from_slice(b"\x1b[1;5B");
                return Some(out);
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Enter => out.push(b'\r'),
        KeyCode::Tab => out.push(b'\t'),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Backspace => out.push(0x7f),
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Left => out.extend_from_slice(b"\x1b[D"),
        KeyCode::Right => out.extend_from_slice(b"\x1b[C"),
        KeyCode::Up => out.extend_from_slice(b"\x1b[A"),
        KeyCode::Down => out.extend_from_slice(b"\x1b[B"),
        KeyCode::Home => out.extend_from_slice(b"\x1b[H"),
        KeyCode::End => out.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => out.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => out.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => out.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => out.extend_from_slice(b"\x1b[2~"),
        KeyCode::Char(c) => {
            let mut buf = [0_u8; 4];
            let s = c.encode_utf8(&mut buf);
            out.extend_from_slice(s.as_bytes());
        }
        _ => return None,
    }

    Some(out)
}

fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);
}

struct TerminalRestore;

impl Drop for TerminalRestore {
    fn drop(&mut self) {
        restore_terminal();
    }
}
