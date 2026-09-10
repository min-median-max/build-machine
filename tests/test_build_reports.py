import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build


class BuildReportTests(unittest.TestCase):
    def invoke(self, root, prepare, execute):
        (root / 'machine.json').write_text(json.dumps({'platforms': {'linux': {}, 'macos': {}}, 'share': 'test'}))
        argv = ['build.py', 'build', str(root), '--os', 'linux', 'macos', '--result-file', str(root / 'gui-result.json')]
        with patch.object(build, 'ROOT', root), patch.object(build, 'STATE', root / '.state'), \
             patch.object(build.sys, 'platform', 'darwin'), patch.object(sys, 'argv', argv), \
             patch.object(build, 'snapshot_project', prepare), patch.object(build.Runner, 'execute', execute), \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return build.main()

    def test_source_preparation_failure_replaces_the_incomplete_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)

            def prepare(args):
                reports = list((root / '.state' / 'runs').glob('*/report.json'))
                self.assertEqual(len(reports), 1)
                pending = json.loads(reports[0].read_text())
                self.assertEqual(pending['status'], 'running')
                self.assertEqual(pending['project'], str(root.resolve()))
                self.assertEqual(pending['platforms'], ['linux', 'macos'])
                self.assertEqual(pending['results'], {})
                raise ValueError('test source could not be prepared')

            self.assertEqual(self.invoke(root, prepare, lambda *args: self.fail('A worker must not start.')), 1)
            report = json.loads((root / 'gui-result.json').read_text())
            self.assertEqual(report['status'], 'failure')
            self.assertIn('test source could not be prepared', report['error'])
            self.assertTrue(report['finishedAt'])
            self.assertIn(report['error'], Path(report['log']).read_text())
            self.assertFalse(list((root / '.state').rglob('*.partial')))

    def test_completed_platforms_are_recorded_before_the_next_starts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)

            def execute(runner, args, source):
                pending = json.loads((root / 'gui-result.json').read_text())
                self.assertEqual(pending['status'], 'running')
                if runner.platform == 'macos':
                    self.assertTrue(pending['results']['linux']['success'])
                    raise RuntimeError('test compiler failed')

            self.assertEqual(self.invoke(root, lambda args: {'project': str(root)}, execute), 1)
            report = json.loads((root / 'gui-result.json').read_text())
            self.assertEqual(report['status'], 'failure')
            self.assertTrue(report['results']['linux']['success'])
            self.assertFalse(report['results']['macos']['success'])

    def test_parallel_execution_keeps_one_snapshot_and_uniform_platform_results(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            project = root / 'project'
            project.mkdir()
            subprocess = __import__('subprocess')
            subprocess.run(['git', 'init', '-q', str(project)], check=True)
            subprocess.run(['git', '-C', str(project), 'config', 'user.email', 'test@example.invalid'], check=True)
            subprocess.run(['git', '-C', str(project), 'config', 'user.name', 'test'], check=True)
            (project / '.github/workflows').mkdir(parents=True)
            (project / '.github/workflows/build.yml').write_text('''# build-machine: skip test reason=fixture\n# build-machine: skip smoke reason=fixture\nname: build\non: workflow_dispatch\njobs:\n  build:\n    runs-on: macos-14\n    steps:\n      - run: echo build\n''')
            subprocess.run(['git', '-C', str(project), 'add', '.'], check=True)
            subprocess.run(['git', '-C', str(project), 'commit', '-qm', 'initial'], check=True)
            machine = root / 'machine.json'
            machine.write_text(json.dumps({'platforms': {'linux': {}, 'macos': {}}, 'share': 'test'}))
            result_file = root / 'result.json'
            argv = ['build.py', 'ci', 'run', str(project), '--os', 'linux', 'macos', '--workflow', '.github/workflows/build.yml', '--execution', 'parallel', '--result-file', str(result_file)]
            calls = []
            def execute_ci(runner, args, snapshot, config):
                calls.append((runner.platform, snapshot['sourceHash'], args.execution))
                return {'success': True, 'status': 'passed_with_limits', 'platform': runner.platform, 'stages': {}, 'limits': ['fixture']}
            with patch.object(build, 'ROOT', root), patch.object(build, 'STATE', root / '.state'), \
                 patch.object(build.sys, 'platform', 'darwin'), patch.object(sys, 'argv', argv), \
                 patch.object(build.Runner, 'execute_ci', execute_ci), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(build.main(), 0)
            report = json.loads(result_file.read_text())
            self.assertEqual(report['executionMode'], 'parallel')
            self.assertEqual(report['status'], 'passed_with_limits')
            self.assertEqual(set(report['results']), {'linux', 'macos'})
            self.assertEqual({source for _, source, _ in calls}, {report['source']['sourceHash']})
            self.assertEqual({mode for _, _, mode in calls}, {'parallel'})


if __name__ == '__main__':
    unittest.main()
