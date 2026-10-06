# Contributing

## Local checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

## Safety rules

- Never construct privileged shell command strings from user input.
- Never enable `force` or `remove_hiberfile` by default.
- Keep privileged operations minimal and explicit.
- Mount points must stay inside `/Volumes`.
- Add tests for new validation logic.
