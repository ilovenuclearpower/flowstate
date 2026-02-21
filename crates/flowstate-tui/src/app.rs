use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use flowstate_core::project::{CreateProject, UpdateProject};
use flowstate_core::sprint::{CreateSprint, Sprint};
use flowstate_core::task::{
    next_subtask_status, prev_subtask_status, ApprovalStatus, CreateTask, Priority, Status, Task,
    TaskFilter, UpdateTask,
};
use flowstate_core::Project;
use flowstate_service::BlockingHttpService;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::components::task_board::TaskBoard;

/// What the app is currently doing
#[derive(Debug, Clone)]
pub enum Mode {
    /// Normal board navigation
    Normal,
    /// Typing a new task title
    NewTask { input: String },
    /// Viewing task detail
    TaskDetail { task: Task },
    /// Editing a task's title
    EditTitle { task_id: String, input: String },
    /// Editing a task's description
    EditDescription { task_id: String, input: String },
    /// Confirm delete task
    ConfirmDelete { task: Task },
    /// Priority picker
    PriorityPick { task_id: String, current: Priority },
    /// Project list/switcher
    ProjectList {
        projects: Vec<Project>,
        list_state: ListState,
    },
    /// Creating a new project
    NewProject {
        name: String,
        slug: String,
        field: ProjectField,
    },
    /// Confirm delete project
    ConfirmDeleteProject { project: Project },
    /// Claude action picker (design/plan/build)
    ClaudeActionPick { task: Task },
    /// Waiting for a Claude run to finish (polling)
    ClaudeRunning {
        task: Task,
        run_id: String,
        progress: Option<String>,
    },
    /// Viewing Claude output (scrollable)
    ClaudeOutput {
        task: Task,
        output: String,
        scroll: u16,
    },
    /// Approve/reject spec or plan
    ApprovalPick {
        task: Task,
        /// "spec" or "plan"
        field: String,
    },
    /// Read-only spec viewer
    ViewSpec { task: Task, scroll: u16 },
    /// Read-only plan viewer
    ViewPlan { task: Task, scroll: u16 },
    /// Read-only research viewer
    ViewResearch { task: Task, scroll: u16 },
    /// Read-only verification viewer
    ViewVerification { task: Task, scroll: u16 },
    /// Entering feedback text for rejection
    FeedbackInput { task: Task, field: String, input: String },
    /// Editing a project's repo URL
    EditRepoUrl {
        project_id: String,
        input: String,
    },
    /// Editing a project's repo token (PAT)
    EditRepoToken {
        project_id: String,
        input: String,
    },
    /// System health checks
    Health {
        checks: Vec<HealthCheck>,
    },
    /// Sprint list/picker
    SprintList {
        sprints: Vec<Sprint>,
        list_state: ListState,
    },
    /// Creating a new sprint
    NewSprint { input: String },
    /// Creating a subtask
    NewSubtask { parent: Task, input: String },
}

#[derive(Debug, Clone)]
pub struct HealthCheck {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub enum CheckStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy)]
pub enum ProjectField {
    Name,
    Slug,
}

pub struct App {
    service: BlockingHttpService,
    project: Project,
    board: TaskBoard,
    mode: Mode,
    status_message: Option<String>,
    /// Set by handle_key when the user wants to open $EDITOR.
    /// The event loop checks this and handles the editor subprocess.
    pub editor_request: Option<EditorRequest>,
    /// Active sprint filter (if set, board shows only tasks in this sprint)
    active_sprint: Option<Sprint>,
}

/// Request to open a file in $EDITOR, with context for what to do after.
#[derive(Debug, Clone)]
pub struct EditorRequest {
    pub path: PathBuf,
    pub task_id: String,
    /// "spec" or "plan"
    pub kind: String,
}

impl App {
    pub fn new(service: BlockingHttpService) -> Result<Self> {
        let project = match service.list_projects() {
            Ok(projects) if !projects.is_empty() => projects.into_iter().next().unwrap(),
            _ => service.create_project(&CreateProject {
                name: "Default".into(),
                slug: "default".into(),
                description: "Default project".into(),
                repo_url: String::new(),
            })?,
        };

        let board = Self::load_board(&service, &project.id, None)?;

        Ok(Self {
            service,
            project,
            board,
            mode: Mode::Normal,
            status_message: None,
            editor_request: None,
            active_sprint: None,
        })
    }

    fn load_board(
        service: &BlockingHttpService,
        project_id: &str,
        sprint_id: Option<String>,
    ) -> Result<TaskBoard> {
        let mut columns: Vec<(Status, Vec<Task>)> = Vec::new();
        for &status in Status::BOARD_COLUMNS {
            let tasks = service.list_tasks(&TaskFilter {
                project_id: Some(project_id.to_string()),
                status: Some(status),
                sprint_id: sprint_id.clone(),
                ..Default::default()
            })?;
            columns.push((status, tasks));
        }
        Ok(TaskBoard::new(columns))
    }

    fn refresh(&mut self) {
        let selected_id = self.board.selected_task().map(|t| t.id.clone());
        let sprint_id = self.active_sprint.as_ref().map(|s| s.id.clone());
        if let Ok(board) = Self::load_board(&self.service, &self.project.id, sprint_id) {
            self.board = board;
            if let Some(id) = selected_id {
                self.board.select_task_by_id(&id);
            }
        }
    }

    fn switch_project(&mut self, project: Project) {
        self.project = project;
        self.refresh();
        self.mode = Mode::Normal;
    }

    pub fn is_input_mode(&self) -> bool {
        matches!(
            self.mode,
            Mode::NewTask { .. }
                | Mode::EditTitle { .. }
                | Mode::EditDescription { .. }
                | Mode::NewProject { .. }
                | Mode::EditRepoUrl { .. }
                | Mode::EditRepoToken { .. }
                | Mode::FeedbackInput { .. }
                | Mode::NewSprint { .. }
                | Mode::NewSubtask { .. }
        )
    }

    /// Returns true if the event loop should use a poll timeout instead of blocking.
    pub fn needs_polling(&self) -> bool {
        matches!(self.mode, Mode::ClaudeRunning { .. })
    }

    /// Poll the Claude run status. Called on timeout from event loop.
    pub fn poll_claude_run(&mut self) {
        if let Mode::ClaudeRunning { ref task, ref run_id, .. } = self.mode.clone() {
            match self.service.get_claude_run(run_id) {
                Ok(run) => {
                    let is_done = matches!(
                        run.status,
                        flowstate_core::claude_run::ClaudeRunStatus::Completed
                            | flowstate_core::claude_run::ClaudeRunStatus::Failed
                            | flowstate_core::claude_run::ClaudeRunStatus::Cancelled
                    );
                    if is_done {
                        let output = self
                            .service
                            .get_claude_run_output(run_id)
                            .unwrap_or_else(|_| "(no output)".into());
                        let msg = format!("Claude run {}: {}", run.status, run.action);
                        self.status_message = Some(msg);
                        self.mode = Mode::ClaudeOutput {
                            task: task.clone(),
                            output,
                            scroll: 0,
                        };
                    } else {
                        // Update progress message
                        self.mode = Mode::ClaudeRunning {
                            task: task.clone(),
                            run_id: run_id.clone(),
                            progress: run.progress_message,
                        };
                    }
                }
                Err(e) => {
                    self.status_message = Some(format!("Poll error: {e}"));
                    self.mode = Mode::TaskDetail { task: task.clone() };
                }
            }
        }
    }

