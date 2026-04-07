use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::broadcast;

use crate::bundler::{create_bundle, sanitize_filename, BundleOptions};
use crate::cache::MemoryCache;
use crate::config::{Credential, ProfileStore, SharedProfileStore, StoreTarget};
use crate::server::state::LiveReloadMessage;
use crate::stats::SharedStats;

// ── Bundle status ─────────────────────────────────────────────────────────────

/// Current state of a TUI-triggered bundle operation.
#[derive(Clone)]
pub enum BundleStatus {
    Idle,
    Running { started: Instant },
    Done { elapsed: Duration, file_count: usize, size_mb: f64, path: String },
    Error(String),
}

pub type SharedBundleStatus = Arc<Mutex<BundleStatus>>;

// ── TUI mode ──────────────────────────────────────────────────────────────────

/// Which interactive mode the TUI is in.
pub enum TuiMode {
    /// Normal server-monitoring mode.
    Normal,
    /// "Add credential" modal. `focus` is the active field (0=name, 1=token, 2=api_host).
    AddCred {
        name: String,
        token: String,
        api_host: String,
        focus: u8,
    },
    /// "Add store" modal. `focus` is the active field (0=name, 1=url, 2=port).
    AddStore {
        name: String,
        url: String,
        port: String,
        focus: u8,
    },
}

// ── Static server info ────────────────────────────────────────────────────────

