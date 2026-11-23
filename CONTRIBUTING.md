# Contributing to sociogram

Thanks for helping. This is a small project, so the rules are short.

## The one hard rule: original work only

sociogram is a clean-room implementation. **Do not copy or translate code, shaders, assets, text or API designs from other graph visualization projects or products** (including Cosmograph and cosmos.gl), and do not paste in decompiled or minified code. Many of them have licenses that are incompatible with ours.

- Ideas, public algorithms and published papers are fine (force-directed layout, Barnes-Hut and grid approximations, instanced rendering, GPU atomics). Write the code yourself.
- If you read another project's source closely, say so in the pull request and keep that part of the work to someone else, so the implementation stays independent.
- Say where a non-obvious algorithm comes from (paper, book, documentation) in a comment.

Pull requests that contain copied code will be closed.

## Setup

See "Build from source" in the [README](README.md). You need Rust (stable), the `wasm32-unknown-unknown` target, a matching `wasm-bindgen-cli` (the version is pinned in `crates/sociogram-web/Cargo.toml`), and Node.js.

## Before you open a pull request

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p sociogram-web --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace
(cd packages/sociogram && npm run build)
(cd web && npm test)        # if your change touches rendering, layout or the browser API
```

GPU tests skip themselves when no adapter is available, so please run them on a machine with a GPU for changes to `sociogram-gpu` or `sociogram-web`. Try more than one backend when you can (`WGPU_BACKEND=dx12`, `vulkan`, `gl`, `metal`).

## Code guidelines

- Write the test first and watch it fail for the right reason. Bug fixes need a test that fails without the fix.
- Validate input at the boundary (what JavaScript passes in) and return clear errors. Do not panic on bad input.
- Keep files under 500 lines. Split by responsibility.
- `sociogram-core` stays free of GPU code and dependencies.
- Instance structs that go to the GPU must have no padding between fields that the vertex attributes pack tightly; there are tests for the sizes.
- No `unsafe` without a comment explaining why it is sound.
- Measure before optimizing and put the numbers in the pull request (machine, GPU, browser, graph size). Performance claims without numbers will be asked for them.
- User-visible changes belong in the README's status section.

## Pull requests

- Keep them focused; one change per pull request.
- Describe what changed and why, how you tested it, and on which hardware and browser.
- Be open about what you did not verify.

## License of contributions

Unless you state otherwise, any contribution you submit is licensed under the same terms as the project: `MIT OR Apache-2.0`.

## Conduct

Be respectful. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
