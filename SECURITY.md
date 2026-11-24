# Security policy

## Report a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/dvgyl/randoc/security/advisories/new).
Include the Randoc version, the operating system, the affected command or
API, and the steps necessary to reproduce the problem. Remove credentials
and private data from attachments. Keep the report private while the
maintainers assess it.

## Security boundaries

- Randoc treats document input as untrusted. Conversion limits bound nesting,
  table size, input size, and archive expansion. The server uses
  `Limits::restricted()` for stricter limits on network input.
- The `--sandbox` option restricts file and network access during conversion. Filters
  and external PDF engines can run outside that boundary. Use only trusted
  filters and engines. Use operating system isolation for a public service.
- Generated HTML can contain scripts, event attributes, and active URLs from
  the input. Sanitize the HTML before serving it to other users, or display
  it in an isolated origin. Serve a public conversion API with request limits
  and a policy for which conversion options clients can use.
- Randoc forbids Rust `unsafe` code except at the audited Lua C foreign
  function interface in `randoc-lua`. See ADRs 11 and 19 in the
  [architecture decisions](docs/ADR.md).
