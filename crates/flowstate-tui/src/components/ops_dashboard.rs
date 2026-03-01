use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Row, Table, Wrap};

use crate::app::{OpsSection, OpsState};

/// Render the full Ops dashboard into `area`.
pub fn render_ops(frame: &mut Frame, area: Rect, state: &OpsState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Min(0)])
        .split(area);

    render_nav_sidebar(frame, chunks[0], state);
    render_content(frame, chunks[1], state);
}

fn render_nav_sidebar(frame: &mut Frame, area: Rect, state: &OpsState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = OpsSection::ALL
        .iter()
        .map(|section| {
            let style = if *section == state.section {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default()
            };
            let marker = if *section == state.section {
                "> "
            } else {
                "  "
            };
            let key = match section {
                OpsSection::Overview => "[o] ",
                OpsSection::ApiKeys => "[a] ",
                OpsSection::Runners => "[u] ",
                OpsSection::Gpu => "[g] ",
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(key, Style::default().fg(Color::Yellow)),
                Span::styled(section.label(), style),
            ]))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, inner);
}

fn render_content(frame: &mut Frame, area: Rect, state: &OpsState) {
    match state.section {
        OpsSection::Overview => render_overview(frame, area, state),
        OpsSection::ApiKeys => render_api_keys(frame, area, state),
        OpsSection::Runners => render_runners(frame, area, state),
        OpsSection::Gpu => render_gpu(frame, area, state),
    }
}

fn render_overview(frame: &mut Frame, area: Rect, state: &OpsState) {
    let block = Block::default()
        .title(" Overview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let gpu_status = state
        .gpu
        .as_ref()
        .map(|g| {
            if g.enabled {
                g.pod_status.as_deref().unwrap_or("enabled").to_string()
            } else {
                "disabled".to_string()
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    let queue_depth = state
        .gpu
        .as_ref()
        .map(|g| g.queue_depth.to_string())
        .unwrap_or_else(|| "-".to_string());

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Runners:     ", Style::default().bold()),
            Span::raw(format!("{} registered", state.runners.len())),
        ]),
        Line::from(vec![
            Span::styled("  API Keys:    ", Style::default().bold()),
            Span::raw(format!("{} active", state.api_keys.len())),
        ]),
        Line::from(vec![
            Span::styled("  GPU:         ", Style::default().bold()),
            Span::raw(gpu_status),
        ]),
        Line::from(vec![
            Span::styled("  Queue Depth: ", Style::default().bold()),
            Span::raw(queue_depth),
        ]),
    ];

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn render_api_keys(frame: &mut Frame, area: Rect, state: &OpsState) {
    let block = Block::default()
        .title(" API Keys ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.api_keys.is_empty() {
        let msg = Paragraph::new("  No API keys. Press [g] to generate one.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, inner);
        return;
    }

    let header = Row::new(vec!["  Name", "Created", "Last Used"])
        .style(Style::default().bold().fg(Color::Yellow));

    let rows: Vec<Row> = state
        .api_keys
        .iter()
        .enumerate()
        .map(|(i, key)| {
            let marker = if i == state.selected_key { "> " } else { "  " };
            let style = if i == state.selected_key {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            let last_used = key.last_used_at.as_deref().unwrap_or("never");
            Row::new(vec![
                format!("{}{}", marker, key.name),
                key.created_at.chars().take(10).collect::<String>(),
                last_used.chars().take(10).collect::<String>(),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Min(20),
        Constraint::Length(12),
        Constraint::Length(12),
    ];

    let table = Table::new(rows, widths).header(header);

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_key));
    frame.render_widget(table, inner);
}

fn render_runners(frame: &mut Frame, area: Rect, state: &OpsState) {
    let block = Block::default()
        .title(" Runners ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split inner area: runners table on top, pending handshakes below
    let pending: Vec<_> = state
        .handshakes
        .iter()
        .filter(|h| h.status == "pending")
        .collect();

    let constraints = if pending.is_empty() {
        vec![Constraint::Min(0)]
    } else {
        vec![
            Constraint::Min(0),
            Constraint::Length((pending.len() as u16) + 4),
        ]
    };

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // -- Registered runners --
    if state.runners.is_empty() {
        let msg =
            Paragraph::new("  No runners registered.").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, sections[0]);
    } else {
        let header = Row::new(vec![
            "  ID",
            "Backend",
            "Cap",
            "Active",
            "Status",
            "Last Seen",
        ])
        .style(Style::default().bold().fg(Color::Yellow));

        let rows: Vec<Row> = state
            .runners
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let marker = if i == state.selected_runner {
                    "> "
                } else {
                    "  "
                };
                let style = if i == state.selected_runner {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                let active = r
                    .active_count
                    .map(|a| {
                        let max = r.max_concurrent.unwrap_or(0);
                        format!("{a}/{max}")
                    })
                    .unwrap_or_else(|| "-".to_string());
                Row::new(vec![
                    format!("{}{}", marker, truncate_id(&r.runner_id)),
                    r.backend_name.as_deref().unwrap_or("-").to_string(),
                    r.capability.as_deref().unwrap_or("-").to_string(),
                    active,
                    r.status.clone(),
                    r.last_seen.chars().take(19).collect::<String>(),
                ])
                .style(style)
            })
            .collect();

        let widths = [
            Constraint::Min(14),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(20),
        ];

        let table = Table::new(rows, widths).header(header);
        frame.render_widget(table, sections[0]);
    }

    // -- Pending handshakes --
    if !pending.is_empty() {
        let handshake_block = Block::default()
            .title(" Pending Runners ")
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::Yellow));

        let handshake_inner = handshake_block.inner(sections[1]);
        frame.render_widget(handshake_block, sections[1]);

        let pending_idx = state.selected_handshake;
        let rows: Vec<Row> = pending
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let marker = if i == pending_idx { "> " } else { "  " };
                let style = if i == pending_idx {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                };
                Row::new(vec![
                    format!("{}{}", marker, truncate_id(&h.runner_id)),
                    h.hostname.clone(),
                    h.created_at.chars().take(19).collect::<String>(),
                    "[y] approve  [x] reject".to_string(),
                ])
                .style(style)
            })
            .collect();

        let widths = [
            Constraint::Min(14),
            Constraint::Length(16),
            Constraint::Length(20),
            Constraint::Length(26),
        ];

        let header = Row::new(vec!["  Runner ID", "Hostname", "Requested", "Actions"])
            .style(Style::default().bold().fg(Color::Yellow));

        let table = Table::new(rows, widths).header(header);
        frame.render_widget(table, handshake_inner);
    }
}

