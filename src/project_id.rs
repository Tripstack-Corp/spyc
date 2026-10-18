//! `ProjectId`, the identity of a project (#99). A leaf, so the MCP layer can
//! carry one across its channel without depending on the app.

/// Identity of a project — a uuid, never an index (an index drifts when a
/// project closes) or a path (a path changes identity on a re-home). `Copy`:
/// cheap to carry wherever "which project" needs naming. It crosses the MCP
/// socket thread as its hyphenated text (`Display` / `FromStr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectId(uuid::Uuid);

impl ProjectId {
    /// A fresh id. Named `new`, not `default`, so a `ProjectId` is never
    /// conjured implicitly — every one names a real, just-created project.
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for ProjectId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_survives_its_text() {
        let id = ProjectId::new();
        assert_eq!(id.to_string().parse::<ProjectId>().ok(), Some(id));
        assert!("not-a-uuid".parse::<ProjectId>().is_err());
    }
}
