# Making a project buildable on three operating systems

[한국어](ADOPTING.ko.md) · [what the machine supports](PROJECTS.md)

This is what to change in your own repository so this machine can build it on
Windows, Ubuntu and macOS, and rehearse its release before you tag one.

Nothing here is specific to this machine. Every step is something a project
that claims to ship on three operating systems should be able to answer anyway:
which commit was built, whether the tests ran, whether the application starts.
The machine only refuses to guess when the answer is missing.

Work through the checklist, then run `build-machine ci validate` — it tells you
exactly which item is not satisfied yet.

---

## 1. The repository is the unit

- **Register the Git repository root**, not a subdirectory. A monorepo selects
  its sub-application through a workflow step's `working-directory`.
- **Commit your lockfiles.** `pnpm-lock.yaml` or `package-lock.json`, and
  `src-tauri/Cargo.lock` for a Tauri project. A build here is `--frozen-lockfile`
  and `--locked`; a lockfile that drifts fails rather than resolving something
  new.
- **No symlinks out of the project, and no submodules.** The snapshot copies
  tracked files and non-ignored untracked files, including uncommitted edits.
  Anything reaching outside it is refused instead of being silently followed.
- **Keep machine-local secrets out of tracked files.** Ignored files are not
  copied, which is also why a build must not depend on one.

## 2. The build must run unattended

- No interactive prompts, no development server, no absolute paths belonging to
  one developer's machine.
- Use the framework's production build. For Tauri that is `tauri build`;
  the machine passes `--ci --no-sign --locked` and the target itself.
- Produce **one** identifiable executable per platform, or name it with
  `--artifact`. Two candidates in the output directory is an error, not a
  coin flip.

## 3. Match the declared tool versions

The machine installs exactly what [machine.json](machine.json) declares and puts
it first on PATH. It does not read your `.nvmrc` or `rust-toolchain.toml`.

Make your project work with the declared Node.js, pnpm, Rust and Go, or change
`machine.json` and say why. `build-machine doctor` reports what is actually
installed against what is declared.

## 4. Account for what each platform needs

| Platform | What the project has to survive |
| --- | --- |
| Windows | WebView2, MSVC, ARM64. A `.cmd` shim is not directly executable — go through `cmd.exe`. |
| Ubuntu | WebKitGTK and the system libraries in `machine.json`'s `packages`. |
| macOS | A universal build has to contain both architectures; the machine checks with `lipo` and fails if one is missing. |

Compiling for the right OS is your code's problem. The machine provisions its
declared prerequisites; it does not infer a native library from your source.

## 5. The workflow is the contract

`ci validate` and `ci run` read one `.github/workflows/*.yml` file and reproduce
the part of it this machine can actually perform. Anything else fails
validation rather than being quietly changed into something else.

**Supported actions** — `actions/checkout`, `pnpm/action-setup`,
`actions/setup-node`, `dtolnay/rust-toolchain`, `swatinem/rust-cache`,
`actions/cache`, `tauri-apps/tauri-action`, `actions/upload-artifact`,
`actions/upload-pages-artifact`, `softprops/action-gh-release`. Shell `run`
steps execute as written.

**Not supported** — containers, services, reusable workflows, `strategy.matrix`,
and any action without an adapter. These fail validation.

**Expressions** — a condition whose value cannot be known locally fails the run.
A condition that depends on a secret is treated as false and recorded as a
limit, so a signing step is skipped rather than half-attempted.

### The three gates

A workflow must **build**, and must either **test** and **smoke** or say why it
does not:

```yaml
# build-machine: skip smoke reason=the desktop launch is verified separately against a signed-in session
```

The reason is stored in the platform result. This is the point of the gate: a
missing check becomes a decision on the record instead of an omission nobody
noticed.

Prefer a real step over a skip. If your repository already has tests, run them —
that is usually the whole change.

### How a step is assigned to a stage

A step that uses a supported action is assigned by that adapter: checkout,
pnpm/node/Rust setup and caches are `setup`, the Tauri action is `build`,
artifact upload and release are `release`. Only shell `run` steps are read from
their own name and command, matching `smoke`/`launch`/`health`/`e2e`, then
`test`/`lint`/`check`/`verify`, then `build`/`package`/`compile`.

So name your shell steps for what they do. An action's own name never counts —
`actions/checkout` contains "check" and does not make a test stage.

### A workflow that hard-codes one platform can only be replayed there

`runs-on` says where a job was written to run, and an argument like
`--target universal-apple-darwin` says it again. Replaying such a workflow
elsewhere is refused before anything starts:

```
이 워크플로는 macos 에서 실행되도록 작성됐어요. linux에서는 재현할 수 없습니다.
```

`ci validate` reports which platforms a workflow can be replayed on, so you can
see this without running anything.

To rehearse all three, **do not name the target yourself** — the machine
supplies the one for the platform it is building on, from `machine.json`. Only
name it when you mean that one platform and no other.

### What a replay never establishes

Signing, notarization, artifact upload and release publication are replaced by
local adapters and recorded as limits. A successful replay is
`passed_with_limits`, never proof that a real release would publish.

---

## Checklist

```
[ ] The Git repository root is what you register
[ ] Lockfiles are committed
[ ] No external symlinks, no submodules
[ ] The build runs with no prompts and no developer-specific paths
[ ] One identifiable executable per platform, or --artifact names it
[ ] The project works with the versions machine.json declares
[ ] The workflow uses only supported actions, no matrix, no containers
[ ] A build stage exists
[ ] A test stage exists, or a skip comment says why not
[ ] A smoke stage exists, or a skip comment says why not
[ ] build-machine ci validate passes
```

---

## A worked example

AIRDATA is a Tauri 2 application whose release workflow built and published a
macOS app. It carried 38 Rust tests that the workflow never ran, and it had no
smoke step. Validation refused it:

```
ERROR: test 단계가 없어요. 워크플로에 '# build-machine: skip test reason=...' 주석을 추가해야 해요.
```

Two changes satisfied the contract. A test stage, because the tests already
existed and only needed running:

```yaml
      - name: Test the frontend types and the Rust core
        run: |
          pnpm exec tsc --noEmit
          cargo test --locked --manifest-path src-tauri/Cargo.toml
```

And a recorded reason for the stage a hosted runner genuinely cannot perform:

```yaml
# build-machine: skip smoke reason=the desktop launch is verified by `build-machine run` against a signed-in desktop session, which a hosted runner does not have
```

The replay then ran end to end:

```
전체: passed_with_limits
  setup    passed_with_limits   checkout · pnpm · node · rust · cache · install · sign(skipped)
  test     passed               38 Rust tests and the frontend typecheck
  build    passed               tauri-action → data_0.1.0_universal.dmg
  smoke    passed_with_limits   skip smoke=…
```

Replaying the same workflow on Ubuntu then found something the macOS-only
workflow never could: one of the 38 tests failed there. It connected a second
client and read `items[0]` without waiting for the first client's post to be
acknowledged, so it passed only while the machine happened to be fast enough.
The same file already had the pattern for waiting; the test simply had not used
it. With that fixed all 38 pass on both.

That is the whole point of a three-OS rehearsal, and it is also why replaying
this particular workflow on Ubuntu is now refused earlier: its `runs-on` and its
`--target universal-apple-darwin` both say macOS. A workflow meant for three
platforms must not name the target itself.

The tests now run before a tag is cut, which is a change to the project's own
release quality, not a formality this machine imposed.
