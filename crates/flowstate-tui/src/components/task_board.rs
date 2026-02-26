use crossterm::event::{KeyCode, KeyEvent};
use flowstate_core::task::{ApprovalStatus, Priority, Status, Task};
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

    /// Attempt to select the task with the given ID.
    /// Scans all columns; if found, sets `active_column` to that column
    /// and selects the task's index within the column.
    /// Returns `true` if the task was found and selected, `false` otherwise.
    pub fn select_task_by_id(&mut self, task_id: &str) -> bool {
        for (col_idx, col) in self.columns.iter_mut().enumerate() {
            if let Some(task_idx) = col.tasks.iter().position(|t| t.id == task_id) {
                self.active_column = col_idx;
                col.list_state.select(Some(task_idx));
                return true;
            }
        }
        false
    }

    /// Returns the status of the currently active column.
    pub fn active_status(&self) -> Status {
        self.columns
            .get(self.active_column)
            .map(|c| c.status)
            .unwrap_or(Status::Todo)
    }

    /// Scans the board for the next task requiring attention (Pending or Rejected phase).
    /// Starts from the position after the current cursor and wraps around.
    /// Returns `true` if a task was found and selected, `false` otherwise.
    pub fn select_next_attention(&mut self) -> bool {
        if self.columns.is_empty() {
            return false;
        }

        let num_cols = self.columns.len();
        let start_col = self.active_column;
        let start_row = self.columns[start_col].list_state.selected().unwrap_or(0);

        // Total tasks across all columns
        let total_tasks: usize = self.columns.iter().map(|c| c.tasks.len()).sum();
        if total_tasks == 0 {
            return false;
        }

        // Scan starting from (start_col, start_row + 1), wrapping around
        let mut col = start_col;
        let mut row = start_row + 1;

        for _ in 0..total_tasks {
            // Advance to next valid position
            while col < num_cols && row >= self.columns[col].tasks.len() {
                col += 1;
                row = 0;
            }
            if col >= num_cols {
                col = 0;
                row = 0;
                // Re-advance past empty leading columns
                while col < num_cols && row >= self.columns[col].tasks.len() {
                    col += 1;
                    row = 0;
                }
            }
            if col >= num_cols {
                break;
            }

            // Check if we've wrapped back to start
            if col == start_col && row == start_row {
                // Check the start position itself
                if self.columns[col].tasks[row].attention_required().is_some() {
                    return true; // Already selected
                }
                return false;
            }

            if self.columns[col].tasks[row].attention_required().is_some() {
                self.active_column = col;
                self.columns[col].list_state.select(Some(row));
                return true;
            }

            row += 1;
        }

        // Check the starting position itself (in case it's the only attention task)
        if start_row < self.columns[start_col].tasks.len()
            && self.columns[start_col].tasks[start_row]
                .attention_required()
                .is_some()
        {
            // Already selected at this position
            return true;
        }

        false
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
                let mut spans = vec![priority_span];
                if let Some(indicator) = phase_attention_indicator(task) {
                    spans.push(indicator);
                }
                spans.push(Span::raw(&task.title));
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan).bold())
            .highlight_symbol("> ");

        let mut state = col.list_state.clone();
        frame.render_stateful_widget(list, area, &mut state);
    }
}

