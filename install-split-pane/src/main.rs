//! install-split: the install screen's terminal host.
//!
//! Two panes side by side with no frame, the install hub on the left and the docs on
//! the right, each running a program in its own PTY, drawn through a vt100 emulator.
//! The pane without the keyboard is drawn dimmer. The bottom row lists the host's
//! keys as keycaps. Every other key goes to the focused pane. A pane whose program
//! exits, or whose PTY fails, stays on screen and the bottom row gives the reason and
//! offers Enter to restart it; the host itself ends only when the user quits. A pane's
//! clipboard writes (OSC 52) go on to the host's own terminal, which the emulator
//! would drop.

use std::{
    env,
    io::{self, IsTerminal, Read, Write},
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
use distro_contract::install_ux::{PANE_TERM, SMOKE_OK};
use install_split_pane::{pane_terminal_size, screen_layout};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame, Terminal,
};

/// How long the smoke check gives both panes to draw.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(20);
/// A pane counts as drawn once its output has been quiet this long.
const SMOKE_QUIET: Duration = Duration::from_millis(500);
/// The console the smoke check lays the screen out on without a terminal: the
/// serial console, the smallest one the screen runs on.
const SMOKE_CONSOLE: (u16, u16) = (80, 24);
/// The longest OSC sequence passed on (a clipboard write carries base64 text).
const OSC_MAX: usize = 1 << 20;
/// How long to wait for a key before looking for pane output again.
const INPUT_WAIT: Duration = Duration::from_millis(30);

/// The host's own keys, all Ctrl-<char>; the footer shows this table. Ctrl-] is free
/// in bash, less and the docs app (Ctrl-G, the obvious pick, is less's file info).
const HOST_KEYS: &[(HostKey, char, &str, &str)] = &[
    (HostKey::Switch, ']', "Ctrl-]", "switch pane"),
    (HostKey::Quit, 'q', "Ctrl-q", "quit"),
];

/// The colours of the key bar, from tui/kit/core/src/theme/palette.ts (the TypeScript
/// theme is not available to this crate): `ink` is the dark surface, `rule` the
/// keycap's cell, `paper` the key's text, `lichen` the labels, `night` the screen
/// surface a dimmed pane fades toward, `sand` a pane's notice.
const INK: Color = Color::Rgb(0x14, 0x22, 0x1a);
const RULE: Color = Color::Rgb(0x3a, 0x4b, 0x40);
const PAPER: Color = Color::Rgb(0xf2, 0xee, 0xe4);
const LICHEN: Color = Color::Rgb(0x9a, 0xa8, 0x9d);
const NIGHT: Color = Color::Rgb(0x0d, 0x17, 0x11);
const SAND: Color = Color::Rgb(0xe4, 0xcb, 0xa6);
/// How far a pane without the keyboard fades toward its background, in percent.
const DIM_PERCENT: u16 = 45;

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
    println!("Runs the left command (the install hub) beside the right one (the docs).");
    println!();
    println!("Options:");
    println!("  --smoke     Start both panes at the screen's pane sizes (this terminal's, or");
    println!("              an 80x24 console's without one) and check that each draws and");
    println!("              keeps running; prints '{SMOKE_OK} ...' when they do.");
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
            Self::Left => "Install",
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

impl PtyEvent {
    fn side(&self) -> Side {
        match self {
            Self::Output(side, ..) | Self::Closed(side, ..) => *side,
        }
    }
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
        cmd.env("TERM", PANE_TERM);
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
            // A failed send means the host has stopped listening: it is shutting down.
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

