# Security

Please do not open a public issue for a security problem. Report it privately through GitHub's "Report a
vulnerability" on this repository (Security tab), with steps to reproduce.

Things worth knowing when you assess a report:

- The native app runs an HTTP server on `127.0.0.1:3124`; the web app serves on `localhost:3000`. Neither is meant to be
  exposed beyond your machine.
- Generated designs are HTML that runs in a web view or an iframe. Treat designs from other people like web pages.
- Credentials (provider keys, a Codex login, a pod login) are read from your environment or your own files and are
  never written into the repository or into `state.json`.
