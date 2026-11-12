---
status: proposed
date: 2026-11-09
---
# Keep vault keys on enrolled devices

## Context

The requested storage design uses encryption without server-held decryption keys.
The requested model uses one enrollment per device and excludes a full local index copy.

## Decision

Create and wrap keys in the browser through [Web Cryptography](https://www.w3.org/TR/2017/REC-WebCryptoAPI-20170126/).
Use the [trusted running server design](search-encrypted-storage-on-a-trusted-running-server.md) for content encryption and search.
Generate independent random data and recovery keys.
Wrap the data key with the recovery key, following [envelope encryption](https://cheatsheetseries.owasp.org/cheatsheets/Cryptographic_Storage_Cheat_Sheet.html#encrypting-stored-keys).
Remember a non-extractable device key through IndexedDB.
Keep browser-code delivery and the enrolled operating system inside the trust boundary.
Bind wrapped-key ciphertext to its vault, object, purpose, and position.

## Alternatives

Server-held keys expose content to the running service.
Per-launch secret entry conflicts with the requested device experience.

## Consequences

New devices need a recovery secret.
Lost secrets and lost devices can make content unreadable.
Reconsider key storage if the trusted client-delivery boundary changes.

## Confirmation

Check wrong keys, tampering, object substitution, recovery, remembered access, forgetting, and key replacement.
Keep the record proposed until integration and migration pass.