    /// Stops the program (it may have exited already) and reaps it.
    fn stop(&mut self) -> Result<(), String> {
        match self.child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(err) => return Err(format!("wait failed: {err}")),
        }
        let killed = match self.child.kill() {
            // It exited between the two calls.
            Err(err) if err.raw_os_error() == Some(libc::ESRCH) => Ok(()),
            result => result.map_err(|err| format!("kill failed: {err}")),
        };
        let waited = self.child.wait().map(drop).map_err(|err| format!("wait failed: {err}"));
        killed.and(waited)
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
    /// OSC 52 sequences the program wrote, for the host's terminal.
    clipboard: OscScan,
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
            clipboard: OscScan::default(),
        }
    }

    fn start(&mut self, tx: &mpsc::Sender<PtyEvent>) {
        if let Err(err) = self.stop() {
            self.ended = Some(format!("stopping the last run: {err}"));
            return;
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
            PtyEvent::Output(_, run, bytes) if run == self.run => {
                self.clipboard.scan(&bytes);
                self.parser.process(&bytes);
            }
            PtyEvent::Closed(_, run, error) if run == self.run => {
                let status = self
                    .process
                    .as_mut()
                    .map(|process| process.child.try_wait());
                let reason = match (error, status) {
                    (Some(error), _) => format!("read error: {error}"),
                    (None, Some(Err(err))) => format!("wait failed: {err}"),
                    (None, Some(Ok(Some(status)))) if !status.success() => {
                        format!("exit {}", status.exit_code())
                    }
                    _ => "exited".to_string(),
                };
                self.end(reason);
            }
            _ => {}
        }
    }

    /// The program is done for: stop it, so Enter restarts it, and show why.
    fn end(&mut self, reason: String) {
        self.ended = Some(match self.stop() {
            Ok(()) => reason,
            Err(err) => format!("{reason}; {err}"),
        });
    }

    fn write(&mut self, bytes: &[u8]) {
        if let Some(process) = self.process.as_mut() {
            if let Err(err) = process.write(bytes) {
                self.end(format!("write error: {err}"));
            }
        }
    }

    fn resize(&mut self, area: Rect) {
        self.size = pty_size_for_area(area);
        self.parser.set_size(self.size.rows, self.size.cols);
        if let Some(process) = self.process.as_mut() {
            if let Err(err) = process.master.resize(self.size) {
                self.end(format!("resize error: {err:#}"));
            }
        }
    }

    /// Stops the program if it runs.
    fn stop(&mut self) -> Result<(), String> {
        match self.process.take() {
            Some(mut process) => process.stop(),
            None => Ok(()),
        }
    }
}

/// Picks OSC 52 (clipboard) sequences out of a pane's output, which may split them
/// across reads, for the host to pass on to its own terminal.
#[derive(Default)]
struct OscScan {
    /// The sequence so far: empty, a lone ESC, or an OSC from `ESC ]` on.
    pending: Vec<u8>,
    /// Whole OSC 52 sequences not yet passed on.
    found: Vec<Vec<u8>>,
}

impl OscScan {
    fn scan(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            let in_osc = self.pending.starts_with(b"\x1b]");
            if !in_osc {
                match (self.pending.as_slice(), byte) {
                    (_, 0x1b) => self.pending = vec![byte],
                    ([0x1b], b']') => self.pending.push(byte),
                    _ => self.pending.clear(),
                }
                continue;
            }
            let after_esc = self.pending.last() == Some(&0x1b);
            self.pending.push(byte);
            // BEL or ST (ESC \) ends an OSC; ESC followed by anything else cancels it.
            if byte == 0x07 || (after_esc && byte == b'\\') {
                let sequence = std::mem::take(&mut self.pending);
                if sequence.starts_with(b"\x1b]52;") {
                    self.found.push(sequence);
                }
            } else if (after_esc && byte != 0x1b) || self.pending.len() > OSC_MAX {
                self.pending.clear();
            }
        }
    }

    fn take(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.found)
    }
}

