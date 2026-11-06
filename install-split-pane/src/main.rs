//! install-split: the install screen's terminal host.
//!
//! Two panes side by side, each running a program in its own PTY, drawn through a
//! vt100 emulator, over a footer that lists the host's keys. Every other key goes to
//! the focused pane. A pane whose program exits stays on screen and restarts on
//! Enter; the host itself ends only when the user quits.

use std::{
    env,
    io::{self, Read, Write},
    panic,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, size, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use install_split_pane::{pane_terminal_size, screen_layout};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

const SMOKE_TIMEOUT: Duration = Duration::from_secs(8);
const SMOKE_LEFT_MARKER: &str = "__LEVITATE_SPLIT_LEFT_OK__";
const SMOKE_RIGHT_MARKER: &str = "__LEVITATE_SPLIT_RIGHT_OK__";
/// How long to wait for a key before looking for pane output again.
const INPUT_WAIT: Duration = Duration::from_millis(30);

/// The host's own keys, all Ctrl-<char>; the footer shows this table. Ctrl-] is free
/// in bash, less and the docs app (Ctrl-G, the obvious pick, is less's file info).
const HOST_KEYS: &[(HostKey, char, &str, &str)] = &[
    (HostKey::Switch, ']', "Ctrl-]", "switch pane"),
    (HostKey::Quit, 'q', "Ctrl-q", "quit"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum HostKey {
    Switch,
    Quit,
}

fn main() -> Result<()> {
    match Command::parse(env::args().skip(1))? {
        Command::Help => print_help(),
        Command::Version => println!("install-split {}", env!("CARGO_PKG_VERSION")),
        Command::Run(args) if args.smoke => run_smoke(&args)?,
        Command::Run(args) => run_ui(&args)?,
    }
    Ok(())
}

enum Command {
    Help,
    Version,
    Run(Args),
}

/// `install-split [--smoke] --left PROGRAM [ARGS...] --right PROGRAM [ARGS...]`
struct Args {
    smoke: bool,
    /// The left and right panes' argv.
    panes: [Vec<String>; 2],
}

impl Command {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut smoke = false;
        let mut left = Vec::new();
        let mut right = Vec::new();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--smoke" => smoke = true,
                "-h" | "--help" => return Ok(Self::Help),
                "--version" => return Ok(Self::Version),
                "--left" => {
                    for word in args.by_ref() {
                        if word == "--right" {
                            right = args.by_ref().collect();
                            break;
                        }
                        left.push(word);
                    }
                }
                other => bail!("unknown option '{other}'. Use --help for usage."),
            }
        }
        for (side, argv) in [(Side::Left, &left), (Side::Right, &right)] {
            let Some(program) = argv.first() else {
                bail!(
                    "give both panes a command: --left PROGRAM [ARGS...] --right PROGRAM [ARGS...]"
                );
            };
            if !program_exists(program) {
                bail!("{} pane: '{program}' is not a program on PATH", side.name());
            }
        }
        Ok(Self::Run(Args {
            smoke,
            panes: [left, right],
        }))
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
    println!("  --smoke     Non-interactive split-pane smoke check for install UX tests.");
    println!("  --version   Print the version.");
    println!();
    println!("Keys:");
    for (_, _, key, action) in HOST_KEYS {
        println!("  {key:<10}{action}");
    }
    println!("  Enter     restart the focused pane's program after it exits");
}

fn program_exists(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    env::var_os("PATH")
        .is_some_and(|path| env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

impl Side {
    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Left => "Shell",
            Self::Right => "Docs",
        }
    }
}

/// What a pane's reader thread reports. `run` tells a restarted program's output
/// from the previous one's.
enum PtyEvent {
    Output(Side, u64, Vec<u8>),
    Closed(Side, u64, Option<String>),
}

/// A running program in a PTY.
struct Process {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Process {
    fn spawn(
        side: Side,
        run: u64,
        argv: &[String],
        size: PtySize,
        tx: mpsc::Sender<PtyEvent>,
    ) -> Result<Self> {
        let pair = native_pty_system().openpty(size).context("open PTY")?;
        let mut cmd = CommandBuilder::new(&argv[0]);
        cmd.args(&argv[1..]);
        // The panes' terminal is this program's vt100 emulator, an xterm with 256 colours.
        cmd.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("start '{}'", argv[0]))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().context("clone PTY reader")?;
        let writer = pair.master.take_writer().context("take PTY writer")?;
        thread::spawn(move || {
            let mut buf = [0_u8; 8192];
            let error = loop {
                match reader.read(&mut buf) {
                    Ok(0) => break None,
                    Ok(n) => {
                        if tx
                            .send(PtyEvent::Output(side, run, buf[..n].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                    // EIO is how Linux reports the last writer closing the PTY.
                    Err(err) if err.raw_os_error() == Some(libc::EIO) => break None,
                    Err(err) => break Some(err.to_string()),
                }
            };
            let _ = tx.send(PtyEvent::Closed(side, run, error));
        });
        Ok(Self {
            master: pair.master,
            writer,
            child,
        })
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    /// Stops the program; it may have exited already.
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Pane {
    side: Side,
    argv: Vec<String>,
    size: PtySize,
    parser: vt100::Parser,
    process: Option<Process>,
    /// Counts starts, so output from a stopped run is ignored.
    run: u64,
    /// Why the program isn't running, once it isn't.
    ended: Option<String>,
}

impl Pane {
    fn new(side: Side, argv: Vec<String>, size: PtySize) -> Self {
        Self {
            side,
            argv,
            size,
            parser: vt100::Parser::new(size.rows, size.cols, 0),
            process: None,
            run: 0,
            ended: None,
        }
    }

    fn start(&mut self, tx: &mpsc::Sender<PtyEvent>) {
        if let Some(mut process) = self.process.take() {
            process.stop();
        }
        self.run += 1;
        self.parser = vt100::Parser::new(self.size.rows, self.size.cols, 0);
        match Process::spawn(self.side, self.run, &self.argv, self.size, tx.clone()) {
            Ok(process) => {
                self.process = Some(process);
                self.ended = None;
            }
            Err(err) => self.ended = Some(format!("{err:#}")),
        }
    }

    fn on_event(&mut self, event: PtyEvent) {
        match event {
            PtyEvent::Output(_, run, bytes) if run == self.run => self.parser.process(&bytes),
            PtyEvent::Closed(_, run, error) if run == self.run => {
                let status = self
                    .process
                    .as_mut()
                    .and_then(|process| process.child.try_wait().ok().flatten());
                self.ended = Some(match (error, status) {
                    (Some(error), _) => format!("read error: {error}"),
                    (None, Some(status)) if !status.success() => {
                        format!("exit {}", status.exit_code())
                    }
                    _ => "exited".to_string(),
                });
                if let Some(mut process) = self.process.take() {
                    process.stop();
                }
            }
            _ => {}
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        if let Some(process) = self.process.as_mut() {
            if let Err(err) = process.write(bytes) {
                self.ended = Some(format!("write error: {err}"));
            }
        }
    }

    fn resize(&mut self, area: Rect) {
        self.size = pty_size_for_area(area);
        self.parser.set_size(self.size.rows, self.size.cols);
        if let Some(process) = self.process.as_mut() {
            if let Err(err) = process.master.resize(self.size) {
                self.ended = Some(format!("resize error: {err:#}"));
            }
        }
    }

    fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            process.stop();
        }
    }
}

fn run_smoke(args: &Args) -> Result<()> {
    let right_program = &args.panes[1][0];
    let (tx, rx) = mpsc::channel::<PtyEvent>();
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let right_probe = format!(
        "if command -v \"$1\" >/dev/null 2>&1; then printf '{SMOKE_RIGHT_MARKER}\\n'; else printf '__LEVITATE_SPLIT_RIGHT_MISSING__\\n'; fi; sleep 0.2"
    );
    let right_argv = [
        "/bin/sh".to_string(),
        "-c".to_string(),
        right_probe,
        "_".to_string(),
        right_program.clone(),
    ];
    let mut left = Process::spawn(Side::Left, 0, &args.panes[0], size, tx.clone())?;
    let mut right = Process::spawn(Side::Right, 0, &right_argv, size, tx)?;
    left.write(format!("printf '{SMOKE_LEFT_MARKER}\\n'\r").as_bytes())
        .context("type into the left pane")?;

    let deadline = Instant::now() + SMOKE_TIMEOUT;
    let mut output = [String::new(), String::new()];
    let markers = [SMOKE_LEFT_MARKER, SMOKE_RIGHT_MARKER];
    let seen =
        |output: &[String; 2], side: Side| output[side.index()].contains(markers[side.index()]);
    while Instant::now() < deadline && !(seen(&output, Side::Left) && seen(&output, Side::Right)) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(PtyEvent::Output(side, _, bytes)) => {
                output[side.index()].push_str(&String::from_utf8_lossy(&bytes))
            }
            Ok(PtyEvent::Closed(..)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    left.stop();
    right.stop();

    let (left_ok, right_ok) = (seen(&output, Side::Left), seen(&output, Side::Right));
    if left_ok && right_ok {
        println!(
            "split-smoke:ok left={} right={right_program}",
            args.panes[0][0]
        );
        return Ok(());
    }
    bail!(
        "split-smoke: failed (left_ok={left_ok}, right_ok={right_ok})\nleft_output:\n{}\nright_output:\n{}",
        tail(&output[0]),
        tail(&output[1])
    )
}

fn tail(s: &str) -> &str {
    const MAX: usize = 300;
    let mut start = s.len().saturating_sub(MAX);
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

fn run_ui(args: &Args) -> Result<()> {
    let _restore = TerminalRestore;
    install_panic_hook();
    enable_raw_mode().context("enable raw mode")?;
    execute!(io::stdout(), EnterAlternateScreen).context("enter alternate screen")?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).context("create terminal backend")?;
    terminal.clear().context("clear terminal")?;

    let (columns, rows) = size().context("read terminal size")?;
    let areas = screen_layout(Rect::new(0, 0, columns, rows)).panes;
    let (tx, rx) = mpsc::channel::<PtyEvent>();
    let mut panes = [Side::Left, Side::Right].map(|side| {
        let mut pane = Pane::new(
            side,
            args.panes[side.index()].clone(),
            pty_size_for_area(areas[side.index()]),
        );
        pane.start(&tx);
        pane
    });
    let mut focus = Side::Left;

    let mut dirty = true;
    loop {
        while let Ok(event) = rx.try_recv() {
            let side = match &event {
                PtyEvent::Output(side, ..) | PtyEvent::Closed(side, ..) => *side,
            };
            panes[side.index()].on_event(event);
            dirty = true;
        }
        if dirty {
            terminal
                .draw(|frame| draw(frame, &panes, focus))
                .context("draw")?;
            dirty = false;
        }
        if !event::poll(INPUT_WAIT).context("poll terminal events")? {
            continue;
        }
        dirty = true;
        match event::read().context("read terminal event")? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match host_key(&key) {
                Some(HostKey::Quit) => break,
                Some(HostKey::Switch) => focus = focus.other(),
                None => {
                    let pane = &mut panes[focus.index()];
                    if pane.process.is_none() && key.code == KeyCode::Enter {
                        pane.start(&tx);
                    } else if let Some(bytes) = key_bytes(key) {
                        pane.write(&bytes);
                    }
                }
            },
            Event::Paste(text) => panes[focus.index()].write(text.as_bytes()),
            Event::Resize(columns, rows) => {
                let areas = screen_layout(Rect::new(0, 0, columns, rows)).panes;
                for pane in &mut panes {
                    pane.resize(areas[pane.side.index()]);
                }
            }
            _ => {}
        }
    }
    for pane in &mut panes {
        pane.stop();
    }
    Ok(())
}

/// Compared as the byte the key sends: terminals report some control characters
/// under another name (0x1D, Ctrl-], arrives as Ctrl-5).
fn host_key(key: &KeyEvent) -> Option<HostKey> {
    let bytes = key_bytes(*key)?;
    HOST_KEYS
        .iter()
        .find(|(_, ch, _, _)| control_byte(*ch).is_some_and(|byte| bytes == [byte]))
        .map(|(host_key, ..)| *host_key)
}

/// The byte an xterm sends for Ctrl-`c`.
fn control_byte(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        ' ' | '@' | '2' => Some(0),
        c @ '3'..='7' => Some(c as u8 - b'3' + 0x1b),
        '8' | '?' => Some(0x7f),
        c @ ('a'..='z' | '[' | '\\' | ']' | '^' | '_') => Some(c as u8 & 0x1f),
        _ => None,
    }
}

fn draw(frame: &mut Frame, panes: &[Pane; 2], focus: Side) {
    let layout = screen_layout(frame.area());
    for pane in panes {
        let area = layout.panes[pane.side.index()];
        let focused = pane.side == focus;
        let mut title = pane.side.title().to_string();
        if let Some(reason) = &pane.ended {
            title.push_str(&format!(" ({reason}) Enter restarts"));
        }
        let border = if focused {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border);
        let inner = block.inner(area);
        let screen = pane.parser.screen();
        frame.render_widget(
            Paragraph::new(Text::from(screen_lines(screen, inner.height, inner.width)))
                .block(block),
            area,
        );
        if focused && pane.process.is_some() && !screen.hide_cursor() {
            let (row, col) = screen.cursor_position();
            if row < inner.height && col < inner.width {
                frame.set_cursor_position(Position::new(inner.x + col, inner.y + row));
            }
        }
    }
    let footer = HOST_KEYS
        .iter()
        .map(|(_, _, key, action)| format!("{key} {action}"))
        .collect::<Vec<_>>()
        .join(" | ");
    frame.render_widget(
        Paragraph::new(format!("{} pane | {footer}", focus.title()))
            .style(Style::default().fg(Color::DarkGray)),
        layout.footer,
    );
}

fn screen_lines(screen: &vt100::Screen, max_rows: u16, max_cols: u16) -> Vec<Line<'static>> {
    let (rows, cols) = screen.size();
    let cols = cols.min(max_cols);
    (0..rows.min(max_rows))
        .map(|row| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut run = String::new();
            let mut run_style = None;
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let style = cell_style(cell);
                if run_style != Some(style) {
                    if let Some(previous) = run_style {
                        spans.push(Span::styled(std::mem::take(&mut run), previous));
                    }
                    run_style = Some(style);
                }
                match cell.contents() {
                    text if text.is_empty() => run.push(' '),
                    text => run.push_str(&text),
                }
            }
            if let Some(style) = run_style {
                spans.push(Span::styled(run, style));
            }
            Line::from(spans)
        })
        .collect()
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut fg = color(cell.fgcolor());
    let mut bg = color(cell.bgcolor());
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    let mut style = Style::default();
    if let Some(fg) = fg {
        style = style.fg(fg);
    }
    if let Some(bg) = bg {
        style = style.bg(bg);
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

fn color(color: vt100::Color) -> Option<Color> {
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

/// The bytes an xterm sends for `key`. Keys an xterm has no sequence for (media
/// keys, lone modifiers) give `None`.
fn key_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    if key.modifiers.contains(KeyModifiers::ALT) {
        out.push(0x1b);
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let csi_modified = |code: char| format!("\x1b[1;5{code}").into_bytes();
    let sequence: Vec<u8> = match key.code {
        KeyCode::Char(c) if ctrl && control_byte(c).is_some() => vec![control_byte(c)?],
        KeyCode::Left if ctrl => csi_modified('D'),
        KeyCode::Right if ctrl => csi_modified('C'),
        KeyCode::Up if ctrl => csi_modified('A'),
        KeyCode::Down if ctrl => csi_modified('B'),
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n @ 1..=4) => vec![0x1b, b'O', b'P' + (n - 1)],
        KeyCode::F(n @ 5..=12) => {
            let code = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)];
            format!("\x1b[{code}~").into_bytes()
        }
        _ => return None,
    };
    out.extend(sequence);
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
    let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
}

struct TerminalRestore;

impl Drop for TerminalRestore {
    fn drop(&mut self) {
        restore_terminal();
    }
}
