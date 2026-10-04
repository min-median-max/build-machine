# Checklist

The canonical work list of this repository. States: `[ ]` waiting, `[~]` active, `[o]` complete, `[!]` temporarily bypassed with a cause and a retry condition.

- [~] B1 — P0: Check out another repository in a workflow step. Found on 2026-10-04 while rehearsing the soksak release workflows: `actions/checkout` with `repository`, `ref` and `path` failed validation with `with 입력 'path'은 아직 지원하지 않아요`, and every soksak component release checks out `soksak-app/core` to build its `sok`. `machine.json` `repositories` maps a GitHub repository to its local clone on this Mac; the controller bundles that clone's history beside the project's, and the worker fetches the requested `ref` into `path` under the workspace. A repository that the map does not name fails validation.
