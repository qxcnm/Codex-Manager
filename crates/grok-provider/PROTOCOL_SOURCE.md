# Grok Web protocol source

This crate's typed protocol boundary was independently implemented with the
MIT-licensed `chenyme/grok2api` project as a public protocol reference:

- Repository: <https://github.com/chenyme/grok2api>
- Reference commit: `c015c2367c99445e99a0c260a5f3daa40928e6f4`
- License copy: [`GROK2API_LICENSE`](./GROK2API_LICENSE)

The crate intentionally contains no credentials, login automation, HTTP
service, account pool, or admin UI from the reference project.

Quota tier inference and the external Statsig signer request envelope follow
the same fixed reference. Signing is deliberately not guessed or generated
locally.
