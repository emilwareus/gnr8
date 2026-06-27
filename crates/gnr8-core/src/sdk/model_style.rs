//! SDK model emitter styles.

/// Python SDK model implementation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PyModelStyle {
    /// Pydantic v2 `BaseModel` models. This is the preferred/default Python SDK surface.
    #[default]
    Pydantic,
    /// Stdlib `@dataclass` models. Kept for no-dependency consumers.
    Dataclass,
}

impl PyModelStyle {
    /// Pydantic v2 model style.
    #[must_use]
    pub const fn pydantic() -> Self {
        Self::Pydantic
    }

    /// Stdlib dataclass model style.
    #[must_use]
    pub const fn dataclass() -> Self {
        Self::Dataclass
    }

    /// Whether this style emits Pydantic models.
    #[must_use]
    pub const fn is_pydantic(self) -> bool {
        matches!(self, Self::Pydantic)
    }
}
