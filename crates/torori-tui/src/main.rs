//! torori-tui — thin bin: parse args, build the real facade,
//! run. Full-screen ratatui when stdout is a TTY; the non-TTY
//! line mode (same execute path) otherwise or with `--line`.
//!
//! Usage: `torori-tui [--data <dir>] [--root <dir>]
//! [--triggers <triggers.toml>] [--line]`
//! Line commands: see `help` in line mode.
#![forbid(unsafe_code)]

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use torori::state::Paths;
use torori_tui::app::{App, Mode, Screen};
use torori_tui::facade::LibFacade;
use torori_tui::triggers::TriggerEngine;

struct Cli {
    data: Option<PathBuf>,
    root: Option<PathBuf>,
    triggers: Option<PathBuf>,
    line: bool,
}

fn parse_cli(args: &[String]) -> Cli {
    let mut cli = Cli {
        data: None,
        root: None,
        triggers: None,
        line: false,
    };
    let mut argv = args.iter();
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--data" => cli.data = argv.next().map(PathBuf::from),
            "--root" => cli.root = argv.next().map(PathBuf::from),
            "--triggers" => cli.triggers = argv.next().map(PathBuf::from),
            "--line" => cli.line = true,
            other => eprintln!("torori-tui: ignoring unknown arg {other}"),
        }
    }
    cli
}

fn build_app(cli: &Cli) -> Result<App, String> {
    let engine = match &cli.triggers {
        Some(path) => TriggerEngine::load_file(path)?,
        None => TriggerEngine::embedded_default()?,
    };
    let facade = LibFacade::load(Paths::resolve(cli.data.clone(), cli.root.clone()))
        .map_err(|e| e.to_string())?;
    let mut app = App::new(Box::new(facade), engine);
    app.refresh_overview();
    Ok(app)
}

fn run_line(app: &mut App) -> ExitCode {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            return ExitCode::FAILURE;
        };
        for out in app.handle_line(&line) {
            if writeln!(stdout, "{out}").is_err() {
                return ExitCode::FAILURE;
            }
        }
        if app.should_quit {
            break;
        }
    }
    ExitCode::SUCCESS
}

fn run_fullscreen(app: &mut App) -> ExitCode {
    use crossterm::event::{Event, KeyCode};
    let mut terminal = ratatui::init();
    let mut result = ExitCode::SUCCESS;
    loop {
        if terminal
            .draw(|frame| torori_tui::screens::render(frame, app))
            .is_err()
        {
            result = ExitCode::FAILURE;
            break;
        }
        match crossterm::event::read() {
            Ok(Event::Key(key)) => match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('1') => app.screen = Screen::Overview,
                KeyCode::Char('2') => app.screen = Screen::Workflows,
                KeyCode::Char('3') => app.screen = Screen::Log,
                KeyCode::Char('4') => app.screen = Screen::Fleet,
                KeyCode::Char('m') => {
                    app.mode = match app.mode {
                        Mode::Plan => Mode::Auto,
                        Mode::Auto => Mode::Plan,
                    };
                }
                KeyCode::Char('a') => {
                    let _ = app.approve_staged();
                }
                KeyCode::Char('x') => app.abort_staged(),
                KeyCode::Char('r') => app.refresh_overview(),
                _ => {}
            },
            Ok(_) => {}
            Err(_) => {
                result = ExitCode::FAILURE;
                break;
            }
        }
        if app.should_quit {
            break;
        }
    }
    ratatui::restore();
    result
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = parse_cli(&args);
    let mut app = match build_app(&cli) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("torori-tui: {e}");
            return ExitCode::FAILURE;
        }
    };
    if cli.line || !std::io::stdout().is_terminal() {
        return run_line(&mut app);
    }
    run_fullscreen(&mut app)
}
