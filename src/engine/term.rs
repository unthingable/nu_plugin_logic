use nu_protocol::Value;

/// A pattern term used for unification against Nushell values.
#[derive(Debug, Clone)]
pub enum Term {
    /// Must match exactly (compared via Value's PartialEq, which ignores spans).
    Literal(Value),
    /// Binds to the matched value, or checks consistency if already bound.
    Variable(String),
    /// Open record pattern: each field must match, extra fields in the value are ignored.
    Record(Vec<(String, Term)>),
    /// String decomposition pattern, e.g. "&stem.rs".
    StringPattern(Vec<StringPatternPart>),
}

#[derive(Debug, Clone)]
pub enum StringPatternPart {
    Literal(String),
    Variable(String),
}
