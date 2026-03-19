# nu_plugin_logic

Prolog-style pattern matching and relational search for Nushell. Prefix variables with `&`, describe the shape of your answer — `solve` finds every valid combination through unification and backtracking.

```nushell
let services = [
  {name: web, config: {port: 8080, host: localhost}, version: 2.1.3},
  {name: api, config: {port: 3000, host: 0.0.0.0}, version: 1.0.12}
]
let deploys = [{name: web, env: prod}, {name: api, env: staging}]

> solve [
    $services {
        name: &svc, config: {port: &port},
        version: &major.&minor.&patch
    }
    $deploys [&svc &env]
  ]
╭───┬─────┬──────┬───────┬───────┬───────┬─────────╮
│ # │ svc │ port │ major │ minor │ patch │   env   │
├───┼─────┼──────┼───────┼───────┼───────┼─────────┤
│ 0 │ web │ 8080 │ 2     │ 1     │ 3     │ prod    │
│ 1 │ api │ 3000 │ 1     │ 0     │ 12    │ staging │
╰───┴─────┴──────┴───────┴───────┴───────┴─────────╯
```

`solve` is not opinionated about syntax and supports several variants to choose from.

One expression: nested field access (`config.port`), string decomposition (version into semver parts), and cross-source join on `&svc`. Without `solve`, that's nested loops, manual parsing, and null checks.

## Install

```sh
cargo install nu_plugin_logic
plugin add ~/.cargo/bin/nu_plugin_logic
plugin use logic
```

Or build from source:

```sh
cargo build --release
plugin add ./target/release/nu_plugin_logic
plugin use logic
```

## Guide

