pub mod convert;
pub mod native;
pub mod search;
pub mod substitution;
pub mod term;
pub mod unify;

use nu_protocol::{Span, Value};

use term::Term;

/// Trait boundary for the logic engine.
///
/// MVP: NativeEngine (hand-rolled unification + backtracking).
/// Future: swap in Trealla-via-Wasmtime or Scryer for full Prolog support.
///
/// Both methods return lazy iterators so results stream through the pipeline.
/// `first 5` after `solve` will short-circuit after 5 solutions.
pub trait LogicEngine: Send + Sync {
    /// Single-source mode: filter input rows against a pattern,
    /// returning matches with variable bindings merged as new columns.
    fn filter(
        &self,
        pattern: Term,
        input: Vec<Value>,
        span: Span,
    ) -> Box<dyn Iterator<Item = Value> + Send>;

    /// Multi-source mode: backtracking search across resolved fact data.
    /// Returns one row per solution, columns are variable bindings.
    fn search(
        &self,
        sources: Vec<(Term, Vec<Value>)>,
        span: Span,
    ) -> Box<dyn Iterator<Item = Value> + Send>;
}
