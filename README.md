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
# Filter by literal values
ls | solve {type: "file"}

# Bind variables — $stem becomes a new column
ls | solve {type: "file", name: "$f"} | select f size

# String decomposition — split on structure
ls src/**/* | solve {type: "file", name: "$path.$ext"} | select path ext size
```

String patterns support multiple variables: `"$name.$ext"` splits `"main.rs"` into `name=main`, `ext=rs`. Variables capture up to the next literal delimiter. The last variable captures the remainder.

**Multi-source** (pipeline): pass a record-of-tables. Shared variable names across sources become join conditions.

```nushell
let proc = [{pid: 1, name: "nginx"}, {pid: 2, name: "postgres"}]
let ports = [{pid: 1, port: 80}, {pid: 1, port: 443}, {pid: 2, port: 5432}]

{proc: $proc, ports: $ports} | solve {
  proc: {pid: "$pid", name: "$name"},
  ports: {pid: "$pid", port: "$port"}
}
# => pid | name     | port
#      1 | nginx    |   80
#      1 | nginx    |  443
#      2 | postgres | 5432
```

This works with any number of sources. Variables scoped naturally — use `let` or `do { }` blocks.

```nushell
# Three-way join
{proc: $proc, ports: $ports, deploy: $deploy} | solve {
  proc: {pid: "$pid", name: "$name"},
  ports: {pid: "$pid", port: "$port"},
  deploy: {name: "$name", env: "$env"}
}
```

**Multi-source** (fact store): for interactive sessions where you want to store data and query it multiple times.

```nushell
ps | facts proc
ss -tlnp | from ssv | facts ports

solve {
  proc: {pid: "$pid", name: "$name"},
  ports: {pid: "$pid", port: "$port"}
}
```

Pipeline sources are checked first. If the input isn't a matching record-of-tables, `solve` falls back to the fact store.

**Composability**: `solve` returns a standard Nushell table. Pipe into `where`, `sort-by`, `select`, `first`, etc.

```nushell
{proc: $proc, ports: $ports} | solve {
  proc: {pid: "$pid", name: "$name", mem: "$mem"},
  ports: {pid: "$pid", port: "$port"}
} | where port < 1024 | sort-by mem -r
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

| Pattern | Meaning |
|---|---|
| `"$name"` | Logic variable — binds to matched value |
| `"hello"` | Literal — must match exactly |
| `"$stem.rs"` | String pattern — decomposes strings |
| `"$a.$b.$c"` | Multi-variable — splits on literal delimiters |
| `42`, `true` | Non-string literals — exact match |
| `{k: pat}` | Record pattern — each field matched, extras ignored |

Variables are `$`-prefixed strings. Nushell evaluates real `$variables` before the plugin sees them, so `"$pid"` is a string literal that the plugin interprets as a logic variable.

## When to use solve vs join

| Scenario | `join` | `solve` |
|---|---|---|
| Two sources, one shared key | Works well | Works, no advantage |
| Two sources, multiple shared keys | Polars or manual loops | Handles naturally |
| Three+ sources | Chain joins, manage intermediates | One pattern |
| String decomposition | `path parse`, manual `insert` | `"$stem.$ext"` |
| Filter + extract | `where` + `get` chains | One pattern |

## Known limitations

- **Fact scoping**: facts are global to the session. Use the pipeline approach (`{a: $a, b: $b} | solve {...}`) for scoped queries, or `facts --clear` to clean up.
- **Single-variable string patterns use leftmost matching**: `"$a.$b"` against `"x.y.z"` gives `a=x`, `b=y.z`. There's no backtracking within string patterns.
- **No arithmetic or conditions in patterns**: use `where` after `solve` for filtering on computed values.
- **Cross-product risk**: joining large fact sets (N×M×K) can produce many results. Use `first N` to limit, or filter sources before storing.

## Architecture

Hand-rolled unification + backtracking engine behind a `LogicEngine` trait. The trait boundary exists so the engine can be swapped for a real Prolog runtime (Trealla via Wasmtime or Scryer) if the hand-rolled engine outgrows itself (rules, negation-as-failure, conditions).

The search is an iterative depth-first `SearchIterator` that yields one solution at a time. Sources are searched smallest-first for early pruning.
