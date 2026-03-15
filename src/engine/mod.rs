pub mod convert;
pub mod native;
pub mod search;
pub mod substitution;
pub mod term;
pub mod unify;

use nu_protocol::{Span, Value};

use crate::store::FactStore;
use term::Term;

/// Trait boundary for the logic engine.
///
/// MVP: NativeEngine (hand-rolled unification + backtracking).
/// Future: swap in Trealla-via-Wasmtime or Scryer for full Prolog support.
pub trait LogicEngine: Send + Sync {
    /// Single-source mode: filter input rows against a pattern,
    /// returning matches with variable bindings merged as new columns.
    fn filter(&self, pattern: &Term, input: &[Value], span: Span) -> Vec<Value>;

    /// Multi-source mode: backtracking search across named fact sets.
    /// Returns a table of variable bindings (one row per solution).
    fn search(
        &self,
        queries: &[(String, Term)],
        store: &FactStore,
        span: Span,
    ) -> Result<Vec<Value>, String>;
}
