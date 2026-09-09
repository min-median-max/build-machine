import json
import subprocess
import tempfile
import unittest
from pathlib import Path

import workflow
from winbuild import make_source_archive


class WorkflowTests(unittest.TestCase):
    def workflow_file(self, text):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        (root / '.github/workflows').mkdir(parents=True)
        path = root / '.github/workflows/build.yml'
        path.write_text(text)
        return root, path

    def test_supported_steps_and_explicit_skip_markers_are_read(self):
        root, path = self.workflow_file('''# build-machine: skip test reason=no test command in this release workflow
# build-machine: skip smoke reason=smoke is verified by the desktop fixture
name: release
on:
  workflow_dispatch:
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - run: pnpm install --frozen-lockfile
      - uses: tauri-apps/tauri-action@v0
        with:
          args: --target universal-apple-darwin
''')
        parsed = workflow.load(path)
        self.assertEqual(parsed.name, 'release')
        self.assertEqual(parsed.skips['smoke'], 'smoke is verified by the desktop fixture')
        self.assertEqual(len(parsed.jobs[0]['steps']), 4)
        self.assertEqual(workflow.stage_steps(parsed)['build'][0]['adapter'], 'tauri-build')

    def test_unsupported_action_and_missing_gate_fail_closed(self):
        root, path = self.workflow_file('''name: bad
on: workflow_dispatch
jobs:
  build:
    runs-on: ubuntu
    steps:
      - uses: evil/action@v1
''')
        with self.assertRaisesRegex(workflow.WorkflowError, '어댑터'):
            workflow.load(path)

    def test_missing_test_or_smoke_requires_a_reasoned_comment(self):
        root, path = self.workflow_file('''name: bad
on: workflow_dispatch
jobs:
  build:
    runs-on: ubuntu
    steps:
      - name: build
        run: echo build
''')
        with self.assertRaisesRegex(workflow.WorkflowError, 'test 단계'):
            workflow.load(path)

    def test_ref_archive_is_clean_and_does_not_include_worktree_edits(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name) / 'repo'
        root.mkdir()
        subprocess.run(['git', 'init', '-q', str(root)], check=True)
        subprocess.run(['git', '-C', str(root), 'config', 'user.email', 'test@example.invalid'], check=True)
        subprocess.run(['git', '-C', str(root), 'config', 'user.name', 'test'], check=True)
        (root / 'value.txt').write_text('committed')
        subprocess.run(['git', '-C', str(root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(root), 'commit', '-qm', 'initial'], check=True)
        (root / 'value.txt').write_text('dirty')
        archive = Path(directory.name) / 'source.zip'
        result = make_source_archive(root, archive, 'HEAD')
        self.assertFalse(result['dirty'])
        self.assertEqual(result['sourceMode'], 'ref')
        import zipfile
        with zipfile.ZipFile(archive) as source:
            self.assertEqual(source.read('value.txt'), b'committed')


if __name__ == '__main__':
    unittest.main()