fn phase_attention_indicator(task: &Task) -> Option<Span<'_>> {
    let (phase, approval_status) = task.attention_required()?;
    let symbol = match phase {
        Status::Research => "󰒆 ",
        Status::Design => "󰚎 ",
        Status::Plan => "󰏗 ",
        Status::Verify => "󰗠 ",
        _ => return None,
    };
    let style = match approval_status {
        ApprovalStatus::Pending => Style::default().fg(Color::Yellow),
        ApprovalStatus::Rejected => Style::default().fg(Color::Red),
        _ => Style::default(),
    };
    Some(Span::styled(symbol, style))
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use flowstate_core::task::{ApprovalStatus, Priority, Status, Task};

    fn make_task(id: &str, status: Status) -> Task {
        Task {
            id: id.to_string(),
            project_id: "proj".to_string(),
            sprint_id: None,
            parent_id: None,
            title: format!("Task {id}"),
            description: String::new(),
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
            research_status: ApprovalStatus::default(),
            spec_status: ApprovalStatus::default(),
            plan_status: ApprovalStatus::default(),
            verify_status: ApprovalStatus::default(),
            spec_approved_hash: String::new(),
            research_approved_hash: String::new(),
            research_feedback: String::new(),
            spec_feedback: String::new(),
            plan_feedback: String::new(),
            verify_feedback: String::new(),
            status,
            priority: Priority::Medium,
            sort_order: 0.0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_board() -> TaskBoard {
        TaskBoard::new(vec![
            (
                Status::Todo,
                vec![make_task("t1", Status::Todo), make_task("t2", Status::Todo)],
            ),
            (Status::Research, vec![make_task("r1", Status::Research)]),
            (Status::Design, vec![]),
            (
                Status::Plan,
                vec![
                    make_task("p1", Status::Plan),
                    make_task("p2", Status::Plan),
                    make_task("p3", Status::Plan),
                ],
            ),
            (Status::Build, vec![]),
            (Status::Verify, vec![]),
            (Status::Done, vec![make_task("d1", Status::Done)]),
        ])
    }

    #[test]
    fn select_task_in_first_column() {
        let mut board = make_board();
        assert!(board.select_task_by_id("t2"));
        assert_eq!(board.active_column, 0);
        assert_eq!(board.selected_task().unwrap().id, "t2");
    }

    #[test]
    fn select_task_in_middle_column() {
        let mut board = make_board();
        assert!(board.select_task_by_id("p2"));
        assert_eq!(board.active_column, 3);
        assert_eq!(board.selected_task().unwrap().id, "p2");
    }

    #[test]
    fn select_task_in_last_column() {
        let mut board = make_board();
        assert!(board.select_task_by_id("d1"));
        assert_eq!(board.active_column, 6);
        assert_eq!(board.selected_task().unwrap().id, "d1");
    }

    #[test]
    fn select_nonexistent_task_returns_false() {
        let mut board = make_board();
        // Set cursor to a known position first
        board.select_task_by_id("p2");
        assert_eq!(board.active_column, 3);

        // Attempt to select non-existent task
        assert!(!board.select_task_by_id("nonexistent"));
        // Cursor should remain unchanged
        assert_eq!(board.active_column, 3);
        assert_eq!(board.selected_task().unwrap().id, "p2");
    }

    #[test]
    fn select_on_empty_board() {
        let mut board = TaskBoard::new(vec![
            (Status::Todo, vec![]),
            (Status::Research, vec![]),
            (Status::Design, vec![]),
            (Status::Plan, vec![]),
            (Status::Build, vec![]),
            (Status::Verify, vec![]),
            (Status::Done, vec![]),
        ]);
        assert!(!board.select_task_by_id("anything"));
    }

    #[test]
    fn select_only_task_in_column() {
        let mut board = make_board();
        assert!(board.select_task_by_id("r1"));
        assert_eq!(board.active_column, 1);
        assert_eq!(board.selected_task().unwrap().id, "r1");
    }

    #[test]
    fn select_updates_from_one_column_to_another() {
        let mut board = make_board();
        assert!(board.select_task_by_id("t1"));
        assert_eq!(board.active_column, 0);
        assert_eq!(board.selected_task().unwrap().id, "t1");

        assert!(board.select_task_by_id("d1"));
        assert_eq!(board.active_column, 6);
        assert_eq!(board.selected_task().unwrap().id, "d1");
    }

    // --- Navigation tests ---

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn navigate_right_clamps_at_last_column() {
        let mut board = make_board();
        // Move to last column (index 6)
        board.active_column = 6;
        board.handle_key(key(KeyCode::Char('l')));
        assert_eq!(board.active_column, 6);
    }

    #[test]
    fn navigate_left_clamps_at_first_column() {
        let mut board = make_board();
        assert_eq!(board.active_column, 0);
        board.handle_key(key(KeyCode::Char('h')));
        assert_eq!(board.active_column, 0);
    }

    #[test]
    fn navigate_down_past_end() {
        let mut board = make_board();
        // Column 0 has 2 tasks (t1, t2), cursor starts at 0
        assert_eq!(board.selected_task().unwrap().id, "t1");

        board.handle_key(key(KeyCode::Char('j')));
        assert_eq!(board.selected_task().unwrap().id, "t2");

        // Third press should stay at last task
        board.handle_key(key(KeyCode::Char('j')));
        assert_eq!(board.selected_task().unwrap().id, "t2");
    }

    #[test]
    fn navigate_up_past_start() {
        let mut board = make_board();
        // Cursor starts at first task
        assert_eq!(board.selected_task().unwrap().id, "t1");

        board.handle_key(key(KeyCode::Char('k')));
        assert_eq!(board.selected_task().unwrap().id, "t1");
    }

    #[test]
    fn empty_board_navigation() {
        let mut board = TaskBoard::new(vec![
            (Status::Todo, vec![]),
            (Status::Research, vec![]),
            (Status::Design, vec![]),
        ]);
        // None of these should panic
        board.handle_key(key(KeyCode::Char('h')));
        board.handle_key(key(KeyCode::Char('l')));
        board.handle_key(key(KeyCode::Char('j')));
        board.handle_key(key(KeyCode::Char('k')));
        board.handle_key(key(KeyCode::Char('g')));
        board.handle_key(key(KeyCode::Char('G')));
        assert!(board.selected_task().is_none());
    }

    #[test]
    fn navigate_right_selected_task_updates() {
        let mut board = make_board();
        // Column 0 has t1,t2; column 1 has r1
        assert_eq!(board.selected_task().unwrap().id, "t1");

        board.handle_key(key(KeyCode::Char('l')));
        assert_eq!(board.active_column, 1);
        assert_eq!(board.selected_task().unwrap().id, "r1");
    }

    #[test]
    fn navigate_g_jumps_to_top() {
        let mut board = make_board();
        // Move to column 3 which has 3 tasks (p1, p2, p3)
        board.select_task_by_id("p3");
        assert_eq!(board.selected_task().unwrap().id, "p3");

        board.handle_key(key(KeyCode::Char('g')));
        assert_eq!(board.selected_task().unwrap().id, "p1");
    }

    #[test]
    fn navigate_big_g_jumps_to_bottom() {
        let mut board = make_board();
        // Move to column 3, cursor starts at p1 after select_task_by_id
        board.select_task_by_id("p1");
        assert_eq!(board.selected_task().unwrap().id, "p1");

        board.handle_key(key(KeyCode::Char('G')));
        assert_eq!(board.selected_task().unwrap().id, "p3");
    }

    #[test]
    fn arrow_keys_match_hjkl() {
        let mut board = make_board();
        // Right arrow moves column like 'l'
        board.handle_key(key(KeyCode::Right));
        assert_eq!(board.active_column, 1);

        // Left arrow moves back like 'h'
        board.handle_key(key(KeyCode::Left));
        assert_eq!(board.active_column, 0);

        // Down arrow moves cursor like 'j'
        board.handle_key(key(KeyCode::Down));
        assert_eq!(board.selected_task().unwrap().id, "t2");

        // Up arrow moves cursor like 'k'
        board.handle_key(key(KeyCode::Up));
        assert_eq!(board.selected_task().unwrap().id, "t1");
    }

    #[test]
    fn navigate_into_empty_column_selected_is_none() {
        let mut board = make_board();
        // Column 1 = Research (has r1), Column 2 = Design (empty)
        board.handle_key(key(KeyCode::Char('l'))); // -> column 1
        assert_eq!(board.active_column, 1);
        assert_eq!(board.selected_task().unwrap().id, "r1");

        board.handle_key(key(KeyCode::Char('l'))); // -> column 2 (empty)
        assert_eq!(board.active_column, 2);
        assert!(board.selected_task().is_none());
    }

    // --- Attention navigation tests ---

    fn make_task_with_approval(id: &str, status: Status, plan_status: ApprovalStatus) -> Task {
        let mut t = make_task(id, status);
        t.plan_status = plan_status;
        t
    }

    fn make_task_with_verify_rejected(id: &str, status: Status) -> Task {
        let mut t = make_task(id, status);
        t.verify_status = ApprovalStatus::Rejected;
        t
    }

    fn make_attention_board() -> TaskBoard {
        // Column 0: Todo — no attention tasks
        // Column 1: Research — one task with pending plan
        // Column 2: Design — empty
        // Column 3: Plan — task p2 has pending plan
        // Column 4: Build — no attention
        // Column 5: Verify — no attention
        // Column 6: Done — no attention
        TaskBoard::new(vec![
            (
                Status::Todo,
                vec![make_task("t1", Status::Todo), make_task("t2", Status::Todo)],
            ),
            (
                Status::Research,
                vec![make_task_with_approval(
                    "r1",
                    Status::Research,
                    ApprovalStatus::Pending,
                )],
            ),
            (Status::Design, vec![]),
            (
                Status::Plan,
                vec![
                    make_task("p1", Status::Plan),
                    make_task_with_approval("p2", Status::Plan, ApprovalStatus::Pending),
                    make_task("p3", Status::Plan),
                ],
            ),
            (Status::Build, vec![]),
            (Status::Verify, vec![]),
            (Status::Done, vec![make_task("d1", Status::Done)]),
        ])
    }

    #[test]
    fn select_next_attention_finds_pending_task() {
        let mut board = make_attention_board();
        // Cursor starts at column 0, task t1 (no attention)
        assert_eq!(board.active_column, 0);
        assert_eq!(board.selected_task().unwrap().id, "t1");

        assert!(board.select_next_attention());
        // Should jump to r1 in column 1 (first attention task)
        assert_eq!(board.active_column, 1);
        assert_eq!(board.selected_task().unwrap().id, "r1");
    }

    #[test]
    fn select_next_attention_skips_non_pending() {
        let mut board = make_attention_board();
        // Move cursor to r1 (which is an attention task)
        board.select_task_by_id("r1");
        assert_eq!(board.selected_task().unwrap().id, "r1");

        // Next attention should skip p1 and go to p2
        assert!(board.select_next_attention());
        assert_eq!(board.active_column, 3);
        assert_eq!(board.selected_task().unwrap().id, "p2");
    }

    #[test]
    fn select_next_attention_wraps_around() {
        let mut board = make_attention_board();
        // Move cursor to p2 (which is an attention task in column 3)
        board.select_task_by_id("p2");
        assert_eq!(board.selected_task().unwrap().id, "p2");

        // Next attention should wrap around to r1 in column 1
        assert!(board.select_next_attention());
        assert_eq!(board.active_column, 1);
        assert_eq!(board.selected_task().unwrap().id, "r1");
    }

    #[test]
    fn select_next_attention_returns_false_when_none() {
        // Board with no attention tasks
        let mut board = make_board();
        let original_col = board.active_column;
        let original_task = board.selected_task().unwrap().id.clone();

        assert!(!board.select_next_attention());
        // Cursor should remain unchanged
        assert_eq!(board.active_column, original_col);
        assert_eq!(board.selected_task().unwrap().id, original_task);
    }

    #[test]
    fn select_next_attention_advances_from_current() {
        let mut board = make_attention_board();
        // Start at r1 (attention task)
        board.select_task_by_id("r1");

        // Should advance to p2, not stay on r1
        assert!(board.select_next_attention());
        assert_eq!(board.selected_task().unwrap().id, "p2");

        // From p2, should wrap to r1
        assert!(board.select_next_attention());
        assert_eq!(board.selected_task().unwrap().id, "r1");
    }

    #[test]
    fn select_next_attention_handles_rejected() {
        let mut board = TaskBoard::new(vec![
            (Status::Todo, vec![make_task("t1", Status::Todo)]),
            (
                Status::Verify,
                vec![make_task_with_verify_rejected("v1", Status::Verify)],
            ),
        ]);

        assert_eq!(board.active_column, 0);
        assert!(board.select_next_attention());
        assert_eq!(board.active_column, 1);
        assert_eq!(board.selected_task().unwrap().id, "v1");
    }
}