- [Patterns filter rows](#patterns-filter-rows)
- [Variables extract values](#variables-extract-values)
- [String decomposition](#string-decomposition)
- [List syntax](#list-syntax)
- [Multiple sources](#multiple-sources) — joins, column mapping, self-joins, nesting
- [Fact store](#fact-store) — session-scoped named storage
- [Streaming and composability](#streaming-and-composability)
- [Gotchas](#gotchas) · [Reference](#reference) · [Roadmap](#roadmap)

---

### Patterns filter rows

The simplest `solve` filters a table with a record pattern:

```nushell
ls | solve {type: file} | select name size
```

Fields in the pattern must match literally — like `where type == file`, but expressed as a pattern. Fields not in the pattern are ignored and passed through.

> In current Nushell **quotes are mostly optional** — `file`, `admin`, `2.1.3` all parse as bare strings. Write them if you like, skip if you don't. Quotes are only required when a value contains spaces.

Multiple fields narrow the match:

```nushell
[{name: alice, role: admin}, {name: bob, role: user}, {name: carol, role: admin}]
  | solve {role: admin}
# => name  | role
#    alice | admin
#    carol | admin
```

### Variables bind and extract values

The `&` prefix denotes a logic variable. Similar to how `$` is an "input" variable that `solve` receives from Nushell, `&` is an "output" variable that comes out of `solve` as a column. Inside `solve` it's a logical variable that binds multiple possible values and powers joins.

```nushell
[{pid: 1, name: nginx, status: running}, {pid: 2, name: postgres, status: stopped}]
  | solve {status: running, name: &proc}
# => pid | name  | status  | proc
#      1 | nginx | running | nginx
```

`&proc` bound to `"nginx"` — the only row where `status` was `"running"`. The bound value appears as a new column named `proc`.

### String decomposition

When a pattern contains variables separated by literal characters, `solve` splits the string:

```nushell
ls src/ | solve {type: file, name: &stem.&ext}
```

`&stem.&ext` splits on `.` — against `"main.rs"`, you get `stem=main`, `ext=rs`. The last variable always captures the remainder, so `&a.&b` against `"x.y.z"` gives `a=x`, `b=y.z`.

In vanilla Nushell, you'd filter then merge parsed results back:

```nushell
ls src/ | where type == file
  | each { |row| $row | merge ($row.name | parse "{stem}.{ext}" | first) }
```

`solve` combines the filter and decomposition into one pattern.

### List syntax

List patterns are an ergonomic alternative to records:

```nushell
# These are equivalent:
ls | solve {type: file, name: &f}
ls | solve [type:file name:&f]
```

Colons and commas are optional readability aids — `[type:file name:&f]`, `[type file name &f]`, and `[type:file, name:&f]` all mean the same thing.

#### Optional field names

When the variable name matches the column, the column may be omitted: instead of `name:&name` you can simply write `&name`.

In space-delimited lists this leads to uneven pairing, but the parser always handles it correctly. Optional colon and comma can aid readability.

```nushell
ls | solve [type file &name &size]
ls | solve [type:file &name &size]
ls | solve [type file, &name, &size]

# Or, in vanilla record syntax:
ls | solve {type: file, name: &name, size: &size}
```

#### Omni syntax

The parser is strong enough to handle mixed syntax correctly:

```nushell
ls | solve [{type: file}, modified:&m, name &name, &size]
```

Use whatever makes most sense.

### Multiple sources

Everything above operates on a single pipeline. The real power of `solve` is searching across multiple sources at once.

Pass sources and patterns as alternating pairs:

```nushell
let procs = [{pid: 1, name: nginx}, {pid: 2, name: postgres}]
let ports = [{pid: 1, port: 80}, {pid: 1, port: 443}, {pid: 2, port: 5432}]

solve [$procs [&pid &name] $ports [&pid &port]]
# => pid | name     | port
#      1 | nginx    |   80
#      1 | nginx    |  443
#      2 | postgres | 5432
```

`&pid` appears in both patterns. For each process row, `solve` tries every port row and only yields combinations where `&pid` agrees. This is unification — the same variable mechanism, now joining data across sources.

Sources can be any Nushell expression — variables, subexpressions, commands:

```nushell
solve [(ps) [&pid &name &cpu] (open ports.csv) [&pid &port]]
```

#### Compared to `join`

For two sources with one shared key, Nushell's built-in `join` does the same thing:

```nushell
$procs | join $ports pid
```

`solve` pulls ahead with multiple join keys, three or more sources, nested field access, or when you need pattern matching and joins together. With `join`, you pick one key per step, manage intermediate tables, and clean up duplicate columns. With `solve`, you describe the relationships and the engine handles the search.

#### Self-joins

The same source can appear twice. Find each process alongside its parent's name:

```nushell
> let p = (ps)
> solve [$p [&ppid &name &cpu] $p [pid:&ppid name:&parent]]
    | where cpu > 0
    | select name parent cpu
    | first 5
╭───┬────────┬──────────────────────┬───────╮
│ # │  name  │        parent        │  cpu  │
├───┼────────┼──────────────────────┼───────┤
│ 0 │ nu     │ zsh                  │  1.96 │
│ 1 │ cmux   │ claude               │ 14.42 │
│ 2 │ claude │ nu                   │  0.03 │
│ ...                                       │
╰───┴────────┴──────────────────────┴───────╯
```

Without `solve`:

```nushell
$p | where cpu > 0 | each { |child|
  let parent = ($p | where pid == $child.ppid)
  if ($parent | is-empty) { null } else {
    {name: $child.name, parent: ($parent | first | get name), cpu: $child.cpu}
  }
} | compact
```

#### Joining different column names

In the procs/ports example, both sources had a column called `pid` — the variable `&pid` matched both naturally. When column names differ across sources, `key: &var` maps them to a shared variable:

```nushell
let hosts = [{name: db1, rack: A}, {name: db2, rack: B}]
let alerts = [{host: db1, level: warn}, {host: db2, level: crit}]

solve [$hosts [name:&h &rack] $alerts [host:&h &level]]
# => h   | rack | level
#    db1  | A    | warn
#    db2  | B    | crit
```

`name:&h` binds the `name` column to variable `&h`. In the alerts pattern, `host:&h` binds the `host` column to the same variable. The join works because both patterns share `&h` — even though the columns are called `name` and `host`. This is the equivalent of SQL's `ON hosts.name = alerts.host`.

The `&var` shorthand (`&pid`) is just the common case where the column name and variable name happen to match — it's sugar for `pid:&pid`.

#### Nested records

Patterns reach into nested structure:

```nushell
let hosts = [
  {name: db1, spec: {cores: 4, mem: 16}, rack: A},
  {name: db2, spec: {cores: 8, mem: 32}, rack: B}
]
let alerts = [{host: db1, level: warn}, {host: db2, level: crit}]

solve {$hosts {name: &host, spec: {cores: &cores}} $alerts [host:&host &level]}
# => host | cores | level
#    db1  | 4     | warn
#    db2  | 8     | crit

# listified:
solve [$hosts [name &host spec [cores &cores]] $alerts [host:&host &level]]
```

The hero example at the top combines nesting with string decomposition and multi-source joins — now you can see how each piece works.

### Fact store

`facts` provides session-scoped named storage for repeated queries — load data once, query it multiple ways.

```nushell
ps | facts procs
open ports.csv | facts ports

solve [@procs [&pid &name] @ports [&pid &port]]
```

`facts` passes data through, so it doubles as a store-and-continue:

```nushell
ps | facts procs | where cpu > 10    # stores AND continues the pipeline
```

Store the same name again to replace the data:

```nushell
ps | facts procs    # refresh with current state
```

Inspect and manage:

```nushell
facts                 # list all stored facts (name and row count)
facts procs --drop    # remove one → {name: procs, rows: 127}
facts --clear         # remove all → list of what was cleared
```

Mix `@`-referenced facts with inline data:

```nushell
solve [$fresh_data [&pid &name] @stored_ports [&pid &port]]
```

For most cases, plain Nushell variables work fine — `solve [$data [...]]` is simpler. `facts` earns its keep when you're iterating on queries at the REPL, or storing as a side effect mid-pipeline.

### Streaming and composability

`solve` returns a standard Nushell table. Pipe into `where`, `sort-by`, `select`, `first`, or anything else:

```nushell
solve [$procs [&pid &name &mem] $ports [&pid &port]]
  | where port < 1024
  | sort-by mem -r
  | first 10
```

Results stream lazily — the engine produces one solution at a time, so `first N` short-circuits without computing the rest.

## Gotchas

**Type-strict joins.** Unification compares types exactly: `pid=593` (int) won't match `pid="593"` (string). This comes up when joining `ps` (integer pids) with CSV data (string pids). Coerce before solving:

```nushell
let ports = (open ports.csv | update pid {into int})
solve [$procs [&pid &name] $ports [&pid &port]]
```

**String decomposition is greedy-to-first.** `&a-&b` against `"web-prod-abc"` gives `a=web`, `b=prod-abc` — the first variable captures up to the first delimiter match. If your data has delimiters inside values, the split may not land where you expect.

**Multi-source results contain only bound variables.** This is by design — explicit `&` binding avoids column name collisions across sources. The `&var` shorthand keeps it concise: `&pid` instead of `pid:&pid`.

**Error messages.** `solve` reports structural problems: a pattern field that doesn't exist in the data (and lists available fields), or a string decomposition pattern like `&a.&b` applied to a non-string value. If a query returns no results without an error, the pattern is valid but nothing matched — check field names and value types.

**Nested patterns work in both syntaxes.** Record: `{config: {port: &port}}`. List: `[config [&port]]`. Can mix them: `[{config: {port: &port}} &name]`.

## Reference

### Patterns

| Form | Meaning | Example |
|---|---|---|
| `&var` | Extract same-named field | `&name` |
| `key:value` | Field must equal literal | `type:file` |
| `key:&var` | Extract field into variable | `pid:&p` |
| `key:&a.&b` | Decompose string field | `name:&stem.&ext` |
| `{k: {k: v}}` | Nested record match | `{config: {port: &port}}` |
| `[k [k v]]` | Nested record match in list format | `[config [&port]]` |

Record syntax `{k: v}` and list syntax `[...]` are interchangeable. Colons and commas in list syntax are optional.

### Prefixes

| Prefix | Meaning |
|---|---|
| `&` | Logic variable — bound by `solve` during search |
| `$` | Nushell variable — evaluated before `solve` sees it |
| `@` | Fact reference — resolved from `facts` storage |

### Commands

- **`solve <pattern>`** — match patterns against pipeline input or across multiple sources
- **`facts [name]`** — store, retrieve, and manage named data sets

## Roadmap
More Prolog.

- **Negation-as-failure** — "find processes with no open ports." Filtering by absence currently requires post-hoc workarounds.
- **Rules** — named, reusable query fragments. Define a relationship once, use it across queries. Moves `solve` from ad-hoc queries toward inference.
- **Type casting in patterns** — `&port:int` to coerce during matching, eliminating the manual `update` step for mixed-type joins.
- **Constraints** — `&port > 1024` directly in patterns. Currently you filter after `solve`; inline constraints let the engine prune during search.

## License

MIT
