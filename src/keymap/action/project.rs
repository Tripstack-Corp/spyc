//! [`ProjectAction`], the projects' share of the [`super::Action`] vocabulary.

/// What [`super::Action::Project`] does (#99): the project in view's home, and the
/// open projects themselves. One variant of `Action` for all of them, so each
/// naming table carries one arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, strum::EnumIter)]
pub enum ProjectAction {
    /// `Space p` — jump to PROJECT_HOME.
    #[default]
    JumpHome,
    /// `gP` / `Space P` — set PROJECT_HOME to the current directory.
    SetHomeHere,
    /// `Space n` — open a directory as a project.
    Open,
    /// `Space x` — close the project in view.
    Close,
    /// `Space j` — list the open projects, to go to one.
    List,
    /// `Space !` / `^a !` — go to the agent that needs you, in any project.
    Attention,
    /// `Space Space` — go to the project the user was in last.
    Last,
    /// `Space ]` — go to the next open project, wrapping.
    Next,
    /// `Space [` — go to the previous open project, wrapping.
    Prev,
    /// `Space 1`..`Space 9` — go to open project N.
    ByIndex(u8),
}

impl ProjectAction {
    pub const fn describe(self) -> &'static str {
        match self {
            Self::JumpHome => "jump to PROJECT_HOME",
            Self::SetHomeHere => "set PROJECT_HOME to current dir",
            Self::Open => "open a directory as a project",
            Self::Close => "close the project in view",
            Self::List => "list the open projects",
            Self::Attention => "go to the agent that needs you",
            Self::Last => "go to the last project",
            Self::Next => "go to the next project",
            Self::Prev => "go to the previous project",
            Self::ByIndex(_) => "go to project N",
        }
    }

    pub const fn canonical_name(self) -> &'static str {
        match self {
            Self::JumpHome => "jump_project_home",
            Self::SetHomeHere => "set_project_home_here",
            Self::Open => "project_open",
            Self::Close => "project_close",
            Self::List => "project_list",
            Self::Attention => "project_attention",
            Self::Last => "project_last",
            Self::Next => "project_next",
            Self::Prev => "project_prev",
            Self::ByIndex(_) => "project_by_index",
        }
    }

    /// The action `name` names, a parametric one in its default form
    /// (`ByIndex(1)`).
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "jump_project_home" => Self::JumpHome,
            "set_project_home_here" => Self::SetHomeHere,
            "project_open" => Self::Open,
            "project_close" => Self::Close,
            "project_list" => Self::List,
            "project_attention" => Self::Attention,
            "project_last" => Self::Last,
            "project_next" => Self::Next,
            "project_prev" => Self::Prev,
            "project_by_index" => Self::ByIndex(1),
            _ => return None,
        })
    }
}
