//! Dragging a project's label along the status bar to move the project, as
//! `:order` does. The press goes to the project, as a click does, and holds
//! it; each label the pointer moves onto gives up its place at once, so the
//! bar drawn after each motion shows where it will land. A label is passed
//! only in the direction the pointer is moving: labels differ in width, and
//! after a short one passes a long one the pointer is over the long one
//! again, which must not send it back.

use crossterm::event::MouseEvent;

use super::super::Effect;
use super::{MouseDragTarget, ProjectDrag};
use crate::keymap::ProjectAction;
use crate::ui::status_segments::StatusSegment;

impl super::super::App {
    /// A press on a project's label: the click, then the drag it may start.
    pub(super) fn press_project_label(
        &mut self,
        ev: MouseEvent,
        action: ProjectAction,
    ) -> Vec<Effect> {
        let effects = self.click_project(action);
        self.begin_project_drag(ev, action);
        effects
    }

    /// Hold the project a press on its label went to. A press that didn't
    /// go there (refused, or a label past the ninth) holds nothing, nor does
    /// one with a single project open.
    fn begin_project_drag(&mut self, ev: MouseEvent, action: ProjectAction) {
        let ProjectAction::ByIndex(n) = action else {
            return;
        };
        let id = self.state.active;
        let from = usize::from(n).saturating_sub(1);
        let here = self.state.open_projects().position(|p| p.id == id);
        if here != Some(from) || self.state.open_projects().nth(1).is_none() {
            return;
        }
        self.view.project_drag = Some(ProjectDrag {
            id,
            from,
            col: ev.column,
        });
        self.view.mouse_selection = Some(MouseDragTarget::Project);
    }

    /// Move the held project to the place of the label under the pointer,
    /// when the pointer is moving towards it.
    pub(super) fn extend_project_drag(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some(drag) = self.view.project_drag.as_mut() else {
            return Vec::new();
        };
        let was = std::mem::replace(&mut drag.col, ev.column);
        let id = drag.id;
        let Some(StatusSegment::ProjectLabel(to)) = self.status_segment_under(ev) else {
            return Vec::new();
        };
        let Some(at) = self.state.open_projects().position(|p| p.id == id) else {
            return Vec::new();
        };
        let towards = (to < at && ev.column < was) || (to > at && ev.column > was);
        if towards {
            self.state.move_project(id, to);
        }
        Vec::new()
    }

    /// Let go: the project stays where the bar shows it, and the message says
    /// where that is, as `:order` does, when it moved.
    pub(super) fn finish_project_drag(&mut self) -> Vec<Effect> {
        self.view.mouse_selection = None;
        let Some(drag) = self.view.project_drag.take() else {
            return Vec::new();
        };
        let Some((at, project)) = self
            .state
            .open_projects()
            .enumerate()
            .find(|(_, p)| p.id == drag.id)
        else {
            return Vec::new();
        };
        if at != drag.from {
            let label = project.label();
            self.state
                .flash_info(format!("order: {label} is project {}", at + 1));
        }
        Vec::new()
    }
}
