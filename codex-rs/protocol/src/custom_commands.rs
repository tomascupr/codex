use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

/// Stable identifier for a custom command.
///
/// This is a normalized, lowercase, kebab-case string suitable for lookups
/// and persistence. Construction/normalization is performed in `codex-core`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct CustomCommandId(pub String);

/// Visibility for a custom command in UIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CustomCommandVisibility {
    /// Shown in popups/menus (default)
    Popup,
    /// Hidden from menus; still invocable by name
    Hidden,
}

impl Default for CustomCommandVisibility {
    fn default() -> Self {
        Self::Popup
    }
}

/// Declaration of an argument accepted by a custom command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CustomCommandArgSpec {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Serialized specification for a custom command.
///
/// This is the cross-crate representation. Runtime fields like source paths
/// live in `codex-core`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct CustomCommandSpec {
    pub id: CustomCommandId,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub description: String,
    /// Template/body text after YAML frontmatter.
    pub template: String,
    #[serde(default)]
    pub args: Vec<CustomCommandArgSpec>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub visibility: CustomCommandVisibility,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn serde_roundtrip_spec() {
        let spec = CustomCommandSpec {
            id: CustomCommandId("hello-world".into()),
            name: "Hello World".into(),
            aliases: vec!["hi".into(), "hello".into()],
            description: "Greet the world".into(),
            template: "Hello $1".into(),
            args: vec![CustomCommandArgSpec {
                name: "name".into(),
                required: false,
                description: Some("Optional name".into()),
            }],
            tags: vec!["greeting".into()],
            version: Some("1".into()),
            disabled: false,
            visibility: CustomCommandVisibility::Popup,
        };

        let json = serde_json::to_string(&spec).unwrap();
        let back: CustomCommandSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, back);
    }
}
