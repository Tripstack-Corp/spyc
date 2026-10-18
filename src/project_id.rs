//! `ProjectId`, the identity of a project (#99). A leaf, so the MCP layer can
//! carry one across its channel without depending on the app.

/// Identity of a project — a uuid, never an index (an index drifts when a
/// project closes) or a path (a path changes identity on a re-home). `Copy`:
/// cheap to carry wherever "which project" needs naming. It crosses the MCP
/// socket thread and the recovery manifest as its hyphenated text (`Display` /
/// `FromStr`).
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

impl serde::Serialize for ProjectId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for ProjectId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
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

    #[test]
    fn an_id_survives_the_manifest() {
        let id = ProjectId::new();
        let json = serde_json::to_value(id).unwrap();
        assert_eq!(json, serde_json::json!(id.to_string()));
        assert_eq!(serde_json::from_value::<ProjectId>(json).ok(), Some(id));
        assert!(serde_json::from_value::<ProjectId>(serde_json::json!("nope")).is_err());
    }
}
