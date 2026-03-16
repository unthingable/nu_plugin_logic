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

/// Collect all variable names from a `Term` in declaration order (depth-first,
/// left-to-right). Each name appears at most once; duplicates are skipped.
pub fn vars_in_term(term: &Term) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_vars(term, &mut out);
    out
}

fn collect_vars(term: &Term, out: &mut Vec<String>) {
    match term {
        Term::Literal(_) => {}
        Term::Variable(name) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        Term::Record(fields) => {
            for (_, child) in fields {
                collect_vars(child, out);
            }
        }
        Term::StringPattern(parts) => {
            for part in parts {
                if let StringPatternPart::Variable(name) = part
                    && !out.contains(name)
                {
                    out.push(name.clone());
                }
            }
        }
    }
}
