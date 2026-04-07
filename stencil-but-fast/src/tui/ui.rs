use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::config::ProfileStore;
use crate::stats::{EventKind, ServerStats};

use super::app::{BundleStatus, TuiInfo, TuiMode};

// ── Top-level render ──────────────────────────────────────────────────────────

pub fn render(
    frame: &mut Frame,
    info: &TuiInfo,
    stats: &ServerStats,
    bundle: &BundleStatus,
    profiles: &ProfileStore,
    mode: &TuiMode,
) {
    let area = frame.area();

    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(8),     // content
            Constraint::Length(1),  // bundle status bar
            Constraint::Length(4),  // footer / keybindings (2 inner lines)
        ])
        .split(area);

    render_header(frame, main[0], info);
    render_content(frame, main[1], info, stats, profiles);
    render_bundle_bar(frame, main[2], bundle);
    render_footer(frame, main[3], mode);

    // Modal overlays rendered last (on top)
    match mode {
        TuiMode::AddCred { name, token, api_host, focus } => {
            render_add_cred_modal(frame, area, name, token, api_host, *focus);
        }
        TuiMode::AddStore { name, url, port, focus } => {
            render_add_store_modal(frame, area, name, url, port, *focus);
        }
        TuiMode::Normal => {}
    }
}

// ── Header ────────────────────────────────────────────────────────────────────

