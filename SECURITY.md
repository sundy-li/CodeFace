# Security policy

## Supported versions

Security fixes are provided for the latest released version of CodeFace.

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability. Use GitHub's **Security** tab and select **Report a vulnerability** to send a private report to the maintainers.

Include the affected version, operating system, reproduction steps, impact, and any proof-of-concept material that is safe to share. You can expect an initial acknowledgement within seven days. We will coordinate disclosure after confirming the issue and preparing a fix.

## Security boundary

CodeFace must keep CDP endpoints on loopback, validate theme input, preserve official Codex files and credentials, and make restoration recoverable. See [`docs/security.md`](docs/security.md) for the detailed model.
