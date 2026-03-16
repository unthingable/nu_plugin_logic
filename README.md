# nu_plugin_logic

A Nushell plugin that adds `solve` — describe relationships with shared variables across any number of sources, get all valid combinations back. No loops, no join direction, no intermediate tables.

## Install

```sh
cargo build --release
plugin add ./target/release/nu_plugin_logic
plugin use logic
```

## Commands

### `solve` — pattern matching and multi-source joins

**Single-source**: filter and decompose pipeline data.

```nushell
# Filter by literal values, extract variables
ls | solve [type:file &name &size]

# Equivalent longer forms
ls | solve [type file, name &name, size &size]
ls | solve {type: "file", name: "&name", size: "&size"}

# String decomposition — split on structure
ls src/**/* | solve [type:file name:&path.&ext] | select path ext size
```

**Pattern list syntax** — elements are parsed left to right:

| Form | Meaning | Example |
|---|---|---|
| `&var` | Extract field as same-named variable | `&name` |
| `key:value` | Match field against value | `type:file` |
| `key:&var` | Extract field as named variable | `pid:&p` |
| `key:&a.&b` | Decompose field with string pattern | `name:&stem.&ext` |
| `key value` | Match field (bare pair, legacy) | `type file` |

String patterns support multiple variables: `&name.&ext` splits `"main.rs"` into `name=main`, `ext=rs`. Variables capture up to the next literal delimiter. The last variable captures the remainder.

Record syntax works too — `{type: "file", name: "&path.&ext"}` is equivalent to `[type:file name:&path.&ext]`.

**Multi-source** (inline): pass data and patterns together in one list. Shared variable names across patterns become join conditions.

```nushell
let proc = [{pid: 1, name: "nginx"}, {pid: 2, name: "postgres"}]
let ports = [{pid: 1, port: 80}, {pid: 1, port: 443}, {pid: 2, port: 5432}]

solve [$proc [&pid &name] $ports [&pid &port]]
# => pid | name     | port
#      1 | nginx    |   80
#      1 | nginx    |  443
#      2 | postgres | 5432
```

Subexpressions work as inline sources — no `let` or `facts` needed:

```nushell
solve [(ps) [&pid &name] (ss -tlnp | from ssv) [&pid &port]]
```

Any number of sources. Variables scoped naturally — use `let` or `do { }` blocks.

```nushell
# Three-way join
solve [
  $proc [&pid &name]
  $ports [&pid &port]
  $deploy [&name &env]
]
```

**Multi-source** (fact store): for interactive sessions. Use `@name` to reference stored facts. Useful when you want to load data once and run multiple queries.

```nushell
ps | facts proc
ss -tlnp | from ssv | facts ports

solve [@proc [&pid &name] @ports [&pid &port]]
```

**Mixed sources**: combine inline data with stored facts in the same query.

```nushell
solve [$fresh_data [&pid &name] @stored_ports [&pid &port]]
```

**Multi-source** (pipeline): pass a record-of-tables. Pattern uses record syntax.

```nushell
{proc: $proc, ports: $ports} | solve {
  proc: {pid: "&pid", name: "&name"},
  ports: {pid: "&pid", port: "&port"}
}
```

**Composability**: `solve` returns a standard Nushell table. Pipe into `where`, `sort-by`, `select`, `first`, etc.

```nushell
solve [$proc [&pid &name &mem] $ports [&pid &port]]
  | where port < 1024 | sort-by mem -r
```

Results stream lazily — `first N` short-circuits after N solutions.

### `facts` — named data store

One command, behavior determined by context:

```nushell
# Store (with pipeline input — passes data through)
ls | facts files | where size > 1kb

# Retrieve (no pipeline input)
facts files

# List all registered fact sets
facts

# Remove one
facts files --drop

# Remove all
facts --clear
```

Facts persist for the Nushell session (the plugin process lifetime). They're useful for interactive exploration — load data once, run multiple `solve` queries against it, inspect with `facts` to see what's registered.

## Pattern syntax

Patterns can use list syntax or record syntax `{k: v}`. List elements are parsed left to right — each element is self-describing.

| List element | Meaning |
|---|---|
| `&name` | Extract field `name` as variable `name` |
| `type:file` | Field `type` must equal `"file"` |
| `name:&stem.&ext` | Decompose field `name` with string pattern |
| `pid:&p` | Extract field `pid` as variable `p` |
| `type file` | Bare pair — field `type` must equal `"file"` (legacy) |
| `name &f` | Bare pair — extract field `name` as variable `f` (legacy) |

Record syntax `{k: v}` works identically — `{type: "file", name: "&stem.&ext"}`.

**String patterns**: `&name.&ext` splits `"main.rs"` into `name=main`, `ext=rs`. Variables capture up to the next literal delimiter. The last variable captures the remainder. `&a.&b` against `"x.y.z"` gives `a=x`, `b=y.z`.

**Prefix conventions:**
- `&x` — logic variable (output: the plugin binds this during matching)
- `$x` — nushell variable (input: evaluated before the plugin sees it)
- `@x` — fact store reference (resolved by the plugin from stored data)

## When to use solve vs join

| Scenario | `join` | `solve` |
|---|---|---|
| Two sources, one shared key | Works well | Works, no advantage |
| Two sources, multiple shared keys | Polars or manual loops | Handles naturally |
| Three+ sources | Chain joins, manage intermediates | One pattern |
| String decomposition | `path parse`, manual `insert` | `&stem.&ext` |
| Filter + extract | `where` + `get` chains | One pattern |

## Known limitations

- **Fact scoping**: facts are global to the session. Use inline sources (`solve [$data [pattern]]`) for scoped queries, or `facts --clear` to clean up.
- **No arithmetic or conditions in patterns**: use `where` after `solve` for filtering on computed values.
- **Cross-product risk**: joining large sources (N×M×K) can produce many results. Use `first N` to limit, or filter sources before joining.
- **Type-strict joins**: variables match by exact type. `pid=593` (int) won't unify with `pid="593"` (string). When joining sources with different types for the same field (common with `from ssv`, `from csv`, etc.), coerce first: `| update pid {into int}`.
- **Column name case**: field matching is case-sensitive. Use `key:&var` to bridge sources with different conventions: `PID:&pid` matches uppercase `PID` and joins with lowercase `pid`.

## Architecture

Hand-rolled unification + backtracking engine behind a `LogicEngine` trait. The trait boundary exists so the engine can be swapped for a real Prolog runtime (Trealla via Wasmtime or Scryer) if the hand-rolled engine outgrows itself (rules, negation-as-failure, conditions).

The search is an iterative depth-first `SearchIterator` that yields one solution at a time. Sources are searched smallest-first for early pruning.