fn render_header(frame: &mut Frame, area: Rect, info: &TuiInfo) {
    let line = Line::from(vec![
        Span::styled("● ", Style::default().fg(Color::Green)),
        Span::styled(
            "Stencil Dev Server",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("   {}   ", info.local_url),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(
            format!("Store: {}", truncate(&info.store_url, 50)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .alignment(Alignment::Left);
    frame.render_widget(paragraph, area);
}

// ── Content ───────────────────────────────────────────────────────────────────

fn render_content(
    frame: &mut Frame,
    area: Rect,
    info: &TuiInfo,
    stats: &ServerStats,
    profiles: &ProfileStore,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // top panels
            Constraint::Min(4),    // events log
        ])
        .split(area);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(30),
            Constraint::Percentage(40),
        ])
        .split(chunks[0]);

    render_server_info(frame, top[0], info, stats);
    render_stats(frame, top[1], stats);
    render_profiles(frame, top[2], profiles);
    render_events(frame, chunks[1], stats);
}

fn render_server_info(frame: &mut Frame, area: Rect, info: &TuiInfo, stats: &ServerStats) {
    let caching_text = if info.caching {
        Span::styled("✓ enabled", Style::default().fg(Color::Green))
    } else {
        Span::styled("✗ disabled", Style::default().fg(Color::Red))
    };

    let inner_width = area.width.saturating_sub(16) as usize;

    let lines = vec![
        Line::from(vec![
            Span::styled("  Store:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate(&info.store_url, inner_width),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Local:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(&info.local_url, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("  Theme:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate(&info.theme_path, inner_width),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Variation: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&info.variation, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  Caching:   ", Style::default().fg(Color::DarkGray)),
            caching_text,
        ]),
        Line::from(vec![
            Span::styled("  Uptime:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(stats.uptime_str(), Style::default().fg(Color::White)),
        ]),
    ];

    let block = Block::default()
        .title(" Server Info ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_stats(frame: &mut Frame, area: Rect, stats: &ServerStats) {
    let hit_rate = stats.cache_hit_rate();
    let lines = vec![
        Line::from(vec![
            Span::styled("  Requests:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                stats.requests_total.to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Cache Hits:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}  ({:.1}%)", stats.cache_hits, hit_rate),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Cache Misses:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                stats.cache_misses.to_string(),
                Style::default().fg(Color::Red),
            ),
        ]),
        Line::from(vec![
            Span::styled("  CSS Compiled:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                stats.css_compilations.to_string(),
                Style::default().fg(Color::Blue),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Full Reloads:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                stats.live_reloads.to_string(),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Avg Response:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}ms", stats.avg_response_ms()),
                Style::default().fg(Color::White),
            ),
        ]),
    ];

    let block = Block::default()
        .title(" Stats ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_profiles(frame: &mut Frame, area: Rect, profiles: &ProfileStore) {
    // 6 inner lines: 1 cred header + up to 2 cred entries + 1 store header + up to 2 store entries
    let inner_w = area.width.saturating_sub(4) as usize;

    let mut lines: Vec<Line> = Vec::new();

    // ── Credentials section ───────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        " ↑↓ Credentials",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));

    if profiles.credentials.is_empty() {
        lines.push(Line::from(Span::styled(
            "    (none — press n)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Show a window of entries centred around the active one
        let cred_count = profiles.credentials.len();
        let active = profiles.active_credential;
        let start = active.saturating_sub(1);
        let end = (start + 2).min(cred_count);
        for (i, cred) in profiles.credentials[start..end].iter().enumerate() {
            let idx = start + i;
            let is_active = idx == active;
            let marker = if is_active { "▶ " } else { "  " };
            let style = if is_active {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let label = format!("{}{}", marker, truncate(&cred.name, inner_w.saturating_sub(2)));
            lines.push(Line::from(Span::styled(format!("  {}", label), style)));
        }
        if cred_count > 2 {
            lines.push(Line::from(Span::styled(
                format!("  … {} total", cred_count),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    // ── Stores section ────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        " ←→ Stores",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));

    if profiles.stores.is_empty() {
        lines.push(Line::from(Span::styled(
            "    (none — press N)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let store_count = profiles.stores.len();
        let active = profiles.active_store;
        let start = active.saturating_sub(1);
        let end = (start + 2).min(store_count);
        for (i, store) in profiles.stores[start..end].iter().enumerate() {
            let idx = start + i;
            let is_active = idx == active;
            let marker = if is_active { "▶ " } else { "  " };
            let style = if is_active {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let label = format!("{}{}", marker, truncate(&store.name, inner_w.saturating_sub(2)));
            lines.push(Line::from(Span::styled(format!("  {}", label), style)));
        }
        if store_count > 2 {
            lines.push(Line::from(Span::styled(
                format!("  … {} total", store_count),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let block = Block::default()
        .title(" Profiles ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_events(frame: &mut Frame, area: Rect, stats: &ServerStats) {
    let visible = area.height.saturating_sub(2) as usize;

    let items: Vec<ListItem> = stats
        .recent_events
        .iter()
        .rev()
        .take(visible)
        .map(|e| {
            let (label_color, label) = match &e.kind {
                EventKind::Request => {
                    let status: u16 = e
                        .extra
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    let color = if status >= 500 {
                        Color::Red
                    } else if status >= 400 {
                        Color::Yellow
                    } else {
                        Color::Green
                    };
                    (color, format!("{:<6}", &e.label))
                }
                EventKind::CssReload => (Color::Blue, "CSS   ".to_string()),
                EventKind::FullReload => (Color::Magenta, "RELOAD".to_string()),
                EventKind::Build => (Color::Yellow, "BUILD ".to_string()),
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("  {} ", e.time),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{} ", label),
                    Style::default().fg(label_color),
                ),
                Span::styled(&e.message, Style::default().fg(Color::White)),
                Span::styled(
                    format!("  {}", e.extra),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::default()
        .title(" Recent Events (newest first) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    frame.render_widget(List::new(items).block(block), area);
}

// ── Bundle status bar ─────────────────────────────────────────────────────────

fn render_bundle_bar(frame: &mut Frame, area: Rect, bundle: &BundleStatus) {
    const SPINNER: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];

    let line = match bundle {
        BundleStatus::Idle => Line::from(Span::styled(
            "  Press [b] to bundle the theme into a ZIP for upload",
            Style::default().fg(Color::DarkGray),
        )),
        BundleStatus::Running { started } => {
            let frame_idx = (started.elapsed().as_millis() / 150) as usize % SPINNER.len();
            let spinner = SPINNER[frame_idx];
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(spinner.to_string(), Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!(" Bundling…  ({:.1}s)", started.elapsed().as_secs_f32()),
                    Style::default().fg(Color::Cyan),
                ),
            ])
        }
        BundleStatus::Done { elapsed, file_count, size_mb, path } => Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(Color::Green)),
            Span::styled(
                format!(
                    "Bundle ready — {} files, {:.2} MB, {:.2?}  →  {}",
                    file_count, size_mb, elapsed, path
                ),
                Style::default().fg(Color::Green),
            ),
        ]),
        BundleStatus::Error(msg) => Line::from(vec![
            Span::styled("  ✗ Bundle failed: ", Style::default().fg(Color::Red)),
            Span::styled(msg.as_str(), Style::default().fg(Color::Red)),
        ]),
    };

    frame.render_widget(Paragraph::new(line), area);
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn render_footer(frame: &mut Frame, area: Rect, mode: &TuiMode) {
    let (line1_spans, line2_spans): (Vec<Span>, Vec<Span>) = match mode {
        TuiMode::Normal => {
            let l1 = key_hints(&[
                ("[q]", " Quit  "),
                ("[b]", " Bundle  "),
                ("[c]", " Clear Cache  "),
                ("[o]", " Open Browser  "),
                ("[r]", " Reload Browser  "),
            ]);
            let l2 = key_hints(&[
                ("[↑↓]", " Cycle Credentials  "),
                ("[←→]", " Cycle Stores  "),
                ("[n]", " New Credential  "),
                ("[N]", " New Store  "),
                ("[Del]", " Remove Credential  "),
            ]);
            (l1, l2)
        }
        TuiMode::AddCred { .. } | TuiMode::AddStore { .. } => {
            let l1 = key_hints(&[
                ("[Tab]", " Next Field  "),
                ("[Enter]", " Confirm / Next  "),
                ("[Esc]", " Cancel  "),
            ]);
            (l1, vec![])
        }
    };

    let lines = vec![
        Line::from(line1_spans),
        Line::from(line2_spans),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

fn key_hints(pairs: &[(&str, &str)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (key, desc) in pairs {
        spans.push(Span::styled(
            key.to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            desc.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans
}

// ── Modals ────────────────────────────────────────────────────────────────────

fn render_add_cred_modal(
    frame: &mut Frame,
    area: Rect,
    name: &str,
    token: &str,
    api_host: &str,
    focus: u8,
) {
    let popup = centered_rect(60, 11, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" New Credential ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // name
            Constraint::Length(2), // token
            Constraint::Length(2), // api_host
            Constraint::Length(1), // hint
        ])
        .split(inner);

    render_modal_field(frame, rows[0], "Name       ", name, focus == 0);
    render_modal_field(frame, rows[1], "Token      ", token, focus == 1);
    render_modal_field(frame, rows[2], "API Host   ", api_host, focus == 2);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  Tab: next field   Enter: confirm / next   Esc: cancel",
            Style::default().fg(Color::DarkGray),
        ))),
        rows[3],
    );
}

fn render_add_store_modal(
    frame: &mut Frame,
    area: Rect,
    name: &str,
    url: &str,
    port: &str,
    focus: u8,
) {
    let popup = centered_rect(60, 11, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" New Store ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // name
            Constraint::Length(2), // url
            Constraint::Length(2), // port
            Constraint::Length(1), // hint
        ])
        .split(inner);

    render_modal_field(frame, rows[0], "Name       ", name, focus == 0);
    render_modal_field(frame, rows[1], "Store URL  ", url, focus == 1);
    render_modal_field(frame, rows[2], "Port       ", port, focus == 2);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  Tab: next field   Enter: confirm / next   Esc: cancel",
            Style::default().fg(Color::DarkGray),
        ))),
        rows[3],
    );
}

fn render_modal_field(frame: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let label_style = Style::default().fg(Color::DarkGray);
    let (value_style, cursor) = if focused {
        (
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            "█",
        )
    } else {
        (Style::default().fg(Color::White), "")
    };

    let block_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let line = Line::from(vec![
        Span::styled(label, label_style),
        Span::styled(value, value_style),
        Span::styled(cursor, Style::default().fg(Color::Cyan)),
    ]);

    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(block_style),
    );
    frame.render_widget(paragraph, area);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> String {
    if max == 0 || s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

/// Returns a [`Rect`] centred within `r` with the given percentage dimensions.
fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let w = r.width * percent_x / 100;
    let x = r.x + (r.width.saturating_sub(w)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: height.min(r.height),
    }
}
