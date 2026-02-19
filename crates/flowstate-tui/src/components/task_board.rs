use crossterm::event::{KeyCode, KeyEvent};
use flowstate_core::task::{Priority, Status, Task};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

pub struct TaskBoard {
    columns: Vec<BoardColumn>,
    active_column: usize,
}

struct BoardColumn {
    status: Status,
    tasks: Vec<Task>,
    list_state: ListState,
}

impl TaskBoard {
    pub fn new(columns: Vec<(Status, Vec<Task>)>) -> Self {
        let columns = columns
            .into_iter()
            .map(|(status, tasks)| {
                let mut list_state = ListState::default();
                if !tasks.is_empty() {
                    list_state.select(Some(0));
                }
                BoardColumn {
                    status,
                    tasks,
                    list_state,
                }
            })
            .collect();
        Self {
            columns,
            active_column: 0,
        }
    }

    /// Returns the currently highlighted task, if any.
    pub fn selected_task(&self) -> Option<&Task> {
        let col = self.columns.get(self.active_column)?;
        let idx = col.list_state.selected()?;
        col.tasks.get(idx)
    }

    /// Returns the status of the currently active column.
    pub fn active_status(&self) -> Status {
        self.columns
            .get(self.active_column)
            .map(|c| c.status)
            .unwrap_or(Status::Todo)
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => {
                if self.active_column > 0 {
                    self.active_column -= 1;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.active_column + 1 < self.columns.len() {
                    self.active_column += 1;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(col) = self.columns.get_mut(self.active_column) {
                    let current = col.list_state.selected().unwrap_or(0);
                    if current + 1 < col.tasks.len() {
                        col.list_state.select(Some(current + 1));
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(col) = self.columns.get_mut(self.active_column) {
                    let current = col.list_state.selected().unwrap_or(0);
                    if current > 0 {
                        col.list_state.select(Some(current - 1));
                    }
                }
            }
            // Jump to first/last
            KeyCode::Char('g') => {
                if let Some(col) = self.columns.get_mut(self.active_column) {
                    if !col.tasks.is_empty() {
                        col.list_state.select(Some(0));
                    }
                }
            }
            KeyCode::Char('G') => {
                if let Some(col) = self.columns.get_mut(self.active_column) {
                    if !col.tasks.is_empty() {
                        col.list_state.select(Some(col.tasks.len() - 1));
                    }
                }
            }
            _ => {}
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let col_count = self.columns.len() as u16;
        if col_count == 0 {
            return;
        }

        let constraints: Vec<Constraint> = (0..col_count)
            .map(|_| Constraint::Ratio(1, col_count as u32))
            .collect();

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);

        for (i, (col, chunk)) in self.columns.iter().zip(chunks.iter()).enumerate() {
            let is_active = i == self.active_column;
            self.render_column(frame, col, *chunk, is_active);
        }
    }

    fn render_column(&self, frame: &mut Frame, col: &BoardColumn, area: Rect, is_active: bool) {
        let task_count = col.tasks.len();
        let title = format!(" {} ({}) ", col.status.display_name(), task_count);

        let border_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        let items: Vec<ListItem> = col
            .tasks
            .iter()
            .map(|task| {
                let priority_span = Span::styled(
                    format!("{} ", task.priority.symbol()),
                    priority_color(task.priority),
                );
                let title_span = Span::raw(&task.title);
                ListItem::new(Line::from(vec![priority_span, title_span]))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .bold(),
            )
            .highlight_symbol("> ");

        let mut state = col.list_state.clone();
        frame.render_stateful_widget(list, area, &mut state);
    }
}

fn priority_color(priority: Priority) -> Style {
    match priority {
        Priority::Urgent => Style::default().fg(Color::Red).bold(),
        Priority::High => Style::default().fg(Color::LightRed),
        Priority::Medium => Style::default().fg(Color::Yellow),
        Priority::Low => Style::default().fg(Color::Blue),
        Priority::None => Style::default().fg(Color::DarkGray),
    }
}