/// Starts both panes as the screen does, at the pane sizes of this terminal (or of
/// the serial console without one), and checks that each program draws and is still
/// running once its output goes quiet.
fn run_smoke(args: &Args) -> Result<()> {
    let (columns, rows) = if io::stdout().is_terminal() {
        size().context("read terminal size")?
    } else {
        SMOKE_CONSOLE
    };
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
    drop(tx);

    let deadline = Instant::now() + SMOKE_TIMEOUT;
    let mut last_output: [Option<Instant>; 2] = [None, None];
    loop {
        let now = Instant::now();
        let settled = |side: usize| {
            panes[side].ended.is_some()
                || last_output[side].is_some_and(|at| now - at >= SMOKE_QUIET)
        };
        if now >= deadline || (settled(0) && settled(1)) {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => {
                let side = event.side().index();
                if matches!(event, PtyEvent::Output(..)) {
                    last_output[side] = Some(Instant::now());
                }
                panes[side].on_event(event);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let mut report = Vec::new();
    let mut failed = false;
    for pane in &mut panes {
        let screen = pane.parser.screen().contents();
        let problem = match (&pane.ended, screen.trim().is_empty()) {
            (Some(reason), _) => Some(format!("ended ({reason})")),
            (None, true) => Some(format!("drew nothing in {}s", SMOKE_TIMEOUT.as_secs())),
            (None, false) => None,
        };
        let stopped = pane.stop();
        let size = format!("{}x{}", pane.size.cols, pane.size.rows);
        match (problem, stopped) {
            (None, Ok(())) => report.push(format!("{}={} ({size})", pane.side.name(), pane.argv[0])),
            (problem, stopped) => {
                failed = true;
                let mut why: Vec<String> = problem.into_iter().collect();
                why.extend(stopped.err());
                report.push(format!(
                    "{} pane `{}` ({size}): {}\nscreen:\n{}",
                    pane.side.name(),
                    pane.argv.join(" "),
                    why.join("; "),
                    tail(screen.trim_end())
                ));
            }
        }
    }
    if failed {
        bail!("split-smoke: failed on a {columns}x{rows} console\n{}", report.join("\n"));
    }
    println!("{SMOKE_OK} {}", report.join(" "));
    Ok(())
}

fn tail(s: &str) -> &str {
    const MAX: usize = 4000;
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
            let pane = &mut panes[event.side().index()];
            pane.on_event(event);
            for sequence in pane.clipboard.take() {
                let backend = terminal.backend_mut();
                backend
                    .write_all(&sequence)
                    .and_then(|()| backend.flush())
                    .context("pass a pane's clipboard write to the terminal")?;
            }
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
    let errors: Vec<String> = panes
        .iter_mut()
        .filter_map(|pane| pane.stop().err().map(|err| format!("{} pane: {err}", pane.side.name())))
        .collect();
    if !errors.is_empty() {
        bail!("stopping the panes: {}", errors.join("; "));
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
        let screen = pane.parser.screen();
        frame.render_widget(
            Paragraph::new(Text::from(screen_lines(
                screen,
                area.height,
                area.width,
                !focused,
            ))),
            area,
        );
        if focused && pane.process.is_some() && !screen.hide_cursor() {
            let (row, col) = screen.cursor_position();
            if row < area.height && col < area.width {
                frame.set_cursor_position(Position::new(area.x + col, area.y + row));
            }
        }
    }
    draw_key_bar(frame, layout.footer, panes);
}

/// The bottom row on the dark surface: the host's keys as keycaps at the right, and
/// at the left what a pane whose program ended says, with Enter to restart it.
fn draw_key_bar(frame: &mut Frame, area: Rect, panes: &[Pane; 2]) {
    let buffer = frame.buffer_mut();
    let surface = Style::default().fg(LICHEN).bg(INK);
    for x in area.left()..area.right() {
        buffer[(x, area.y)].set_symbol(" ").set_style(surface);
    }
    let cap = Style::default().fg(PAPER).bg(RULE);
    let mut keys: Vec<(String, &str)> = Vec::new();
    for (_, _, key, action) in HOST_KEYS {
        keys.push((format!(" {key} "), action));
    }
    let width: usize = keys
        .iter()
        .map(|(cap, action)| cap.chars().count() + 1 + action.chars().count() + 2)
        .sum::<usize>()
        .saturating_sub(2);
    let mut x = area.right().saturating_sub(width as u16).max(area.left());
    let keys_start = x;
    for (index, (key, action)) in keys.iter().enumerate() {
        if index > 0 {
            x = put(buffer, x, area, "  ", surface);
        }
        x = put(buffer, x, area, key, cap);
        x = put(buffer, x, area, &format!(" {action}"), surface);
    }
    let notice: Vec<String> = panes
        .iter()
        .filter_map(|pane| {
            pane.ended
                .as_ref()
                .map(|reason| format!("{} ended ({reason})", pane.side.title()))
        })
        .collect();
    if notice.is_empty() {
        return;
    }
    let room = usize::from(keys_start.saturating_sub(area.left())).saturating_sub(3);
    let mut text = format!("{}.  Enter restarts", notice.join(", "));
    if text.chars().count() > room {
        text = text.chars().take(room.saturating_sub(1)).collect::<String>() + "…";
    }
    if room > 1 {
        put(
            buffer,
            area.left() + 1,
            area,
            &text,
            Style::default().fg(SAND).bg(INK),
        );
    }
}

/// Writes `text` on the bar's row from column `x`, stopping at the bar's end, and
/// returns the column after it.
fn put(
    buffer: &mut ratatui::buffer::Buffer,
    x: u16,
    area: Rect,
    text: &str,
    style: Style,
) -> u16 {
    let room = usize::from(area.right().saturating_sub(x));
    let text: String = text.chars().take(room).collect();
    let (end, _) = buffer.set_stringn(x, area.y, &text, room, style);
    end
}

fn screen_lines(
    screen: &vt100::Screen,
    max_rows: u16,
    max_cols: u16,
    dim: bool,
) -> Vec<Line<'static>> {
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
                let style = cell_style(cell, dim);
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

/// A cell's style; when `dim`, its foreground faded toward its background (the
/// screen surface where the cell sets none; a default foreground counts as paper).
fn cell_style(cell: &vt100::Cell, dim: bool) -> Style {
    let mut fg = color(cell.fgcolor());
    let mut bg = color(cell.bgcolor());
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    if dim {
        let toward = bg.and_then(rgb_of).unwrap_or(rgb_of(NIGHT).unwrap_or_default());
        let from = fg.and_then(rgb_of).unwrap_or(rgb_of(PAPER).unwrap_or_default());
        fg = Some(fade(from, toward));
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

/// `from` moved `DIM_PERCENT` of the way to `toward`.
fn fade(from: (u8, u8, u8), toward: (u8, u8, u8)) -> Color {
    let mix = |a: u8, b: u8| {
        let (a, b) = (i32::from(a), i32::from(b));
        (a + (b - a) * i32::from(DIM_PERCENT) / 100) as u8
    };
    Color::Rgb(
        mix(from.0, toward.0),
        mix(from.1, toward.1),
        mix(from.2, toward.2),
    )
}

/// The RGB of a colour the emulator reports (xterm's palette for indexed ones).
fn rgb_of(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(idx) => Some(match idx {
            0..=15 => {
                const BASE: [(u8, u8, u8); 16] = [
                    (0, 0, 0),
                    (205, 0, 0),
                    (0, 205, 0),
                    (205, 205, 0),
                    (0, 0, 238),
                    (205, 0, 205),
                    (0, 205, 205),
                    (229, 229, 229),
                    (127, 127, 127),
                    (255, 0, 0),
                    (0, 255, 0),
                    (255, 255, 0),
                    (92, 92, 255),
                    (255, 0, 255),
                    (0, 255, 255),
                    (255, 255, 255),
                ];
                BASE[usize::from(idx)]
            }
            16..=231 => {
                let n = idx - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                (level(n / 36), level(n / 6 % 6), level(n % 6))
            }
            _ => {
                let grey = 8 + (idx - 232) * 10;
                (grey, grey, grey)
            }
        }),
        _ => None,
    }
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

/// Leaves raw mode and the alternate screen; a failure is reported on stderr, which
/// is all that is left by then.
fn restore_terminal() {
    if let Err(err) = disable_raw_mode() {
        eprintln!("install-split: leaving raw mode failed: {err}");
    }
    if let Err(err) = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show) {
        eprintln!("install-split: leaving the alternate screen failed: {err}");
    }
}

struct TerminalRestore;

impl Drop for TerminalRestore {
    fn drop(&mut self) {
        restore_terminal();
    }
}
