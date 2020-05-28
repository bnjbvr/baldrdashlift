baldrdashlift
===

A cli tool to make it more pleasant to work with Spidermonkey and Cranelift.

In the examples below, `builddir` refers to the root of a spidermonkey-only
build dir.

### import latest Cranelift in Spidermonkey, with commits

```
cargo run bump /path/to/mozilla-central
```

### use local Cranelift in Spidermonkey tree, with commits

```
cargo run local /path/to/mozilla-central /path/to/wasmtime
```

### run Spidermonkey test cases using Cranelift as the compiler

```
cargo run tests /path/to/mozilla-central /path/to/builddir
cargo run tests /path/to/mozilla-central /path/to/builddir wasm/multi-value # runs a subset of tests
```

### freebies: run make from a given build dir

```
cargo run build /path/to/builddir
```
