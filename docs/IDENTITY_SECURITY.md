# FileFlow identity and security

## Scope

FileFlow currently implements **device-local identity**. A user creates an account on the computer, authenticates with an e-mail address and password, receives an opaque desktop-session token, and keeps profile/onboarding data in the local SQLite database.

This is deliberately different from pretending that a remote/cloud account already exists. A future synchronized account will require a server-side identity provider, transport security, recovery flows, revocation and device management. The Angular UI and Tauri commands are separated so that a remote provider can be introduced without exposing credentials to the file-processing core.

## Password handling

- Passwords are never stored in clear text.
- The current local KDF is PBKDF2-HMAC-SHA256 with 600,000 iterations and a unique 128-bit salt per account.
- Password derivation and verification run on a blocking worker instead of the async UI/runtime path.
- Unknown-account login attempts perform comparable KDF work to reduce account-discovery timing differences.
- Password changes create a fresh salt/hash and rotate the active session.
- Passwords are limited to 12–128 Unicode characters in the local profile UI.

The KDF implementation is intentionally isolated in `fileflow-storage::auth`; it can be replaced by a dedicated Argon2id/PBKDF2 library without changing account storage or the frontend contract.

## Session model

- Session tokens are opaque random identifiers generated in the native Tauri process.
- The active token expires after 12 hours.
- The token is held in Angular memory only for the running application session; it is not persisted to `localStorage`.
- Tauri commands that access profile/onboarding/avatar data require a valid session token.
- Filesystem-sensitive workspace, execution, analysis, history, recipe and favourite commands also require an active native session.
- Expiration or logout cancels active jobs and clears the current-session output registry.
- Login failures are throttled after repeated failures with increasing delays.

Native FileFlow does not manufacture HTTP cookies for a desktop-only IPC boundary. Cookies become relevant when a remote HTTPS identity service is introduced.

## Profile and avatar safety

- Display names and personal names are normalized and bounded.
- E-mail addresses are normalized and checked for uniqueness in the local database.
- Avatar upload is limited to 4 MiB.
- File signatures are checked before accepting JPEG/PNG/WebP avatars.
- Avatar files are copied into the application data directory.
- Reading an avatar canonicalizes the stored path and verifies that it remains inside the authenticated account's profile directory.

## Storage directory safety

The first-run FileFlow directory is normalized before setup can complete:

- empty paths are rejected;
- the directory is created if needed;
- symbolic-link destinations are rejected for the configured root;
- the final directory is canonicalized;
- guided-mode results use this directory with non-destructive conflict handling.

## Data boundaries

SQLite stores account/profile metadata and onboarding. Favourites, recipes, operation history and post-login preferences are scoped to the active local profile; the first local account claims legacy pre-account data. Non-secret pre-login appearance preferences remain available as a fallback for the welcome screen. FileFlow does not put source document contents in history. Conversion engines receive filesystem paths only when the user starts an operation.

## Future connected-account architecture

A true multi-device/cloud account should add a separate identity service with:

1. HTTPS-only API and server-side password/OIDC/WebAuthn policy;
2. short-lived access tokens and refresh-token rotation;
3. OS secure storage (Keychain/Secret Service) for refresh credentials;
4. e-mail verification and account recovery;
5. device/session listing and revocation;
6. server-side rate limiting and audit events;
7. optional encrypted preference/recipe synchronization;
8. no automatic upload of user documents unless an explicit future feature requires it.

Until that backend exists, FileFlow should describe the current account as local to the device rather than as a cloud account.
