//! Configurable SDK output layout.
//!
//! The emitters own language-specific code; this module only names the file-structure policy that
//! built-in SDK targets apply after generation. Keeping the policy explicit lets small projects keep a
//! compact SDK while larger APIs choose a navigable, split layout.

/// How an SDK target maps generated API shapes to files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkFileLayout {
    kind: SdkFileLayoutKind,
    operation_split: OperationFileSplit,
    operation_dir: Option<String>,
    model_dir: Option<String>,
    operation_file_template: Option<String>,
    model_file_template: Option<String>,
}

impl SdkFileLayout {
    /// Compact single-file-per-concern layout.
    #[must_use]
    pub fn compact() -> Self {
        Self {
            kind: SdkFileLayoutKind::Compact,
            operation_split: OperationFileSplit::Compact,
            operation_dir: None,
            model_dir: None,
            operation_file_template: None,
            model_file_template: None,
        }
    }

    /// Split navigable layout for larger APIs.
    #[must_use]
    pub fn split() -> Self {
        Self {
            kind: SdkFileLayoutKind::Split,
            operation_split: OperationFileSplit::PerTag,
            operation_dir: None,
            model_dir: None,
            operation_file_template: None,
            model_file_template: None,
        }
    }

    /// Place split operation files under a relative directory.
    ///
    /// `None` keeps operation files at the SDK package root. Unsafe paths are rejected when materialized
    /// by the SDK bundle writer, so this method stays a plain configuration setter.
    #[must_use]
    pub fn operation_dir<S>(mut self, dir: S) -> Self
    where
        S: Into<String>,
    {
        self.operation_dir = Some(dir.into());
        self
    }

    /// Keep split operation files at the SDK package root.
    #[must_use]
    pub fn root_operations(mut self) -> Self {
        self.operation_dir = None;
        self
    }

    /// Keep operation methods in the compact client/operations file while other split layout features,
    /// such as split model files, can still be used.
    #[must_use]
    pub const fn compact_operations(mut self) -> Self {
        self.operation_split = OperationFileSplit::Compact;
        self
    }

    /// Emit split client operation files grouped by operation tag/group.
    ///
    /// The tag/group is [`crate::graph::Operation::group`], usually set by
    /// [`crate::sdk::builtins::GroupOperations`] or imported from the first OpenAPI tag. Untagged
    /// operations use the synthetic `"default"` group.
    #[must_use]
    pub const fn operations_per_tag(mut self) -> Self {
        self.operation_split = OperationFileSplit::PerTag;
        self
    }

    /// Emit one split client operation file per endpoint/operation.
    #[must_use]
    pub const fn operations_per_endpoint(mut self) -> Self {
        self.operation_split = OperationFileSplit::PerEndpoint;
        self
    }

    /// Place split model files under a relative directory.
    ///
    /// For TypeScript and Python this is usually `"models"`; Go defaults to package-root model files.
    #[must_use]
    pub fn model_dir<S>(mut self, dir: S) -> Self
    where
        S: Into<String>,
    {
        self.model_dir = Some(dir.into());
        self
    }

    /// Keep split model files at the SDK package root.
    #[must_use]
    pub fn root_models(mut self) -> Self {
        self.model_dir = None;
        self
    }

    /// Set the split operation file path template.
    ///
    /// Supported placeholders are `{operation}`, `{operation_snake}`, `{operation_kebab}`,
    /// `{service}`, `{service_snake}`, and `{service_kebab}`. `service` comes from
    /// [`crate::sdk::builtins::GroupOperations`], or `"default"` when an operation has no group.
    /// Templates are validated when the SDK is generated.
    #[must_use]
    pub fn operation_file_template<S>(mut self, template: S) -> Self
    where
        S: Into<String>,
    {
        self.operation_file_template = Some(template.into());
        self
    }

    /// Set the split model file path template.
    ///
    /// Supported placeholders are `{schema}`, `{schema_snake}`, and `{schema_kebab}`. Templates are
    /// validated when the SDK is generated.
    #[must_use]
    pub fn model_file_template<S>(mut self, template: S) -> Self
    where
        S: Into<String>,
    {
        self.model_file_template = Some(template.into());
        self
    }

    /// Whether this layout is split.
    #[must_use]
    pub const fn is_split(&self) -> bool {
        matches!(self.kind, SdkFileLayoutKind::Split)
    }

    /// How split layouts distribute generated client operation methods across files.
    #[must_use]
    pub const fn operation_split(&self) -> OperationFileSplit {
        self.operation_split
    }

    /// Relative directory for split operation files, if configured.
    #[must_use]
    pub fn operation_dir_ref(&self) -> Option<&str> {
        self.operation_dir.as_deref()
    }

    /// Relative directory for split model files, if configured.
    #[must_use]
    pub fn model_dir_ref(&self) -> Option<&str> {
        self.model_dir.as_deref()
    }

    /// Optional split operation file path template.
    #[must_use]
    pub fn operation_file_template_ref(&self) -> Option<&str> {
        self.operation_file_template.as_deref()
    }

    /// Optional split model file path template.
    #[must_use]
    pub fn model_file_template_ref(&self) -> Option<&str> {
        self.model_file_template.as_deref()
    }
}

impl Default for SdkFileLayout {
    fn default() -> Self {
        Self::compact()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SdkFileLayoutKind {
    Compact,
    Split,
}

/// How a split SDK layout distributes generated client operation methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationFileSplit {
    /// Keep operation methods in the compact client/operations file.
    Compact,
    /// Emit one operation file per tag/group.
    PerTag,
    /// Emit one operation file per endpoint/operation.
    PerEndpoint,
}

#[cfg(test)]
mod tests {
    use super::SdkFileLayout;

    #[test]
    fn split_layout_can_configure_operation_and_model_directories() {
        let layout = SdkFileLayout::split()
            .operation_dir("apis")
            .model_dir("models");
        assert!(layout.is_split());
        assert_eq!(layout.operation_dir_ref(), Some("apis"));
        assert_eq!(layout.model_dir_ref(), Some("models"));
    }

    #[test]
    fn split_layout_can_configure_file_templates() {
        let layout = SdkFileLayout::split()
            .operation_file_template("resources/{service_snake}/{operation_snake}.ts")
            .model_file_template("types/{schema_snake}.ts");
        assert_eq!(
            layout.operation_file_template_ref(),
            Some("resources/{service_snake}/{operation_snake}.ts")
        );
        assert_eq!(
            layout.model_file_template_ref(),
            Some("types/{schema_snake}.ts")
        );
    }

    #[test]
    fn split_layout_defaults_to_package_root_files() {
        let layout = SdkFileLayout::split();
        assert!(layout.is_split());
        assert_eq!(layout.operation_split(), super::OperationFileSplit::PerTag);
        assert_eq!(layout.operation_dir_ref(), None);
        assert_eq!(layout.model_dir_ref(), None);
    }

    #[test]
    fn split_layout_can_choose_endpoint_operation_files() {
        let layout = SdkFileLayout::split().operations_per_endpoint();
        assert_eq!(
            layout.operation_split(),
            super::OperationFileSplit::PerEndpoint
        );
    }

    #[test]
    fn split_layout_can_keep_operations_compact() {
        let layout = SdkFileLayout::split().compact_operations();
        assert_eq!(layout.operation_split(), super::OperationFileSplit::Compact);
    }
}
