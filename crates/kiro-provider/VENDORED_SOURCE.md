# Vendored source provenance

This crate is derived exclusively from [`hank9999/kiro.rs`](https://github.com/hank9999/kiro.rs).

- Pinned upstream commit: `b9e757e1f8c1d2f1bbfb1157e8f9002fd5c19c9c`
- Upstream license: MIT
- Local license copy: `KIRO_RS_LICENSE`

The standalone HTTP server, admin UI, API-key layer, and JSON-file runtime
storage were removed. Authentication, request conversion, signing, machine ID,
AWS EventStream decoding, thinking/tool/image support, credential failover, and
quota handling are integrated into CodexManager's service and encrypted SQLite
storage.