fn render_gpu(frame: &mut Frame, area: Rect, state: &OpsState) {
    let block = Block::default()
        .title(" GPU / RunPod ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = match &state.gpu {
        Some(gpu) => {
            let status_style = if gpu.enabled {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let pod_status = gpu.pod_status.as_deref().unwrap_or("none");
            let daily_cost = gpu
                .daily_cost_cents
                .map(|c| format!("${:.2}/day", c as f64 / 100.0))
                .unwrap_or_else(|| "-".to_string());
            let cost_capped = gpu
                .cost_capped
                .map(|c| if c { "Yes" } else { "No" })
                .unwrap_or("-");

            vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Enabled:     ", Style::default().bold()),
                    Span::styled(if gpu.enabled { "Yes" } else { "No" }, status_style),
                ]),
                Line::from(vec![
                    Span::styled("  Pod ID:      ", Style::default().bold()),
                    Span::raw(gpu.pod_id.as_deref().unwrap_or("none")),
                ]),
                Line::from(vec![
                    Span::styled("  Pod Status:  ", Style::default().bold()),
                    Span::raw(pod_status),
                ]),
                Line::from(vec![
                    Span::styled("  Daily Cost:  ", Style::default().bold()),
                    Span::raw(daily_cost),
                ]),
                Line::from(vec![
                    Span::styled("  Cost Capped: ", Style::default().bold()),
                    Span::raw(cost_capped),
                ]),
                Line::from(vec![
                    Span::styled("  Queue Depth: ", Style::default().bold()),
                    Span::raw(gpu.queue_depth.to_string()),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "  [s] Start Pod   [S] Stop Pod",
                    Style::default().fg(Color::DarkGray),
                )),
            ]
        }
        None => {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  GPU status unavailable. Press [r] to refresh.",
                    Style::default().fg(Color::DarkGray),
                )),
            ]
        }
    };

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn truncate_id(id: &str) -> &str {
    if id.len() > 12 {
        &id[..12]
    } else {
        id
    }
}