    /// Called after $EDITOR exits. Pushes content back to server via API.
    pub fn editor_done(&mut self) {
        if let Some(req) = self.editor_request.take() {
            // Read the edited file and push it to the server
            match std::fs::read_to_string(&req.path) {
                Ok(content) => {
                    let write_result = match req.kind.as_str() {
                        "spec" => self.service.write_task_spec(&req.task_id, &content),
                        "plan" => self.service.write_task_plan(&req.task_id, &content),
                        "research" => self.service.write_task_research(&req.task_id, &content),
                        "verification" => self.service.write_task_verification(&req.task_id, &content),
                        _ => self.service.write_task_spec(&req.task_id, &content),
                    };

                    if let Err(e) = write_result {
                        self.status_message = Some(format!("Save error: {e}"));
                    } else {
                        self.status_message = Some(format!("{} saved", req.kind));
                    }
                }
                Err(e) => {
                    self.status_message = Some(format!("Read error: {e}"));
                }
            }

            // Reload the task to reflect changes
            match self.service.get_task(&req.task_id) {
                Ok(task) => self.mode = Mode::TaskDetail { task },
                Err(_) => self.mode = Mode::Normal,
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.status_message = None;

        match &self.mode.clone() {
            Mode::Normal => self.handle_normal(key),
            Mode::NewTask { input } => self.handle_new_task(key, input.clone()),
            Mode::TaskDetail { task } => self.handle_task_detail(key, task.clone()),
            Mode::EditTitle { task_id, input } => {
                self.handle_edit_title(key, task_id.clone(), input.clone())
            }
            Mode::EditDescription { task_id, input } => {
                self.handle_edit_description(key, task_id.clone(), input.clone())
            }
            Mode::ConfirmDelete { task } => self.handle_confirm_delete(key, task.clone()),
            Mode::PriorityPick { task_id, current } => {
                self.handle_priority_pick(key, task_id.clone(), *current)
            }
            Mode::ProjectList {
                projects,
                list_state,
            } => self.handle_project_list(key, projects.clone(), list_state.clone()),
            Mode::NewProject { name, slug, field } => {
                self.handle_new_project(key, name.clone(), slug.clone(), *field)
            }
            Mode::ConfirmDeleteProject { project } => {
                self.handle_confirm_delete_project(key, project.clone())
            }
            Mode::ClaudeActionPick { task } => {
                self.handle_claude_action_pick(key, task.clone())
            }
            Mode::ClaudeRunning { task, .. } => {
                // Only Esc cancels (returns to detail, run continues in background)
                if key.code == KeyCode::Esc {
                    self.mode = Mode::TaskDetail { task: task.clone() };
                }
            }
            Mode::Health { .. } => self.handle_health(key),
            Mode::ClaudeOutput { task, output, scroll } => {
                self.handle_claude_output(key, task.clone(), output.clone(), *scroll)
            }
            Mode::ApprovalPick { task, field } => {
                self.handle_approval_pick(key, task.clone(), field.clone())
            }
            Mode::ViewSpec { task, scroll } => {
                self.handle_view_scroll(key, task.clone(), *scroll, "spec")
            }
            Mode::ViewPlan { task, scroll } => {
                self.handle_view_scroll(key, task.clone(), *scroll, "plan")
            }
            Mode::ViewResearch { task, scroll } => {
                self.handle_view_scroll(key, task.clone(), *scroll, "research")
            }
            Mode::ViewVerification { task, scroll } => {
                self.handle_view_scroll(key, task.clone(), *scroll, "verification")
            }
            Mode::FeedbackInput { task, field, input } => {
                self.handle_feedback_input(key, task.clone(), field.clone(), input.clone())
            }
            Mode::EditRepoUrl { project_id, input } => {
                self.handle_edit_repo_url(key, project_id.clone(), input.clone())
            }
            Mode::EditRepoToken { project_id, input } => {
                self.handle_edit_repo_token(key, project_id.clone(), input.clone())
            }
            Mode::SprintList {
                sprints,
                list_state,
            } => self.handle_sprint_list(key, sprints.clone(), list_state.clone()),
            Mode::NewSprint { input } => self.handle_new_sprint(key, input.clone()),
            Mode::NewSubtask { parent, input } => {
                self.handle_new_subtask(key, parent.clone(), input.clone())
            }
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => {
                self.mode = Mode::NewTask {
                    input: String::new(),
                };
            }
            KeyCode::Enter => {
                if let Some(task) = self.board.selected_task() {
                    self.mode = Mode::TaskDetail {
                        task: task.clone(),
                    };
                }
            }
            KeyCode::Char('m') => {
                if let Some(task) = self.board.selected_task() {
                    let next = if task.is_subtask() {
                        next_subtask_status(task.status)
                    } else {
                        next_status(task.status)
                    };
                    if let Some(next) = next {
                        let id = task.id.clone();
                        match self.service.update_task(
                            &id,
                            &UpdateTask {
                                status: Some(next),
                                ..Default::default()
                            },
                        ) {
                            Ok(_) => self.refresh(),
                            Err(e) => self.status_message = Some(format!("Error: {e}")),
                        }
                    }
                }
            }
            KeyCode::Char('M') => {
                if let Some(task) = self.board.selected_task() {
                    let prev = if task.is_subtask() {
                        prev_subtask_status(task.status)
                    } else {
                        prev_status(task.status)
                    };
                    if let Some(prev) = prev {
                        let id = task.id.clone();
                        match self.service.update_task(
                            &id,
                            &UpdateTask {
                                status: Some(prev),
                                ..Default::default()
                            },
                        ) {
                            Ok(_) => self.refresh(),
                            Err(e) => self.status_message = Some(format!("Error: {e}")),
                        }
                    }
                }
            }
            KeyCode::Char('d') => {
                if let Some(task) = self.board.selected_task() {
                    self.mode = Mode::ConfirmDelete {
                        task: task.clone(),
                    };
                }
            }
            KeyCode::Char('p') => {
                if let Some(task) = self.board.selected_task() {
                    self.mode = Mode::PriorityPick {
                        task_id: task.id.clone(),
                        current: task.priority,
                    };
                }
            }
            // Project switcher
            KeyCode::Char('P') => {
                if let Ok(projects) = self.service.list_projects() {
                    let mut list_state = ListState::default();
                    if !projects.is_empty() {
                        // Select current project
                        let idx = projects
                            .iter()
                            .position(|p| p.id == self.project.id)
                            .unwrap_or(0);
                        list_state.select(Some(idx));
                    }
                    self.mode = Mode::ProjectList {
                        projects,
                        list_state,
                    };
                }
            }
            // Health checks
            KeyCode::Char('H') => {
                let checks = self.run_health_checks();
                self.mode = Mode::Health { checks };
            }
            // Sprint list
            KeyCode::Char('x') => {
                if let Ok(sprints) = self.service.list_sprints(&self.project.id) {
                    let mut list_state = ListState::default();
                    if !sprints.is_empty() {
                        // Select current active sprint if any
                        let idx = self
                            .active_sprint
                            .as_ref()
                            .and_then(|active| sprints.iter().position(|s| s.id == active.id))
                            .unwrap_or(0);
                        list_state.select(Some(idx));
                    }
                    self.mode = Mode::SprintList {
                        sprints,
                        list_state,
                    };
                }
            }
            // Clear active sprint filter
            KeyCode::Char('X') => {
                self.active_sprint = None;
                self.refresh();
                self.status_message = Some("Sprint filter cleared".into());
            }
            _ => self.board.handle_key(key),
        }
    }

    fn handle_new_task(&mut self, key: KeyEvent, mut input: String) {
        match key.code {
            KeyCode::Enter => {
                let title = input.trim().to_string();
                if !title.is_empty() {
                    let status = self.board.active_status();
                    match self.service.create_task(&CreateTask {
                        project_id: self.project.id.clone(),
                        title,
                        description: String::new(),
                        status,
                        priority: Priority::Medium,
                        parent_id: None,
                        reviewer: String::new(),
                    }) {
                        Ok(_) => {
                            self.refresh();
                            self.status_message = Some("Task created".into());
                        }
                        Err(e) => self.status_message = Some(format!("Error: {e}")),
                    }
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::NewTask { input };
            }
            KeyCode::Char(c) => {
                input.push(c);
                self.mode = Mode::NewTask { input };
            }
            _ => {}
        }
    }

    fn handle_task_detail(&mut self, key: KeyEvent, task: Task) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Char('t') => {
                self.mode = Mode::EditTitle {
                    task_id: task.id.clone(),
                    input: task.title.clone(),
                };
            }
            KeyCode::Char('e') => {
                self.mode = Mode::EditDescription {
                    task_id: task.id.clone(),
                    input: task.description.clone(),
                };
            }
            KeyCode::Char('n') => {
                self.mode = Mode::NewSubtask {
                    parent: task,
                    input: String::new(),
                };
            }
            KeyCode::Char('p') => {
                self.mode = Mode::PriorityPick {
                    task_id: task.id.clone(),
                    current: task.priority,
                };
            }
            KeyCode::Char('m') => {
                let next = if task.is_subtask() {
                    next_subtask_status(task.status)
                } else {
                    next_status(task.status)
                };
                if let Some(next) = next {
                    match self.service.update_task(
                        &task.id,
                        &UpdateTask {
                            status: Some(next),
                            ..Default::default()
                        },
                    ) {
                        Ok(updated) => {
                            self.refresh();
                            self.mode = Mode::TaskDetail { task: updated };
                        }
                        Err(e) => self.status_message = Some(format!("Error: {e}")),
                    }
                }
            }
            KeyCode::Char('d') => {
                self.mode = Mode::ConfirmDelete { task };
            }
            // Claude action picker
            KeyCode::Char('c') => {
                self.mode = Mode::ClaudeActionPick { task };
            }
            // View spec
            KeyCode::Char('s') => {
                self.mode = Mode::ViewSpec { task, scroll: 0 };
            }
            // Edit spec in $EDITOR
            KeyCode::Char('S') => {
                let path = flowstate_db::task_spec_path(&task.id);
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                // Fetch current spec content from server and write to temp file
                let content = self.service.read_task_spec(&task.id).unwrap_or_default();
                let _ = std::fs::write(&path, &content);
                self.editor_request = Some(EditorRequest {
                    path,
                    task_id: task.id.clone(),
                    kind: "spec".into(),
                });
            }
            // View plan
            KeyCode::Char('i') => {
                self.mode = Mode::ViewPlan { task, scroll: 0 };
            }
            // Edit plan in $EDITOR
            KeyCode::Char('I') => {
                let path = flowstate_db::task_plan_path(&task.id);
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let content = self.service.read_task_plan(&task.id).unwrap_or_default();
                let _ = std::fs::write(&path, &content);
                self.editor_request = Some(EditorRequest {
                    path,
                    task_id: task.id.clone(),
                    kind: "plan".into(),
                });
            }
            // View research
            KeyCode::Char('w') => {
                self.mode = Mode::ViewResearch { task, scroll: 0 };
            }
            // Edit research in $EDITOR
            KeyCode::Char('W') => {
                let path = flowstate_db::task_research_path(&task.id);
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let content = self.service.read_task_research(&task.id).unwrap_or_default();
                let _ = std::fs::write(&path, &content);
                self.editor_request = Some(EditorRequest {
                    path,
                    task_id: task.id.clone(),
                    kind: "research".into(),
                });
            }
            // View verification
            KeyCode::Char('v') => {
                self.mode = Mode::ViewVerification { task, scroll: 0 };
            }
            // Edit verification in $EDITOR
            KeyCode::Char('V') => {
                let path = flowstate_db::task_verification_path(&task.id);
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let content = self.service.read_task_verification(&task.id).unwrap_or_default();
                let _ = std::fs::write(&path, &content);
                self.editor_request = Some(EditorRequest {
                    path,
                    task_id: task.id.clone(),
                    kind: "verification".into(),
                });
            }
            // Approve/reject
            KeyCode::Char('a') => {
                // Pick which field to approve based on what's pending
                if task.research_status == ApprovalStatus::Pending {
                    self.mode = Mode::ApprovalPick {
                        task,
                        field: "research".into(),
                    };
                } else if task.spec_status == ApprovalStatus::Pending {
                    self.mode = Mode::ApprovalPick {
                        task,
                        field: "spec".into(),
                    };
                } else if task.plan_status == ApprovalStatus::Pending {
                    self.mode = Mode::ApprovalPick {
                        task,
                        field: "plan".into(),
                    };
                } else if task.verify_status == ApprovalStatus::Pending {
                    self.mode = Mode::ApprovalPick {
                        task,
                        field: "verify".into(),
                    };
                } else {
                    self.status_message = Some("Nothing pending approval".into());
                }
            }
            _ => {}
        }
    }

    fn handle_edit_title(&mut self, key: KeyEvent, task_id: String, mut input: String) {
        match key.code {
            KeyCode::Enter => {
                let title = input.trim().to_string();
                if !title.is_empty() {
                    match self.service.update_task(
                        &task_id,
                        &UpdateTask {
                            title: Some(title),
                            ..Default::default()
                        },
                    ) {
                        Ok(updated) => {
                            self.refresh();
                            self.mode = Mode::TaskDetail { task: updated };
                            self.status_message = Some("Title updated".into());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("Error: {e}"));
                            self.mode = Mode::Normal;
                        }
                    }
                } else if let Ok(task) = self.service.get_task(&task_id) {
                    self.mode = Mode::TaskDetail { task };
                } else {
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Esc => {
                if let Ok(task) = self.service.get_task(&task_id) {
                    self.mode = Mode::TaskDetail { task };
                } else {
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::EditTitle { task_id, input };
            }
            KeyCode::Char(c) => {
                input.push(c);
                self.mode = Mode::EditTitle { task_id, input };
            }
            _ => {}
        }
    }

    fn handle_edit_description(&mut self, key: KeyEvent, task_id: String, mut input: String) {
        match key.code {
            KeyCode::Char('s')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                match self.service.update_task(
                    &task_id,
                    &UpdateTask {
                        description: Some(input),
                        ..Default::default()
                    },
                ) {
                    Ok(updated) => {
                        self.refresh();
                        self.mode = Mode::TaskDetail { task: updated };
                        self.status_message = Some("Description updated".into());
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::Normal;
                    }
                }
            }
            KeyCode::Esc => {
                if let Ok(task) = self.service.get_task(&task_id) {
                    self.mode = Mode::TaskDetail { task };
                } else {
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Enter => {
                input.push('\n');
                self.mode = Mode::EditDescription { task_id, input };
            }
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::EditDescription { task_id, input };
            }
            KeyCode::Char(c) => {
                input.push(c);
                self.mode = Mode::EditDescription { task_id, input };
            }
            _ => {}
        }
    }

    fn handle_confirm_delete(&mut self, key: KeyEvent, task: Task) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                match self.service.delete_task(&task.id) {
                    Ok(()) => {
                        self.refresh();
                        self.status_message = Some(format!("Deleted: {}", task.title));
                    }
                    Err(e) => self.status_message = Some(format!("Error: {e}")),
                }
                self.mode = Mode::Normal;
            }
            _ => self.mode = Mode::Normal,
        }
    }

