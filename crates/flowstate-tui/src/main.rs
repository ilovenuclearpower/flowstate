mod app;
mod components;

use std::io;
use std::path::PathBuf;
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

// ---- Credential persistence ----

fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("flowstate")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("flowstate")
    } else {
        PathBuf::from(".config").join("flowstate")
    }
}

fn credentials_path() -> PathBuf {
    config_dir().join("tui-credentials")
}

/// Load saved config (server_url, api_key) from the credentials file.
fn load_saved_config() -> Option<(Option<String>, Option<String>)> {
    let path = credentials_path();
    let content = std::fs::read_to_string(&path).ok()?;
    let mut server_url = None;
    let mut api_key = None;
    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("server_url=") {
            if !val.is_empty() {
                server_url = Some(val.to_string());
            }
        } else if let Some(val) = line.strip_prefix("api_key=") {
            if !val.is_empty() {
                api_key = Some(val.to_string());
            }
        }
    }
    Some((server_url, api_key))
}

fn load_saved_key() -> Option<String> {
    load_saved_config().and_then(|(_, key)| key)
}

fn load_saved_server_url() -> Option<String> {
    load_saved_config().and_then(|(url, _)| url)
}

fn save_config(server_url: &str, api_key: &str) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = credentials_path();
    let content = format!("server_url={server_url}\napi_key={api_key}\n");
    std::fs::write(&path, &content)?;
    // Set 0o600 permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Parse CLI: flowstate [--server URL] [--api-key KEY]
    // No args or "up" → spawn server locally then run TUI
    // --server URL → connect to existing server
    // --api-key KEY → authenticate with API key (also reads FLOWSTATE_API_KEY env var)
    let cli_server = args
        .iter()
        .position(|a| a == "--server")
        .and_then(|pos| args.get(pos + 1).cloned());

    let (server_url, mut child) = if let Some(url) = cli_server.clone() {
        (url, None)
    } else if let Some(saved_url) = load_saved_server_url() {
        // Use saved server URL if no --server flag
        (saved_url, None)
    } else {
        // Spawn flowstate-server as child process
        let child = spawn_server()?;
        (DEFAULT_URL.to_string(), Some(child))
    };

    // Read API key: --api-key flag > FLOWSTATE_API_KEY env > saved credentials
    let cli_api_key = args
        .iter()
        .position(|a| a == "--api-key")
        .and_then(|pos| args.get(pos + 1).cloned());

    let api_key = cli_api_key
        .or_else(|| {
            std::env::var("FLOWSTATE_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
        })
        .or_else(load_saved_key);

    // Wait for server to be ready
    let mut service = match api_key {
        Some(key) => BlockingHttpService::with_api_key(&server_url, key),
        None => BlockingHttpService::new(&server_url),
    };
    wait_for_server(&service)?;

    // Setup wizard: check if we need to initialize the server
    run_setup_check(&mut service, &server_url)?;

    // Run TUI
    let result = run_tui(service);

    // Cleanup: kill server if we spawned it
    if let Some(ref mut child) = child {
        let _ = child.kill();
        let _ = child.wait();
    }

    result
}

/// Check if setup is needed and run the wizard if so.
fn run_setup_check(service: &mut BlockingHttpService, server_url: &str) -> Result<()> {
    // Only run setup wizard if no API key is configured
    // (if we already have one from --api-key, env, or saved, skip)
    match service.setup_status() {
        Ok(status) if status.setup_needed => {
            run_setup_wizard(service, server_url)?;
        }
        Ok(_) => {
            // Setup not needed — server has auth configured.
            // If we have no key but server requires auth, the TUI
            // will get 401 errors and the user will need to provide one.
        }
        Err(_) => {
            // Server might not support the setup endpoint (old version),
            // or we already have auth. Continue normally.
        }
    }
    Ok(())
}

