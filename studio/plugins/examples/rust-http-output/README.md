# HTTP Output (Rust example)

A controller type in Rust with `src/vixen_plugin_sdk.rs` (a copy of `crates/studio-plugin-sdk/src/lib.rs`): it POSTs
the lights' colours as JSON to a URL, at most once per interval and only when they change, from a worker thread. That is
the shape a slow, rate-limited cloud API needs. Build it, then try it:

    ./build.sh
    vixen-studio-mcp plugin validate .
    vixen-studio-mcp plugin run . --seconds 3

It speaks plain http:// with the standard library; for https:// add an HTTP client crate and declare the host in
`permissions.network`.
