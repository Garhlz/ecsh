# ecscript Smoke Scripts

These scripts are meant for manual smoke testing of the current ecscript boundary.

Run them with either binary:

```bash
cargo run --bin ecscript -- examples/ecscript/loop_and_accumulate.ecs
cargo run --bin ecsh -- examples/ecscript/loop_and_accumulate.ecs
```

Successful scripts:

- `loop_and_accumulate.ecs`: `for` range syntax, `while`, arrays, compound assignment
- `closures_and_state.ecs`: closures, captured state, arrays, function values
- `objects_and_collections.ecs`: nested objects, field/index mutation, `keys` / `values`
- `env_and_json.ecs`: `env()`, `range()`, `insert` / `remove`, `to_json`

Expected-failure scripts:

- `boundary_shell_in_block.ecs`: shell commands are still not allowed inside ecscript blocks
- `boundary_range_value.ecs`: bare `1..10` is no longer a normal value expression; use `range(1, 10)`
