# Security Policy

## Supported versions

sociogram is early software (0.x). Only the latest release and the `main` branch receive fixes.

## Reporting a vulnerability

Please do not open a public issue for security problems. Use GitHub's **Report a vulnerability** button on the repository's Security tab (private vulnerability reporting). Include what you found, how to reproduce it, and which version or commit you tested.

Areas that matter most for this project:

- Memory safety and validation of data passed from JavaScript into the WebAssembly engine (`setData`, buffer sizes, indices).
- Anything that lets untrusted graph data crash the page, exhaust GPU or CPU memory, or hang the renderer.
- Supply chain: dependencies and the build and release pipeline.

You can expect an acknowledgement within a few days. Please give us reasonable time to fix the problem before disclosing it publicly.
