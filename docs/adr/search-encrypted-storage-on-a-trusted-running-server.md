---
status: proposed
date: 2026-11-09
---
# Search encrypted storage on a trusted running server

## Context

The personal Tailscale service must conceal offline storage content without corpus-sized device replicas.
[Web Cryptography](https://www.w3.org/TR/2017/REC-WebCryptoAPI-20170126/#security-developers) can't protect keys from a malicious code distributor.

## Decision

Test native search over a fully encrypted SQLCipher catalog.
Encrypt bodies, recovery records, original files, metadata, and indexes.
An enrolled browser supplies an unlock key after each server restart.
Keep runtime keys off persistent server storage.
Trust the running server, operating system, TLS proxy, and browser-code distributor.
Require a device session for plaintext responses.
Require a dedicated HTTPS hostname because [cookies share ports](https://www.rfc-editor.org/rfc/rfc6265.html#section-8.5).
Use [SQLCipher's page encryption and authentication](https://www.zetetic.net/sqlcipher/design/) with memory-only temporary storage.

## Alternatives

[Demand-paged browser search](read-encrypted-index-pages-on-demand.md) exposes access patterns.
[Oblivious random access memory](https://pages.cs.wisc.edu/~yxy/pubs/PathORam.pdf) conceals storage accesses through extra transfers.

## Consequences

Storage copies and exposed listeners must require a key or session for content.
The running service can access plaintext and keys.
Reconsider the decision if protection from active server compromise becomes necessary.
Protecting live searches would also need independent client distribution.

## Confirmation

Check sessions, restart recovery, encrypted journals, corruption, and file ranges.
Measure native search with encrypted million-note fixtures before selecting cache limits.
