// Group navigation and local search for the TUI task board.
// Exports App key helpers for project headers, task selection, and search.
// Deps: App state, tree_data rows, crossterm key codes.

use super::*;

impl App {
    pub(super) fn handle_board_navigation(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.tree_mode = false,
            KeyCode::Char('/') => {
                self.search_mode = true;
                self.search_query.clear();
            }
            KeyCode::Char('n') => self.move_search(1),
            KeyCode::Char('N') => self.move_search(-1),
            KeyCode::Char('r') => {
                self.reload_tasks()?;
                self.last_task_refresh = Instant::now();
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_within_group(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_within_group(-1),
            KeyCode::Left | KeyCode::Char('h') => self.jump_group(-1),
            KeyCode::Right | KeyCode::Char('l') => self.jump_group(1),
            KeyCode::Char(' ') => self.toggle_selected_group(),
            KeyCode::Home | KeyCode::Char('g') => self.select_first_row(),
            KeyCode::End | KeyCode::Char('G') => self.select_last_row(),
            KeyCode::Enter => self.open_selected_row()?,
            _ => {}
        }
        Ok(())
    }

    pub(super) fn handle_search_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.search_mode = false,
            KeyCode::Enter => {
                self.search_mode = false;
                self.move_search(1);
            }
            KeyCode::Backspace => {
                self.search_query.pop();
            }
            KeyCode::Char(character) => self.search_query.push(character),
            _ => {}
        }
        Ok(())
    }

    fn visible_nodes(&self) -> Vec<crate::tui::tree_data::TreeNode> {
        crate::tui::tree_data::build_task_tree_with_state(
            &self.tasks,
            &self.wg_creators,
            &self.collapsed_projects,
        )
    }

    fn move_within_group(&mut self, direction: i8) {
        let nodes = self.visible_nodes();
        let Some(current) = nodes.get(self.tree_selected) else {
            return;
        };
        let candidates: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                !node.is_group_header && node.project_id == current.project_id
            })
            .map(|(index, _)| index)
            .collect();
        if candidates.is_empty() {
            return;
        }
        let next = if current.is_group_header {
            if direction > 0 { candidates[0] } else { *candidates.last().unwrap_or(&candidates[0]) }
        } else {
            let position = candidates
                .iter()
                .position(|index| *index == self.tree_selected)
                .unwrap_or(0);
            let next_position = if direction > 0 {
                (position + 1) % candidates.len()
            } else if position == 0 {
                candidates.len() - 1
            } else {
                position - 1
            };
            candidates[next_position]
        };
        self.select_row(next, &nodes);
    }

    fn jump_group(&mut self, direction: i8) {
        let nodes = self.visible_nodes();
        let headers: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.is_group_header)
            .map(|(index, _)| index)
            .collect();
        if headers.is_empty() {
            return;
        }
        let current_group = nodes
            .get(self.tree_selected)
            .and_then(|node| node.project_id.as_ref());
        let current = headers
            .iter()
            .position(|index| nodes[*index].project_id.as_ref() == current_group)
            .unwrap_or(0);
        let next = if direction > 0 {
            (current + 1) % headers.len()
        } else if current == 0 {
            headers.len() - 1
        } else {
            current - 1
        };
        self.select_row(headers[next], &nodes);
    }

    fn toggle_selected_group(&mut self) {
        let nodes = self.visible_nodes();
        let Some(node) = nodes.get(self.tree_selected) else {
            return;
        };
        let project_id = node.project_id.clone();
        if !self.collapsed_projects.insert(project_id.clone()) {
            self.collapsed_projects.remove(&project_id);
        }
        let nodes = self.visible_nodes();
        self.tree_selected = nodes
            .iter()
            .position(|candidate| candidate.is_group_header && candidate.project_id == project_id)
            .unwrap_or(0);
    }

    fn select_first_row(&mut self) {
        let nodes = self.visible_nodes();
        if !nodes.is_empty() {
            self.select_row(0, &nodes);
        }
    }

    pub(super) fn select_first_task_if_header(&mut self) {
        let nodes = self.visible_nodes();
        if nodes
            .get(self.tree_selected)
            .is_some_and(|node| node.is_group_header)
        {
            self.move_within_group(1);
        }
    }

    fn select_last_row(&mut self) {
        let nodes = self.visible_nodes();
        if let Some(index) = nodes.len().checked_sub(1) {
            self.select_row(index, &nodes);
        }
    }

    fn move_search(&mut self, direction: i8) {
        if self.search_query.trim().is_empty() {
            return;
        }
        let matches: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| self.matches_search(task))
            .map(|(index, _)| index)
            .collect();
        if matches.is_empty() {
            return;
        }
        let current_task = self.selected_task().map(|task| task.id.as_str());
        let current = matches
            .iter()
            .position(|index| Some(self.tasks[*index].id.as_str()) == current_task);
        let next = if current.is_none() {
            0
        } else if direction > 0 {
            (current.unwrap_or(0) + 1) % matches.len()
        } else if current == Some(0) {
            matches.len() - 1
        } else {
            current.unwrap_or(0) - 1
        };
        let project_id = self.tasks[matches[next]].project_id.clone();
        self.collapsed_projects.remove(&project_id);
        let nodes = self.visible_nodes();
        if let Some(node_index) = nodes
            .iter()
            .position(|node| !node.is_group_header && node.task.id == self.tasks[matches[next]].id)
        {
            self.select_row(node_index, &nodes);
        }
    }

    fn matches_search(&self, task: &Task) -> bool {
        let query = self.search_query.to_lowercase();
        task.id.as_str().to_lowercase().contains(&query)
            || task.prompt.to_lowercase().contains(&query)
            || crate::project::project_display(task.project_id.as_deref())
                .to_lowercase()
                .contains(&query)
    }

    fn select_row(&mut self, index: usize, nodes: &[crate::tui::tree_data::TreeNode]) {
        self.tree_selected = index;
        if let Some(node) = nodes.get(index)
            && !node.is_group_header
            && let Some(task_index) = self.tasks.iter().position(|task| task.id == node.task.id)
        {
            self.selected = task_index;
        }
    }

    fn open_selected_row(&mut self) -> Result<()> {
        let nodes = self.visible_nodes();
        let Some(node) = nodes.get(self.tree_selected) else {
            return Ok(());
        };
        let task_id = if node.is_group_header {
            self.tasks
                .iter()
                .find(|task| task.project_id == node.project_id)
                .map(|task| task.id.clone())
        } else {
            Some(node.task.id.clone())
        };
        if let Some(task_id) = task_id
            && let Some(index) = self.tasks.iter().position(|task| task.id == task_id)
        {
            self.selected = index;
            self.enter_detail_mode()?;
        }
        Ok(())
    }
}
