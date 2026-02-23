mod app;
mod components;

use std::io;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use flowstate_service::BlockingHttpService;
use ratatui::prelude::*;

use app::App;

const DEFAULT_PORT: u16 = 3710;
const DEFAULT_URL: &str = "http://127.0.0.1:3710";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Parse CLI: flowstate [--server URL] [--api-key KEY]
    // No args or "up" → spawn server locally then run TUI
    // --server URL → connect to existing server
    // --api-key KEY → authenticate with API key (also reads FLOWSTATE_API_KEY env var)
    let (server_url, mut child) = if let Some(pos) = args.iter().position(|a| a == "--server") {
        let url = args
            .get(pos + 1)
            .context("--server requires a URL argument")?;
        (url.clone(), None)
    } else {
        // Spawn flowstate-server as child process
        let child = spawn_server()?;
        (DEFAULT_URL.to_string(), Some(child))
    };

    // Read API key from --api-key flag or FLOWSTATE_API_KEY env var
    let api_key = if let Some(pos) = args.iter().position(|a| a == "--api-key") {
        args.get(pos + 1)
            .context("--api-key requires a key argument")?
            .clone()
            .into()
    } else {
        std::env::var("FLOWSTATE_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
    };

    // Wait for server to be ready
    let service = match api_key {
        Some(key) => BlockingHttpService::with_api_key(&server_url, key),
        None => BlockingHttpService::new(&server_url),
    };
    wait_for_server(&service)?;

    // Run TUI
    let result = run_tui(service);

    // Cleanup: kill server if we spawned it
    if let Some(ref mut child) = child {
        let _ = child.kill();
        let _ = child.wait();
    }

    result
}

fn spawn_server() -> Result<Child> {
    // Look for flowstate-server binary next to our own binary first,
    // then fall back to PATH
    let self_exe = std::env::current_exe().unwrap_or_default();
    let sibling = self_exe.parent().map(|d| d.join("flowstate-server"));

    let server_bin = if sibling.as_ref().is_some_and(|p| p.exists()) {
        sibling.unwrap()
    } else {
        "flowstate-server".into()
    };

    let child = Command::new(&server_bin)
        .env("FLOWSTATE_BIND", "127.0.0.1")
        .env("FLOWSTATE_PORT", DEFAULT_PORT.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start {}", server_bin.display()))?;

    Ok(child)
}

fn wait_for_server(service: &BlockingHttpService) -> Result<()> {
    let start = Instant::now();
    let timeout = Duration::from_secs(10);

    loop {
        if service.health_check().is_ok() {
            return Ok(());
        }
        if start.elapsed() > timeout {
            bail!(
                "flowstate-server did not become ready within {}s",
                timeout.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn run_tui(service: BlockingHttpService) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, service);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(ref e) = result {
        eprintln!("Error: {e}");
    }

    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    service: BlockingHttpService,
) -> Result<()> {
    let mut app = App::new(service)?;

    loop {
        terminal.draw(|frame| app.render(frame))?;

        // Check for editor request before reading events
        if let Some(ref req) = app.editor_request.clone() {
            open_in_editor(terminal, &req.path)?;
            app.editor_done();
            continue;
        }

        // Use poll with timeout when Claude is running, blocking read otherwise
        if app.needs_polling() {
            if event::poll(Duration::from_secs(2))? {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break;
                    }
                    if key.code == KeyCode::Char('q') && !app.is_input_mode() {
                        break;
                    }
                    app.handle_key(key);
                }
            } else {
                // Timeout — poll the Claude run
                app.poll_claude_run();
            }
        } else if let Event::Key(key) = event::read()? {
            // Ctrl+C always quits
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                break;
            }
            // q quits unless we're in an input mode
            if key.code == KeyCode::Char('q') && !app.is_input_mode() {
                break;
            }
            app.handle_key(key);
        }
    }

    Ok(())
}

/// Leave TUI, open $EDITOR, then restore TUI.
fn open_in_editor(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    path: &std::path::Path,
) -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());

    // Leave TUI mode
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    // Spawn editor, wait for exit
    let status = Command::new(&editor).arg(path).status()?;

    // Restore TUI mode
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.clear()?;

    if !status.success() {
        bail!("editor exited with {status}");
    }

    Ok(())
}