fn run_setup_wizard(service: &mut BlockingHttpService, server_url: &str) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Welcome screen
    terminal.draw(|frame| {
        let area = frame.area();
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(" Flowstate Setup ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let text = vec![
            Line::from(""),
            Line::from("Welcome to Flowstate!").style(Style::default().fg(Color::Cyan).bold()),
            Line::from(""),
            Line::from("No API keys are configured on this server."),
            Line::from("This wizard will generate your admin API key."),
            Line::from(""),
            Line::from("Press Enter to continue...").style(Style::default().fg(Color::DarkGray)),
        ];
        let para =
            ratatui::widgets::Paragraph::new(text).alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(para, inner);
    })?;

    // Wait for Enter
    loop {
        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Enter {
                break;
            }
            if key.code == KeyCode::Char('q')
                || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                bail!("setup cancelled");
            }
        }
    }

    // Call setup_init
    let result = service.setup_init("admin");
    match result {
        Ok(init_resp) => {
            // Save credentials
            let save_result = save_config(server_url, &init_resp.api_key);
            let saved_path = credentials_path();

            // Display key
            terminal.draw(|frame| {
                let area = frame.area();
                let block = ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title(" Flowstate Setup — Complete ");
                let inner = block.inner(area);
                frame.render_widget(block, area);

                let mut text = vec![
                    Line::from(""),
                    Line::from("Your admin API key has been generated.")
                        .style(Style::default().fg(Color::Green).bold()),
                    Line::from(""),
                    Line::from(format!("  {}", init_resp.api_key))
                        .style(Style::default().fg(Color::Yellow)),
                    Line::from(""),
                ];
                if save_result.is_ok() {
                    text.push(Line::from(format!(
                        "Key saved to: {}",
                        saved_path.display()
                    )));
                    text.push(Line::from(""));
                    text.push(Line::from(
                        "On next launch, this key will be loaded automatically.",
                    ));
                } else {
                    text.push(
                        Line::from("Warning: could not save key to disk.")
                            .style(Style::default().fg(Color::Red)),
                    );
                    text.push(Line::from(
                        "Set FLOWSTATE_API_KEY env var or use --api-key on next launch.",
                    ));
                }
                text.push(Line::from(""));
                text.push(Line::from(
                    "To connect runners, generate runner keys from the Ops tab (press 2).",
                ));
                text.push(Line::from(""));
                text.push(
                    Line::from("Press Enter to continue...")
                        .style(Style::default().fg(Color::DarkGray)),
                );

                let para = ratatui::widgets::Paragraph::new(text)
                    .alignment(ratatui::layout::Alignment::Center);
                frame.render_widget(para, inner);
            })?;

            // Wait for Enter
            loop {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Enter {
                        break;
                    }
                }
            }

            // Arm the service with the new key
            service.set_api_key(init_resp.api_key);
        }
        Err(e) => {
            // Display error and exit
            terminal.draw(|frame| {
                let area = frame.area();
                let block = ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title(" Flowstate Setup — Error ");
                let inner = block.inner(area);
                frame.render_widget(block, area);

                let text = vec![
                    Line::from(""),
                    Line::from("Setup failed:").style(Style::default().fg(Color::Red).bold()),
                    Line::from(""),
                    Line::from(format!("  {e}")),
                    Line::from(""),
                    Line::from("Another user may have already set up this server."),
                    Line::from("Use --api-key or set FLOWSTATE_API_KEY to connect."),
                    Line::from(""),
                    Line::from("Press Enter to exit...")
                        .style(Style::default().fg(Color::DarkGray)),
                ];
                let para = ratatui::widgets::Paragraph::new(text)
                    .alignment(ratatui::layout::Alignment::Center);
                frame.render_widget(para, inner);
            })?;

            loop {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Enter {
                        break;
                    }
                }
            }

            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            bail!("setup failed: {e}");
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
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
                    if key.code == KeyCode::Char('q') && app.should_quit_on_q() {
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
            // q quits unless we're in an input mode or on Ops tab
            if key.code == KeyCode::Char('q') && app.should_quit_on_q() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // These tests mutate XDG_CONFIG_HOME (a global env var), so they must not run in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn load_saved_key_returns_none_when_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        let key = load_saved_key();
        assert!(key.is_none());
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());

        save_config("http://localhost:3710", "fs_testkey123").unwrap();

        let (url, key) = load_saved_config().unwrap();
        assert_eq!(url.unwrap(), "http://localhost:3710");
        assert_eq!(key.unwrap(), "fs_testkey123");

        // Also test individual helpers
        assert_eq!(load_saved_key().unwrap(), "fs_testkey123");
        assert_eq!(load_saved_server_url().unwrap(), "http://localhost:3710");

        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn credentials_file_permissions() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());

        save_config("http://localhost:3710", "fs_secret").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = tmp.path().join("flowstate").join("tui-credentials");
            let perms = std::fs::metadata(&path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }

        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