    fn handle_priority_pick(&mut self, key: KeyEvent, task_id: String, _current: Priority) {
        let priority = match key.code {
            KeyCode::Char('1') => Some(Priority::Urgent),
            KeyCode::Char('2') => Some(Priority::High),
            KeyCode::Char('3') => Some(Priority::Medium),
            KeyCode::Char('4') => Some(Priority::Low),
            KeyCode::Char('5') => Some(Priority::None),
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                return;
            }
            _ => None,
        };

        if let Some(p) = priority {
            match self.service.update_task(
                &task_id,
                &UpdateTask {
                    priority: Some(p),
                    ..Default::default()
                },
            ) {
                Ok(_) => {
                    self.refresh();
                    self.status_message = Some(format!("Priority: {p}"));
                }
                Err(e) => self.status_message = Some(format!("Error: {e}")),
            }
        }
        self.mode = Mode::Normal;
    }

    fn handle_project_list(
        &mut self,
        key: KeyEvent,
        projects: Vec<Project>,
        mut list_state: ListState,
    ) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => {
                let i = list_state.selected().unwrap_or(0);
                if i + 1 < projects.len() {
                    list_state.select(Some(i + 1));
                }
                self.mode = Mode::ProjectList {
                    projects,
                    list_state,
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = list_state.selected().unwrap_or(0);
                if i > 0 {
                    list_state.select(Some(i - 1));
                }
                self.mode = Mode::ProjectList {
                    projects,
                    list_state,
                };
            }
            KeyCode::Enter => {
                if let Some(idx) = list_state.selected() {
                    if let Some(project) = projects.get(idx) {
                        self.switch_project(project.clone());
                        self.status_message =
                            Some(format!("Switched to: {}", self.project.name));
                    }
                }
            }
            KeyCode::Char('n') => {
                self.mode = Mode::NewProject {
                    name: String::new(),
                    slug: String::new(),
                    field: ProjectField::Name,
                };
            }
            KeyCode::Char('d') => {
                if let Some(idx) = list_state.selected() {
                    if let Some(project) = projects.get(idx) {
                        if project.id != self.project.id {
                            self.mode = Mode::ConfirmDeleteProject {
                                project: project.clone(),
                            };
                        } else {
                            self.status_message = Some("Cannot delete active project".into());
                            self.mode = Mode::ProjectList {
                                projects,
                                list_state,
                            };
                        }
                    }
                }
            }
            KeyCode::Char('r') => {
                if let Some(idx) = list_state.selected() {
                    if let Some(project) = projects.get(idx) {
                        self.mode = Mode::EditRepoUrl {
                            project_id: project.id.clone(),
                            input: project.repo_url.clone(),
                        };
                    }
                }
            }
            KeyCode::Char('T') => {
                if let Some(idx) = list_state.selected() {
                    if let Some(project) = projects.get(idx) {
                        if project.repo_url.is_empty() {
                            self.status_message = Some("Set repo URL first (r)".into());
                            self.mode = Mode::ProjectList {
                                projects,
                                list_state,
                            };
                        } else {
                            self.mode = Mode::EditRepoToken {
                                project_id: project.id.clone(),
                                input: String::new(),
                            };
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_new_project(
        &mut self,
        key: KeyEvent,
        mut name: String,
        mut slug: String,
        field: ProjectField,
    ) {
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                let next_field = match field {
                    ProjectField::Name => ProjectField::Slug,
                    ProjectField::Slug => ProjectField::Name,
                };
                self.mode = Mode::NewProject {
                    name,
                    slug,
                    field: next_field,
                };
            }
            KeyCode::Enter => {
                match field {
                    ProjectField::Name => {
                        // Auto-generate slug from name if slug is empty
                        if slug.is_empty() {
                            slug = name
                                .trim()
                                .to_lowercase()
                                .replace(|c: char| !c.is_alphanumeric(), "-");
                        }
                        self.mode = Mode::NewProject {
                            name,
                            slug,
                            field: ProjectField::Slug,
                        };
                    }
                    ProjectField::Slug => {
                        let name_trimmed = name.trim().to_string();
                        let slug_trimmed = slug.trim().to_string();
                        if !name_trimmed.is_empty() && !slug_trimmed.is_empty() {
                            match self.service.create_project(&CreateProject {
                                name: name_trimmed,
                                slug: slug_trimmed,
                                description: String::new(),
                                repo_url: String::new(),
                            }) {
                                Ok(project) => {
                                    self.switch_project(project);
                                    self.status_message =
                                        Some(format!("Created: {}", self.project.name));
                                }
                                Err(e) => {
                                    self.status_message = Some(format!("Error: {e}"));
                                    self.mode = Mode::Normal;
                                }
                            }
                        } else {
                            self.mode = Mode::Normal;
                        }
                    }
                }
            }
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                match field {
                    ProjectField::Name => {
                        name.pop();
                    }
                    ProjectField::Slug => {
                        slug.pop();
                    }
                }
                self.mode = Mode::NewProject { name, slug, field };
            }
            KeyCode::Char(c) => {
                match field {
                    ProjectField::Name => name.push(c),
                    ProjectField::Slug => slug.push(c),
                }
                self.mode = Mode::NewProject { name, slug, field };
            }
            _ => {}
        }
    }

    fn handle_confirm_delete_project(&mut self, key: KeyEvent, project: Project) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                match self.service.delete_project(&project.id) {
                    Ok(()) => {
                        self.status_message = Some(format!("Deleted: {}", project.name));
                    }
                    Err(e) => self.status_message = Some(format!("Error: {e}")),
                }
                self.mode = Mode::Normal;
            }
            _ => self.mode = Mode::Normal,
        }
    }

    fn handle_claude_action_pick(&mut self, key: KeyEvent, task: Task) {
        // Subtasks only support build and verify actions
        if task.is_subtask() {
            match key.code {
                KeyCode::Char('b') | KeyCode::Char('v') => {
                    // Fall through to main match below
                }
                KeyCode::Esc => {
                    self.mode = Mode::TaskDetail { task };
                    return;
                }
                _ => {
                    self.status_message =
                        Some("Subtasks only support Build and Verify".into());
                    self.mode = Mode::TaskDetail { task };
                    return;
                }
            }
        }
        match key.code {
            KeyCode::Char('r') => {
                match self.service.trigger_claude_run(&task.id, "research") {
                    Ok(run) => {
                        self.status_message = Some("Claude researching...".into());
                        self.mode = Mode::ClaudeRunning {
                            task,
                            run_id: run.id,
                            progress: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Char('d') => {
                match self.service.trigger_claude_run(&task.id, "design") {
                    Ok(run) => {
                        self.status_message = Some("Claude designing...".into());
                        self.mode = Mode::ClaudeRunning {
                            task,
                            run_id: run.id,
                            progress: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Char('p') => {
                if task.spec_status != ApprovalStatus::Approved {
                    self.status_message = Some(format!(
                        "Plan requires approved spec (current: {})",
                        task.spec_status.display_name()
                    ));
                    self.mode = Mode::TaskDetail { task };
                    return;
                }
                match self.service.trigger_claude_run(&task.id, "plan") {
                    Ok(run) => {
                        self.status_message = Some("Claude planning...".into());
                        self.mode = Mode::ClaudeRunning {
                            task,
                            run_id: run.id,
                            progress: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Char('b') => {
                match self.service.trigger_claude_run(&task.id, "build") {
                    Ok(run) => {
                        self.status_message = Some("Claude building...".into());
                        self.mode = Mode::ClaudeRunning {
                            task,
                            run_id: run.id,
                            progress: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Char('v') => {
                match self.service.trigger_claude_run(&task.id, "verify") {
                    Ok(run) => {
                        self.status_message = Some("Claude verifying...".into());
                        self.mode = Mode::ClaudeRunning {
                            task,
                            run_id: run.id,
                            progress: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Char('R') => {
                match self.service.trigger_claude_run(&task.id, "research_distill") {
                    Ok(run) => {
                        self.status_message = Some("Claude refining research...".into());
                        self.mode = Mode::ClaudeRunning {
                            task,
                            run_id: run.id,
                            progress: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Char('D') => {
                match self.service.trigger_claude_run(&task.id, "design_distill") {
                    Ok(run) => {
                        self.status_message = Some("Claude refining design...".into());
                        self.mode = Mode::ClaudeRunning {
                            task,
                            run_id: run.id,
                            progress: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Char('P') => {
                match self.service.trigger_claude_run(&task.id, "plan_distill") {
                    Ok(run) => {
                        self.status_message = Some("Claude refining plan...".into());
                        self.mode = Mode::ClaudeRunning {
                            task,
                            run_id: run.id,
                            progress: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Char('V') => {
                match self.service.trigger_claude_run(&task.id, "verify_distill") {
                    Ok(run) => {
                        self.status_message = Some("Claude refining verification...".into());
                        self.mode = Mode::ClaudeRunning {
                            task,
                            run_id: run.id,
                            progress: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Esc => self.mode = Mode::TaskDetail { task },
            _ => {}
        }
    }

    fn handle_feedback_input(&mut self, key: KeyEvent, task: Task, field: String, mut input: String) {
        match key.code {
            KeyCode::Enter => {
                let feedback_update = match field.as_str() {
                    "research" => UpdateTask {
                        research_feedback: Some(input),
                        research_status: Some(ApprovalStatus::Rejected),
                        ..Default::default()
                    },
                    "spec" | "design" => UpdateTask {
                        spec_feedback: Some(input),
                        spec_status: Some(ApprovalStatus::Rejected),
                        ..Default::default()
                    },
                    "plan" => UpdateTask {
                        plan_feedback: Some(input),
                        plan_status: Some(ApprovalStatus::Rejected),
                        ..Default::default()
                    },
                    "verify" => UpdateTask {
                        verify_feedback: Some(input),
                        verify_status: Some(ApprovalStatus::Rejected),
                        ..Default::default()
                    },
                    _ => UpdateTask::default(),
                };
                match self.service.update_task(&task.id, &feedback_update) {
                    Ok(updated) => {
                        self.refresh();
                        self.status_message = Some(format!("{field} rejected with feedback"));
                        self.mode = Mode::TaskDetail { task: updated };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Esc => {
                self.mode = Mode::TaskDetail { task };
            }
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::FeedbackInput { task, field, input };
            }
            KeyCode::Char(c) => {
                input.push(c);
                self.mode = Mode::FeedbackInput { task, field, input };
            }
            _ => {}
        }
    }

    fn handle_claude_output(&mut self, key: KeyEvent, task: Task, output: String, mut scroll: u16) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                // Reload task to reflect any status changes
                match self.service.get_task(&task.id) {
                    Ok(t) => self.mode = Mode::TaskDetail { task: t },
                    Err(_) => self.mode = Mode::TaskDetail { task },
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                scroll = scroll.saturating_add(1);
                self.mode = Mode::ClaudeOutput {
                    task,
                    output,
                    scroll,
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                scroll = scroll.saturating_sub(1);
                self.mode = Mode::ClaudeOutput {
                    task,
                    output,
                    scroll,
                };
            }
            _ => {}
        }
    }

    fn handle_approval_pick(&mut self, key: KeyEvent, task: Task, field: String) {
        match key.code {
            KeyCode::Char('a') => {
                let update = match field.as_str() {
                    "research" => UpdateTask {
                        research_status: Some(ApprovalStatus::Approved),
                        ..Default::default()
                    },
                    "spec" => UpdateTask {
                        spec_status: Some(ApprovalStatus::Approved),
                        ..Default::default()
                    },
                    "plan" => UpdateTask {
                        plan_status: Some(ApprovalStatus::Approved),
                        ..Default::default()
                    },
                    "verify" => UpdateTask {
                        verify_status: Some(ApprovalStatus::Approved),
                        ..Default::default()
                    },
                    _ => UpdateTask::default(),
                };
                match self.service.update_task(&task.id, &update) {
                    Ok(updated) => {
                        self.refresh();
                        self.status_message = Some(format!("{field} approved"));
                        self.mode = Mode::TaskDetail { task: updated };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Char('r') => {
                let update = match field.as_str() {
                    "research" => UpdateTask {
                        research_status: Some(ApprovalStatus::Rejected),
                        ..Default::default()
                    },
                    "spec" => UpdateTask {
                        spec_status: Some(ApprovalStatus::Rejected),
                        ..Default::default()
                    },
                    "plan" => UpdateTask {
                        plan_status: Some(ApprovalStatus::Rejected),
                        ..Default::default()
                    },
                    "verify" => UpdateTask {
                        verify_status: Some(ApprovalStatus::Rejected),
                        ..Default::default()
                    },
                    _ => UpdateTask::default(),
                };
                match self.service.update_task(&task.id, &update) {
                    Ok(updated) => {
                        self.refresh();
                        self.status_message = Some(format!("{field} rejected"));
                        self.mode = Mode::TaskDetail { task: updated };
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {e}"));
                        self.mode = Mode::TaskDetail { task };
                    }
                }
            }
            KeyCode::Esc => self.mode = Mode::TaskDetail { task },
            _ => {}
        }
    }

    fn handle_view_scroll(&mut self, key: KeyEvent, task: Task, mut scroll: u16, kind: &str) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::TaskDetail { task };
            }
            KeyCode::Char('j') | KeyCode::Down => {
                scroll = scroll.saturating_add(1);
                match kind {
                    "spec" => self.mode = Mode::ViewSpec { task, scroll },
                    "plan" => self.mode = Mode::ViewPlan { task, scroll },
                    "research" => self.mode = Mode::ViewResearch { task, scroll },
                    "verification" => self.mode = Mode::ViewVerification { task, scroll },
                    _ => self.mode = Mode::ViewSpec { task, scroll },
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                scroll = scroll.saturating_sub(1);
                match kind {
                    "spec" => self.mode = Mode::ViewSpec { task, scroll },
                    "plan" => self.mode = Mode::ViewPlan { task, scroll },
                    "research" => self.mode = Mode::ViewResearch { task, scroll },
                    "verification" => self.mode = Mode::ViewVerification { task, scroll },
                    _ => self.mode = Mode::ViewSpec { task, scroll },
                }
            }
            _ => {}
        }
    }

    fn handle_edit_repo_token(&mut self, key: KeyEvent, project_id: String, mut input: String) {
        match key.code {
            KeyCode::Enter => {
                let token = input.trim().to_string();
                if token.is_empty() {
                    self.status_message = Some("Token cannot be empty".into());
                } else {
                    match self.service.set_repo_token(&project_id, &token) {
                        Ok(()) => {
                            self.status_message = Some("Repo token saved (encrypted)".into());
                        }
                        Err(e) => self.status_message = Some(format!("Error: {e}")),
                    }
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::EditRepoToken { project_id, input };
            }
            KeyCode::Char(c) => {
                input.push(c);
                self.mode = Mode::EditRepoToken { project_id, input };
            }
            _ => {}
        }
    }

    fn handle_health(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('r') => {
                let checks = self.run_health_checks();
                self.mode = Mode::Health { checks };
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    fn run_health_checks(&self) -> Vec<HealthCheck> {
        let mut checks = Vec::new();

        // 1. Server connectivity
        let server = match self.service.health_check() {
            Ok(()) => HealthCheck {
                name: "Server".into(),
                status: CheckStatus::Passed,
                detail: "Connected".into(),
            },
            Err(e) => HealthCheck {
                name: "Server".into(),
                status: CheckStatus::Failed,
                detail: format!("{e}"),
            },
        };
        checks.push(server);

        // 2. Runner(s) via system_status
        match self.service.system_status() {
            Ok(status) => {
                if status.runners.is_empty() {
                    checks.push(HealthCheck {
                        name: "Runner".into(),
                        status: CheckStatus::Failed,
                        detail: "No runners connected".into(),
                    });
                } else {
                    for r in &status.runners {
                        let detail = if r.connected {
                            format!("{} (connected)", r.runner_id)
                        } else {
                            format!("{} (last seen: {})", r.runner_id, r.last_seen)
                        };
                        checks.push(HealthCheck {
                            name: "Runner".into(),
                            status: if r.connected {
                                CheckStatus::Passed
                            } else {
                                CheckStatus::Failed
                            },
                            detail,
                        });
                    }
                }
            }
            Err(_) => {
                checks.push(HealthCheck {
                    name: "Runner".into(),
                    status: CheckStatus::Failed,
                    detail: "Could not fetch status".into(),
                });
            }
        }

        // 3. Repo token — check via get_repo_token (returns error if not set)
        if !self.project.repo_url.is_empty() {
            let has_token = self.service.get_repo_token(&self.project.id).is_ok();
            checks.push(HealthCheck {
                name: "Repo Token".into(),
                status: if has_token {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Failed
                },
                detail: if has_token {
                    "Configured".into()
                } else {
                    "Not set (P > T to configure)".into()
                },
            });
        }

        // 4. Git
        let git = match std::process::Command::new("git")
            .arg("--version")
            .output()
        {
            Ok(out) if out.status.success() => {
                let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                HealthCheck {
                    name: "Git".into(),
                    status: CheckStatus::Passed,
                    detail: ver,
                }
            }
            _ => HealthCheck {
                name: "Git".into(),
                status: CheckStatus::Failed,
                detail: "Not found".into(),
            },
        };
        checks.push(git);

        // 4. Claude CLI
        let claude = match std::process::Command::new("claude")
            .arg("--version")
            .output()
        {
            Ok(out) if out.status.success() => {
                let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                HealthCheck {
                    name: "Claude CLI".into(),
                    status: CheckStatus::Passed,
                    detail: ver,
                }
            }
            _ => HealthCheck {
                name: "Claude CLI".into(),
                status: CheckStatus::Failed,
                detail: "Not found".into(),
            },
        };
        checks.push(claude);

        // 5. GitHub CLI auth
        let gh = match std::process::Command::new("gh")
            .args(["auth", "status"])
            .output()
        {
            Ok(out) if out.status.success() => HealthCheck {
                name: "GitHub CLI".into(),
                status: CheckStatus::Passed,
                detail: "Authenticated".into(),
            },
            Ok(out) => {
                let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
                HealthCheck {
                    name: "GitHub CLI".into(),
                    status: CheckStatus::Failed,
                    detail: if msg.is_empty() {
                        "Not authenticated".into()
                    } else {
                        msg
                    },
                }
            }
            _ => HealthCheck {
                name: "GitHub CLI".into(),
                status: CheckStatus::Failed,
                detail: "Not found".into(),
            },
        };
        checks.push(gh);

        checks
    }

    fn handle_sprint_list(
        &mut self,
        key: KeyEvent,
        sprints: Vec<Sprint>,
        mut list_state: ListState,
    ) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => {
                let i = list_state.selected().unwrap_or(0);
                if i + 1 < sprints.len() {
                    list_state.select(Some(i + 1));
                }
                self.mode = Mode::SprintList {
                    sprints,
                    list_state,
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = list_state.selected().unwrap_or(0);
                if i > 0 {
                    list_state.select(Some(i - 1));
                }
                self.mode = Mode::SprintList {
                    sprints,
                    list_state,
                };
            }
            KeyCode::Enter => {
                if let Some(idx) = list_state.selected() {
                    if let Some(sprint) = sprints.get(idx) {
                        let name = sprint.name.clone();
                        self.active_sprint = Some(sprint.clone());
                        self.refresh();
                        self.status_message = Some(format!("Sprint: {name}"));
                        self.mode = Mode::Normal;
                    }
                }
            }
            KeyCode::Char('n') => {
                self.mode = Mode::NewSprint {
                    input: String::new(),
                };
            }
            KeyCode::Char('d') => {
                if let Some(idx) = list_state.selected() {
                    if let Some(sprint) = sprints.get(idx) {
                        let name = sprint.name.clone();
                        let id = sprint.id.clone();
                        // If deleting the active sprint, clear the filter
                        if self.active_sprint.as_ref().map(|s| &s.id) == Some(&id) {
                            self.active_sprint = None;
                        }
                        match self.service.delete_sprint(&id) {
                            Ok(()) => {
                                self.refresh();
                                self.status_message = Some(format!("Deleted sprint: {name}"));
                            }
                            Err(e) => {
                                self.status_message = Some(format!("Error: {e}"));
                            }
                        }
                        self.mode = Mode::Normal;
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_new_sprint(&mut self, key: KeyEvent, mut input: String) {
        match key.code {
            KeyCode::Enter => {
                let name = input.trim().to_string();
                if !name.is_empty() {
                    match self.service.create_sprint(&CreateSprint {
                        project_id: self.project.id.clone(),
                        name: name.clone(),
                        goal: String::new(),
                        starts_at: None,
                        ends_at: None,
                    }) {
                        Ok(_) => {
                            self.status_message = Some(format!("Sprint created: {name}"));
                        }
                        Err(e) => self.status_message = Some(format!("Error: {e}")),
                    }
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::NewSprint { input };
            }
            KeyCode::Char(c) => {
                input.push(c);
                self.mode = Mode::NewSprint { input };
            }
            _ => {}
        }
    }

    fn handle_new_subtask(&mut self, key: KeyEvent, parent: Task, mut input: String) {
        match key.code {
            KeyCode::Enter => {
                let title = input.trim().to_string();
                if !title.is_empty() {
                    match self.service.create_task(&CreateTask {
                        project_id: parent.project_id.clone(),
                        title,
                        description: String::new(),
                        status: Status::Todo,
                        priority: parent.priority,
                        parent_id: Some(parent.id.clone()),
                        reviewer: String::new(),
                    }) {
                        Ok(_) => {
                            self.refresh();
                            self.status_message = Some("Subtask created".into());
                        }
                        Err(e) => self.status_message = Some(format!("Error: {e}")),
                    }
                }
                // Return to task detail with refreshed data
                match self.service.get_task(&parent.id) {
                    Ok(t) => self.mode = Mode::TaskDetail { task: t },
                    Err(_) => self.mode = Mode::TaskDetail { task: parent },
                }
            }
            KeyCode::Esc => {
                self.mode = Mode::TaskDetail { task: parent };
            }
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::NewSubtask { parent, input };
            }
            KeyCode::Char(c) => {
                input.push(c);
                self.mode = Mode::NewSubtask { parent, input };
            }
            _ => {}
        }
    }

    fn handle_edit_repo_url(&mut self, key: KeyEvent, project_id: String, mut input: String) {
        match key.code {
            KeyCode::Enter => {
                let url = input.trim().to_string();
                match self.service.update_project(
                    &project_id,
                    &UpdateProject {
                        repo_url: Some(url),
                        ..Default::default()
                    },
                ) {
                    Ok(updated) => {
                        if updated.id == self.project.id {
                            self.project = updated;
                        }
                        self.status_message = Some("Repo URL updated".into());
                    }
                    Err(e) => self.status_message = Some(format!("Error: {e}")),
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::EditRepoUrl { project_id, input };
            }
            KeyCode::Char(c) => {
                input.push(c);
                self.mode = Mode::EditRepoUrl { project_id, input };
            }
            _ => {}
        }
    }

    // ── Rendering ──

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);

        self.render_title_bar(frame, layout[0]);
        self.board.render(frame, layout[1]);
        self.render_status_bar(frame, layout[2]);

        // Overlays
        match &self.mode {
            Mode::Normal => {}
            Mode::NewTask { input } => self.render_input_bar(frame, "New task: ", input, area),
            Mode::TaskDetail { task } => self.render_task_detail(frame, task, area),
            Mode::EditTitle { input, .. } => {
                self.render_input_bar(frame, "Title: ", input, area)
            }
            Mode::EditDescription { input, .. } => {
                self.render_description_editor(frame, input, area)
            }
            Mode::ConfirmDelete { task } => self.render_confirm_delete_dialog(frame, task, area),
            Mode::PriorityPick { current, .. } => {
                self.render_priority_pick(frame, *current, area)
            }
            Mode::ProjectList {
                projects,
                list_state,
            } => self.render_project_list(frame, projects, list_state, area),
            Mode::NewProject { name, slug, field } => {
                self.render_new_project(frame, name, slug, *field, area)
            }
            Mode::ConfirmDeleteProject { project } => {
                self.render_confirm_delete_project(frame, project, area)
            }
            Mode::ClaudeActionPick { task } => {
                self.render_claude_action_pick(frame, task, area)
            }
            Mode::ClaudeRunning {
                run_id, progress, ..
            } => {
                self.render_claude_running(frame, run_id, progress.as_deref(), area)
            }
            Mode::Health { checks } => self.render_health(frame, checks, area),
            Mode::ClaudeOutput { output, scroll, .. } => {
                self.render_scrollable_text(frame, " Claude Output ", output, *scroll, area)
            }
            Mode::ApprovalPick { field, .. } => {
                self.render_approval_pick(frame, field, area)
            }
            Mode::ViewSpec { task, scroll } => {
                let content = self
                    .service
                    .read_task_spec(&task.id)
                    .unwrap_or_else(|_| "(error reading spec)".into());
                let display = if content.trim().is_empty() {
                    "(no specification yet)".into()
                } else {
                    content
                };
                self.render_scrollable_text(frame, " Specification ", &display, *scroll, area)
            }
            Mode::ViewPlan { task, scroll } => {
                let content = self
                    .service
                    .read_task_plan(&task.id)
                    .unwrap_or_else(|_| "(error reading plan)".into());
                let display = if content.trim().is_empty() {
                    "(no plan yet)".into()
                } else {
                    content
                };
                self.render_scrollable_text(frame, " Plan ", &display, *scroll, area)
            }
            Mode::ViewResearch { task, scroll } => {
                let content = self
                    .service
                    .read_task_research(&task.id)
                    .unwrap_or_else(|_| "(error reading research)".into());
                let display = if content.trim().is_empty() {
                    "(no research yet)".into()
                } else {
                    content
                };
                self.render_scrollable_text(frame, " Research ", &display, *scroll, area)
            }
            Mode::ViewVerification { task, scroll } => {
                let content = self
                    .service
                    .read_task_verification(&task.id)
                    .unwrap_or_else(|_| "(error reading verification)".into());
                let display = if content.trim().is_empty() {
                    "(no verification yet)".into()
                } else {
                    content
                };
                self.render_scrollable_text(frame, " Verification ", &display, *scroll, area)
            }
            Mode::FeedbackInput { input, field, .. } => {
                self.render_input_bar(frame, &format!("Feedback ({field}): "), input, area)
            }
            Mode::EditRepoUrl { input, .. } => {
                self.render_input_bar(frame, "Repo URL: ", input, area)
            }
            Mode::EditRepoToken { input, .. } => {
                let masked: String = "*".repeat(input.len());
                self.render_input_bar(frame, "Repo Token (PAT): ", &masked, area)
            }
            Mode::SprintList {
                sprints,
                list_state,
            } => self.render_sprint_list(frame, sprints, list_state, area),
            Mode::NewSprint { input } => {
                self.render_input_bar(frame, "New sprint: ", input, area)
            }
            Mode::NewSubtask { input, .. } => {
                self.render_input_bar(frame, "New subtask: ", input, area)
            }
        }
    }

    fn render_title_bar(&self, frame: &mut Frame, area: Rect) {
        let slug_display = format!(" ({})", self.project.slug);
        let mut spans = vec![
            Span::styled(" flowstate ", Style::default().bold().fg(Color::Cyan)),
            Span::raw("| "),
            Span::styled(&self.project.name, Style::default().fg(Color::Yellow)),
            Span::styled(slug_display, Style::default().fg(Color::DarkGray)),
        ];
        if let Some(ref sprint) = self.active_sprint {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("Sprint: {}", sprint.name),
                Style::default().fg(Color::Magenta),
            ));
        }
        let title = Line::from(spans);
        frame.render_widget(title, area);
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        if let Some(ref msg) = self.status_message {
            let line = Line::from(Span::styled(
                format!(" {msg}"),
                Style::default().fg(Color::Green),
            ));
            frame.render_widget(line, area);
            return;
        }

        let hints = match &self.mode {
            Mode::Normal => vec![
                ("q", "quit"),
                ("h/l", "cols"),
                ("j/k", "tasks"),
                ("n", "new"),
                ("Enter", "detail"),
                ("m/M", "move"),
                ("d", "del"),
                ("p", "priority"),
                ("P", "projects"),
                ("x", "sprints"),
                ("X", "clear sprint"),
                ("H", "health"),
            ],
            Mode::NewTask { .. } => vec![("Enter", "create"), ("Esc", "cancel")],
            Mode::TaskDetail { .. } => vec![
                ("t", "title"),
                ("e", "desc"),
                ("n", "subtask"),
                ("p", "priority"),
                ("m", "move"),
                ("d", "del"),
                ("c", "claude"),
                ("s/S", "spec"),
                ("i/I", "plan"),
                ("w/W", "research"),
                ("v/V", "verify"),
                ("a", "approve"),
                ("Esc", "back"),
            ],
            Mode::EditTitle { .. } => vec![("Enter", "save"), ("Esc", "cancel")],
            Mode::EditDescription { .. } => vec![("Ctrl+S", "save"), ("Esc", "cancel")],
            Mode::ConfirmDelete { .. } | Mode::ConfirmDeleteProject { .. } => {
                vec![("y", "confirm"), ("any", "cancel")]
            }
            Mode::PriorityPick { .. } => vec![
                ("1", "urgent"),
                ("2", "high"),
                ("3", "medium"),
                ("4", "low"),
                ("5", "none"),
            ],
            Mode::ProjectList { .. } => vec![
                ("j/k", "nav"),
                ("Enter", "switch"),
                ("n", "new"),
                ("r", "repo url"),
                ("T", "repo token"),
                ("d", "del"),
                ("Esc", "back"),
            ],
            Mode::NewProject { .. } => {
                vec![("Tab", "next field"), ("Enter", "confirm"), ("Esc", "cancel")]
            }
            Mode::ClaudeActionPick { .. } => vec![
                ("r", "research"),
                ("d", "design"),
                ("p", "plan"),
                ("b", "build"),
                ("v", "verify"),
                ("R/D/P/V", "distill"),
                ("Esc", "cancel"),
            ],
            Mode::ClaudeRunning { .. } => vec![("Esc", "background")],
            Mode::ClaudeOutput { .. } | Mode::ViewSpec { .. } | Mode::ViewPlan { .. } | Mode::ViewResearch { .. } | Mode::ViewVerification { .. } => {
                vec![("j/k", "scroll"), ("Esc", "back")]
            }
            Mode::FeedbackInput { .. } => vec![("Enter", "submit"), ("Esc", "cancel")],
            Mode::ApprovalPick { .. } => vec![
                ("a", "approve"),
                ("r", "reject"),
                ("Esc", "cancel"),
            ],
            Mode::EditRepoUrl { .. } | Mode::EditRepoToken { .. } => {
                vec![("Enter", "save"), ("Esc", "cancel")]
            }
            Mode::Health { .. } => vec![("r", "refresh"), ("Esc", "back")],
            Mode::SprintList { .. } => vec![
                ("j/k", "nav"),
                ("Enter", "select"),
                ("n", "new"),
                ("d", "del"),
                ("Esc", "back"),
            ],
            Mode::NewSprint { .. } => vec![("Enter", "create"), ("Esc", "cancel")],
            Mode::NewSubtask { .. } => vec![("Enter", "create"), ("Esc", "cancel")],
        };

        let spans: Vec<Span> = hints
            .into_iter()
            .flat_map(|(key, desc)| {
                vec![
                    Span::styled(
                        format!(" {key}"),
                        Style::default().fg(Color::Yellow).bold(),
                    ),
                    Span::raw(format!(" {desc} ")),
                ]
            })
            .collect();

        frame.render_widget(Line::from(spans), area);
    }

    fn render_input_bar(&self, frame: &mut Frame, label: &str, input: &str, area: Rect) {
        let input_area = Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(3),
            width: area.width,
            height: 3,
        };
        frame.render_widget(Clear, input_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(label);
        let paragraph = Paragraph::new(input).block(block);
        frame.render_widget(paragraph, input_area);
    }

    fn render_task_detail(&self, frame: &mut Frame, task: &Task, area: Rect) {
        let popup = centered_rect(60, 70, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" Task Detail ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Title: ", Style::default().bold()),
                Span::raw(&task.title),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Status: ", Style::default().bold()),
                Span::raw(task.status.display_name()),
            ]),
            Line::from(vec![
                Span::styled("Priority: ", Style::default().bold()),
                Span::styled(
                    task.priority.display_name(),
                    priority_style(task.priority),
                ),
            ]),
        ];

        // Approval statuses
        lines.push(Line::from(vec![
            Span::styled("Research: ", Style::default().bold()),
            Span::styled(
                task.research_status.display_name(),
                approval_style(task.research_status),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Spec: ", Style::default().bold()),
            Span::styled(
                task.spec_status.display_name(),
                approval_style(task.spec_status),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Plan: ", Style::default().bold()),
            Span::styled(
                task.plan_status.display_name(),
                approval_style(task.plan_status),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Verify: ", Style::default().bold()),
            Span::styled(
                task.verify_status.display_name(),
                approval_style(task.verify_status),
            ),
        ]));

        if !task.reviewer.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Reviewer: ", Style::default().bold()),
                Span::raw(&task.reviewer),
            ]));
        }

        // Show parent info if this is a subtask
        if let Some(ref parent_id) = task.parent_id {
            if let Ok(parent) = self.service.get_task(parent_id) {
                lines.push(Line::from(vec![
                    Span::styled("Parent: ", Style::default().bold()),
                    Span::styled(parent.title.clone(), Style::default().fg(Color::Cyan)),
                ]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Description:", Style::default().bold())));
        lines.push(Line::from(if task.description.is_empty() {
            "(none)"
        } else {
            &task.description
        }));

        // Show children (subtasks) if any
        if let Ok(children) = self.service.list_child_tasks(&task.id) {
            if !children.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Subtasks ({}):", children.len()),
                    Style::default().bold(),
                )));
                for child in &children {
                    lines.push(Line::from(vec![
                        Span::styled("  - ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("[{}] ", child.status.display_name()),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(child.title.clone()),
                    ]));
                }
            }
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }

    fn render_description_editor(&self, frame: &mut Frame, input: &str, area: Rect) {
        let popup = centered_rect(70, 50, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" Edit Description (Ctrl+S save, Esc cancel) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(input)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, popup);
    }

    fn render_confirm_delete_dialog(&self, frame: &mut Frame, task: &Task, area: Rect) {
        let popup = centered_rect(50, 20, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" Confirm Delete ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let text = format!("Delete \"{}\"?\n\n(y)es / (any key) cancel", task.title);
        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Center);
        frame.render_widget(paragraph, popup);
    }

    fn render_priority_pick(&self, frame: &mut Frame, current: Priority, area: Rect) {
        let popup = centered_rect(30, 30, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" Set Priority ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let priorities = [
            (Priority::Urgent, "1"),
            (Priority::High, "2"),
            (Priority::Medium, "3"),
            (Priority::Low, "4"),
            (Priority::None, "5"),
        ];

        let lines: Vec<Line> = priorities
            .iter()
            .map(|(p, key)| {
                let marker = if *p == current { "> " } else { "  " };
                Line::from(vec![
                    Span::raw(marker),
                    Span::styled(
                        format!("[{key}] "),
                        Style::default().fg(Color::Yellow).bold(),
                    ),
                    Span::styled(p.display_name(), priority_style(*p)),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, popup);
    }

    fn render_project_list(
        &self,
        frame: &mut Frame,
        projects: &[Project],
        list_state: &ListState,
        area: Rect,
    ) {
        let popup = centered_rect(50, 50, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" Projects ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));

        let items: Vec<ListItem> = projects
            .iter()
            .map(|p| {
                let marker = if p.id == self.project.id {
                    "* "
                } else {
                    "  "
                };
                let mut spans = vec![
                    Span::styled(marker, Style::default().fg(Color::Cyan)),
                    Span::styled(&p.name, Style::default().bold()),
                    Span::styled(
                        format!(" ({})", p.slug),
                        Style::default().fg(Color::DarkGray),
                    ),
                ];
                if !p.repo_url.is_empty() {
                    spans.push(Span::styled(
                        format!(" {}", p.repo_url),
                        Style::default().fg(Color::Blue),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Magenta)
                    .bold(),
            )
            .highlight_symbol("> ");

        let mut state = list_state.clone();
        frame.render_stateful_widget(list, popup, &mut state);
    }

    fn render_sprint_list(
        &self,
        frame: &mut Frame,
        sprints: &[Sprint],
        list_state: &ListState,
        area: Rect,
    ) {
        let popup = centered_rect(50, 50, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" Sprints ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));

        let items: Vec<ListItem> = sprints
            .iter()
            .map(|s| {
                let marker = if self.active_sprint.as_ref().map(|a| &a.id) == Some(&s.id) {
                    "* "
                } else {
                    "  "
                };
                let spans = vec![
                    Span::styled(marker, Style::default().fg(Color::Cyan)),
                    Span::styled(&s.name, Style::default().bold()),
                    Span::styled(
                        format!(" ({})", s.status),
                        Style::default().fg(Color::DarkGray),
                    ),
                ];
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Magenta)
                    .bold(),
            )
            .highlight_symbol("> ");

        let mut state = list_state.clone();
        frame.render_stateful_widget(list, popup, &mut state);
    }

    fn render_new_project(
        &self,
        frame: &mut Frame,
        name: &str,
        slug: &str,
        field: ProjectField,
        area: Rect,
    ) {
        let popup = centered_rect(50, 30, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" New Project ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));

        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let name_style = match field {
            ProjectField::Name => Style::default().fg(Color::Cyan).bold(),
            ProjectField::Slug => Style::default(),
        };
        let slug_style = match field {
            ProjectField::Slug => Style::default().fg(Color::Cyan).bold(),
            ProjectField::Name => Style::default(),
        };

        let lines = vec![
            Line::from(vec![
                Span::styled("Name: ", name_style),
                Span::raw(name),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Slug: ", slug_style),
                Span::raw(slug),
            ]),
        ];

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    fn render_confirm_delete_project(&self, frame: &mut Frame, project: &Project, area: Rect) {
        let popup = centered_rect(50, 20, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" Delete Project ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let text = format!(
            "Delete project \"{}\"?\nAll tasks will be deleted!\n\n(y)es / (any key) cancel",
            project.name
        );
        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Center);
        frame.render_widget(paragraph, popup);
    }

    fn render_claude_action_pick(&self, frame: &mut Frame, task: &Task, area: Rect) {
        let popup = centered_rect(50, 50, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" Claude Action ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        let research_ok = true;
        let design_ok = task.research_status == ApprovalStatus::Approved;
        let plan_ok = task.spec_status == ApprovalStatus::Approved;
        let build_ok = task.spec_status == ApprovalStatus::Approved && task.plan_status == ApprovalStatus::Approved;

        let action_line = |key: &str, name: &str, available: bool, note: &str| -> Line {
            let style = if available {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let text_style = if available { Style::default() } else { Style::default().fg(Color::DarkGray) };
            let suffix = if !available && !note.is_empty() {
                format!(" ({})", note)
            } else {
                String::new()
            };
            Line::from(vec![
                Span::styled(format!("[{key}] "), style),
                Span::styled(format!("{name}{suffix}"), text_style),
            ])
        };

        let lines = vec![
            Line::from(Span::styled("  Primary Actions", Style::default().bold())),
            action_line("r", "Research", research_ok, ""),
            action_line("d", "Design", design_ok, "research not approved"),
            action_line("p", "Plan", plan_ok, "spec not approved"),
            action_line("b", "Build", build_ok, "spec+plan not approved"),
            action_line("v", "Verify", true, ""),
            Line::from(""),
            Line::from(Span::styled("  Distill (Refine)", Style::default().bold())),
            action_line("R", "Research Distill", task.research_status != ApprovalStatus::None, "no research"),
            action_line("D", "Design Distill", task.spec_status != ApprovalStatus::None, "no spec"),
            action_line("P", "Plan Distill", task.plan_status != ApprovalStatus::None, "no plan"),
            action_line("V", "Verify Distill", task.verify_status != ApprovalStatus::None, "no verification"),
        ];

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, popup);
    }

    fn render_claude_running(
        &self,
        frame: &mut Frame,
        run_id: &str,
        progress: Option<&str>,
        area: Rect,
    ) {
        let popup = centered_rect(50, 20, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" Claude Run ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let progress_text = progress.unwrap_or("Starting...");

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Status:   ", Style::default().bold()),
                Span::styled("Running", Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::styled("  Progress: ", Style::default().bold()),
                Span::styled(progress_text, Style::default().fg(Color::Yellow)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Run ID:   ", Style::default().bold()),
                Span::styled(run_id, Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  Press Esc to background",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }

    fn render_health(&self, frame: &mut Frame, checks: &[HealthCheck], area: Rect) {
        let popup = centered_rect(55, 35, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(" System Health ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let mut lines = vec![Line::from("")];

        for check in checks {
            let (icon, icon_style) = match check.status {
                CheckStatus::Passed => ("  OK ", Style::default().fg(Color::Green).bold()),
                CheckStatus::Failed => ("  FAIL ", Style::default().fg(Color::Red).bold()),
            };

            lines.push(Line::from(vec![
                Span::styled(icon, icon_style),
                Span::styled(
                    format!("{:<12}", check.name),
                    Style::default().bold(),
                ),
                Span::raw(&check.detail),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  r = refresh, Esc = back",
            Style::default().fg(Color::DarkGray),
        )));

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }

    fn render_scrollable_text(
        &self,
        frame: &mut Frame,
        title: &str,
        content: &str,
        scroll: u16,
        area: Rect,
    ) {
        let popup = centered_rect(80, 80, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(content)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));
        frame.render_widget(paragraph, popup);
    }

    fn render_approval_pick(&self, frame: &mut Frame, field: &str, area: Rect) {
        let popup = centered_rect(40, 20, area);
        frame.render_widget(Clear, popup);

        let title = format!(" Approve {} ", field);
        let block = Block::default()
            .title(title.as_str())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let lines = vec![
            Line::from(vec![
                Span::styled("[a] ", Style::default().fg(Color::Green).bold()),
                Span::raw("Approve"),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("[r] ", Style::default().fg(Color::Red).bold()),
                Span::raw("Reject"),
            ]),
        ];

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, popup);
    }
}

fn next_status(s: Status) -> Option<Status> {
    match s {
        Status::Todo => Some(Status::Research),
        Status::Research => Some(Status::Design),
        Status::Design => Some(Status::Plan),
        Status::Plan => Some(Status::Build),
        Status::Build => Some(Status::Verify),
        Status::Verify => Some(Status::Done),
        Status::Done => None,
        Status::Cancelled => None,
    }
}

fn prev_status(s: Status) -> Option<Status> {
    match s {
        Status::Todo => None,
        Status::Research => Some(Status::Todo),
        Status::Design => Some(Status::Research),
        Status::Plan => Some(Status::Design),
        Status::Build => Some(Status::Plan),
        Status::Verify => Some(Status::Build),
        Status::Done => Some(Status::Verify),
        Status::Cancelled => None,
    }
}

fn priority_style(p: Priority) -> Style {
    match p {
        Priority::Urgent => Style::default().fg(Color::Red).bold(),
        Priority::High => Style::default().fg(Color::LightRed),
        Priority::Medium => Style::default().fg(Color::Yellow),
        Priority::Low => Style::default().fg(Color::Blue),
        Priority::None => Style::default().fg(Color::DarkGray),
    }
}

fn approval_style(s: ApprovalStatus) -> Style {
    match s {
        ApprovalStatus::None => Style::default().fg(Color::DarkGray),
        ApprovalStatus::Pending => Style::default().fg(Color::Yellow),
        ApprovalStatus::Approved => Style::default().fg(Color::Green),
        ApprovalStatus::Rejected => Style::default().fg(Color::Red),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