/// Static information about the running server, passed into the TUI at startup.
#[derive(Clone)]
pub struct TuiInfo {
    pub store_url: String,
    pub local_url: String,
    pub theme_path: String,
    pub variation: String,
    pub caching: bool,
    pub theme_name: String,
    pub theme_version: String,
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Entry point called from `spawn_blocking`. Owns the terminal for its lifetime.
pub fn run_tui(
    info: TuiInfo,
    stats: SharedStats,
    cache: Arc<tokio::sync::RwLock<MemoryCache>>,
    live_reload_tx: broadcast::Sender<LiveReloadMessage>,
    profiles: SharedProfileStore,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let result = run_loop(&mut terminal, info, stats, cache, live_reload_tx, profiles);

    // Always restore terminal even if run_loop errored
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

// ── Main loop ─────────────────────────────────────────────────────────────────

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    info: TuiInfo,
    stats: SharedStats,
    cache: Arc<tokio::sync::RwLock<MemoryCache>>,
    live_reload_tx: broadcast::Sender<LiveReloadMessage>,
    profiles: SharedProfileStore,
) -> Result<()> {
    let tick = Duration::from_millis(250);
    let mut last_tick = Instant::now();
    let bundle_status: SharedBundleStatus = Arc::new(Mutex::new(BundleStatus::Idle));
    let theme_dir = PathBuf::from(&info.theme_path);
    let mut mode = TuiMode::Normal;

    // We need this to call async cache.clear() from the blocking thread
    let rt_handle = tokio::runtime::Handle::current();

    loop {
        // ── Draw ──────────────────────────────────────────────────────────────
        {
            let s = stats.lock().unwrap();
            let bs = bundle_status.lock().unwrap().clone();
            let p = profiles.lock().unwrap();
            terminal.draw(|f| super::ui::render(f, &info, &s, &bs, &p, &mode))?;
        }

        // ── Event polling ─────────────────────────────────────────────────────
        let timeout = tick
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Each handler returns Some(new_mode) to transition, None to stay.
                    let mode_change: Option<TuiMode> = match &mut mode {
                        TuiMode::AddCred { name, token, api_host, focus } => {
                            handle_add_cred_key(
                                key.code, key.modifiers,
                                name, token, api_host, focus,
                                &profiles, &theme_dir,
                            )
                        }
                        TuiMode::AddStore { name, url, port, focus } => {
                            handle_add_store_key(
                                key.code, key.modifiers,
                                name, url, port, focus,
                                &profiles, &theme_dir,
                            )
                        }
                        TuiMode::Normal => {
                            handle_normal_key(
                                key.code, key.modifiers,
                                &info, &stats, &cache, &live_reload_tx,
                                &bundle_status, &profiles, &theme_dir, &rt_handle,
                            )
                        }
                    };
                    if let Some(new_mode) = mode_change {
                        mode = new_mode;
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick {
            last_tick = Instant::now();
        }
    }
}

// ── Key handlers ──────────────────────────────────────────────────────────────

fn handle_normal_key(
    code: KeyCode,
    _mods: KeyModifiers,
    info: &TuiInfo,
    stats: &SharedStats,
    cache: &Arc<tokio::sync::RwLock<MemoryCache>>,
    live_reload_tx: &broadcast::Sender<LiveReloadMessage>,
    bundle_status: &SharedBundleStatus,
    profiles: &SharedProfileStore,
    theme_dir: &PathBuf,
    rt_handle: &tokio::runtime::Handle,
) -> Option<TuiMode> {
    match code {
        // ── App control ───────────────────────────────────────────────────────
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
            std::process::exit(0);
        }
        KeyCode::Char('c') => {
            rt_handle.block_on(async {
                cache.write().await.clear();
            });
        }
        KeyCode::Char('o') => {
            let _ = open::that(&info.local_url);
        }
        KeyCode::Char('r') => {
            let _ = live_reload_tx.send(LiveReloadMessage::FullReload);
        }

        // ── Bundle ────────────────────────────────────────────────────────────
        KeyCode::Char('b') | KeyCode::Char('B') => {
            let can_bundle = !matches!(
                *bundle_status.lock().unwrap(),
                BundleStatus::Running { .. }
            );
            if can_bundle {
                *bundle_status.lock().unwrap() = BundleStatus::Running {
                    started: Instant::now(),
                };
                let bs = bundle_status.clone();
                let stats_clone = stats.clone();
                let theme_path = PathBuf::from(&info.theme_path);
                let theme_name = info.theme_name.clone();
                let theme_version = info.theme_version.clone();
                std::thread::spawn(move || {
                    let safe_name = sanitize_filename(&theme_name);
                    let safe_version = sanitize_filename(&theme_version);
                    let zip_name = if safe_version.is_empty() {
                        format!("{}.zip", safe_name)
                    } else {
                        format!("{}-{}.zip", safe_name, safe_version)
                    };
                    let output_path = theme_path.join(&zip_name);
                    stats_clone.lock().unwrap().record_build_event(
                        &format!("Bundling → {}", output_path.display()),
                        "",
                    );
                    let opts = BundleOptions {
                        theme_path,
                        output_path: output_path.clone(),
                        source_maps: false,
                    };
                    let start = Instant::now();
                    match create_bundle(&opts) {
                        Ok(result) => {
                            let elapsed = start.elapsed();
                            let size_mb = result.size_bytes as f64 / 1_048_576.0;
                            stats_clone.lock().unwrap().record_build_event(
                                &format!(
                                    "Bundle done  {} files  {:.2} MB",
                                    result.file_count, size_mb
                                ),
                                &format!("{:.2?}", elapsed),
                            );
                            *bs.lock().unwrap() = BundleStatus::Done {
                                elapsed,
                                file_count: result.file_count,
                                size_mb,
                                path: output_path.display().to_string(),
                            };
                        }
                        Err(e) => {
                            let msg = format!("{:#}", e);
                            stats_clone.lock().unwrap().record_build_event(
                                &format!("Bundle FAILED: {}", msg),
                                "",
                            );
                            *bs.lock().unwrap() = BundleStatus::Error(msg);
                        }
                    }
                });
            }
        }

        // ── Profile navigation ────────────────────────────────────────────────
        KeyCode::Up => {
            let mut p = profiles.lock().unwrap();
            p.prev_credential();
            let _ = p.save(theme_dir);
        }
        KeyCode::Down => {
            let mut p = profiles.lock().unwrap();
            p.next_credential();
            let _ = p.save(theme_dir);
        }
        KeyCode::Left => {
            let mut p = profiles.lock().unwrap();
            p.prev_store();
            let _ = p.save(theme_dir);
        }
        KeyCode::Right => {
            let mut p = profiles.lock().unwrap();
            p.next_store();
            let _ = p.save(theme_dir);
        }

        // ── Delete active profile/store ───────────────────────────────────────
        KeyCode::Delete => {
            let mut p = profiles.lock().unwrap();
            p.remove_active_credential();
            let _ = p.save(theme_dir);
        }
        KeyCode::Backspace => {
            // Shift+Backspace removes active store
        }

        // ── Add new ───────────────────────────────────────────────────────────
        KeyCode::Char('n') => {
            return Some(TuiMode::AddCred {
                name: String::new(),
                token: String::new(),
                api_host: "api.bigcommerce.com".to_string(),
                focus: 0,
            });
        }
        KeyCode::Char('N') => {
            return Some(TuiMode::AddStore {
                name: String::new(),
                url: String::new(),
                port: "3000".to_string(),
                focus: 0,
            });
        }

        _ => {}
    }
    None
}

fn handle_add_cred_key(
    code: KeyCode,
    _mods: KeyModifiers,
    name: &mut String,
    token: &mut String,
    api_host: &mut String,
    focus: &mut u8,
    profiles: &SharedProfileStore,
    theme_dir: &PathBuf,
) -> Option<TuiMode> {
    match code {
        KeyCode::Esc => return Some(TuiMode::Normal),
        KeyCode::Tab => {
            *focus = (*focus + 1) % 3;
        }
        KeyCode::BackTab => {
            *focus = (*focus + 2) % 3; // wrapping sub 1
        }
        KeyCode::Enter => {
            if *focus < 2 {
                *focus += 1;
            } else {
                let n = name.trim().to_string();
                let t = token.trim().to_string();
                let h = if api_host.trim().is_empty() {
                    "api.bigcommerce.com".to_string()
                } else {
                    api_host.trim().to_string()
                };
                if !n.is_empty() && !t.is_empty() {
                    let mut p = profiles.lock().unwrap();
                    p.add_credential(Credential { name: n, access_token: t, api_host: h });
                    let _ = p.save(theme_dir);
                }
                return Some(TuiMode::Normal);
            }
        }
        KeyCode::Backspace => match *focus {
            0 => { name.pop(); }
            1 => { token.pop(); }
            2 => { api_host.pop(); }
            _ => {}
        },
        KeyCode::Char(c) => match *focus {
            0 => name.push(c),
            1 => token.push(c),
            2 => api_host.push(c),
            _ => {}
        },
        _ => {}
    }
    None
}

fn handle_add_store_key(
    code: KeyCode,
    _mods: KeyModifiers,
    name: &mut String,
    url: &mut String,
    port: &mut String,
    focus: &mut u8,
    profiles: &SharedProfileStore,
    theme_dir: &PathBuf,
) -> Option<TuiMode> {
    match code {
        KeyCode::Esc => return Some(TuiMode::Normal),
        KeyCode::Tab => {
            *focus = (*focus + 1) % 3;
        }
        KeyCode::BackTab => {
            *focus = (*focus + 2) % 3;
        }
        KeyCode::Enter => {
            if *focus < 2 {
                *focus += 1;
            } else {
                let n = name.trim().to_string();
                let u = url.trim().to_string();
                let p_val: u16 = port.trim().parse().unwrap_or(3000);
                if !n.is_empty() && !u.is_empty() {
                    let mut p = profiles.lock().unwrap();
                    p.add_store(StoreTarget { name: n, url: u, port: p_val });
                    let _ = p.save(theme_dir);
                }
                return Some(TuiMode::Normal);
            }
        }
        KeyCode::Backspace => match *focus {
            0 => { name.pop(); }
            1 => { url.pop(); }
            2 => { port.pop(); }
            _ => {}
        },
        KeyCode::Char(c) => match *focus {
            0 => name.push(c),
            1 => url.push(c),
            2 => port.push(c),
            _ => {}
        },
        _ => {}
    }
    None
}
