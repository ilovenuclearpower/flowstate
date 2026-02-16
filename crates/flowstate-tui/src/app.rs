use anyhow::Result;
use crossterm::event::KeyEvent;
use flowstate_core::project::CreateProject;
use flowstate_core::task::{Status, Task, TaskFilter};
use flowstate_core::Project;
use flowstate_db::Db;
use ratatui::prelude::*;

use crate::components::task_board::TaskBoard;

pub struct App {
    db: Db,
    project: Project,
    board: TaskBoard,
}

impl App {
    pub fn new(db: Db) -> Result<Self> {
        // Get or create a default project
        let project = match db.list_projects() {
            Ok(projects) if !projects.is_empty() => projects.into_iter().next().unwrap(),
            _ => db.create_project(&CreateProject {
                name: "Default".into(),
                slug: "default".into(),
                description: "Default project".into(),
            })?,
        };

        let board = Self::load_board(&db, &project.id)?;

        Ok(Self { db, project, board })
    }

    fn load_board(db: &Db, project_id: &str) -> Result<TaskBoard> {
        let mut columns: Vec<(Status, Vec<Task>)> = Vec::new();
        for &status in Status::BOARD_COLUMNS {
            let tasks = db.list_tasks(&TaskFilter {
                project_id: Some(project_id.to_string()),
                status: Some(status),
                ..Default::default()
            })?;
            columns.push((status, tasks));
        }
        Ok(TaskBoard::new(columns))
    }

    pub fn refresh_board(&mut self) -> Result<()> {
        self.board = Self::load_board(&self.db, &self.project.id)?;
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.board.handle_key(key);
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title bar
                Constraint::Min(0),   // board
                Constraint::Length(1), // status bar
            ])
            .split(area);

        self.render_title_bar(frame, layout[0]);
        self.board.render(frame, layout[1]);
        self.render_status_bar(frame, layout[2]);
    }

    fn render_title_bar(&self, frame: &mut Frame, area: Rect) {
        let title = Line::from(vec![
            Span::styled(" flowstate ", Style::default().bold().fg(Color::Cyan)),
            Span::raw("| "),
            Span::styled(
                &self.project.name,
                Style::default().fg(Color::Yellow),
            ),
        ]);
        frame.render_widget(title, area);
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let status = Line::from(vec![
            Span::styled(
                " q",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw(" quit  "),
            Span::styled("h/l", Style::default().fg(Color::Yellow).bold()),
            Span::raw(" columns  "),
            Span::styled("j/k", Style::default().fg(Color::Yellow).bold()),
            Span::raw(" tasks  "),
            Span::styled("n", Style::default().fg(Color::Yellow).bold()),
            Span::raw(" new task  "),
            Span::styled("m/M", Style::default().fg(Color::Yellow).bold()),
            Span::raw(" move status"),
        ]);
        frame.render_widget(status, area);
    }
}
