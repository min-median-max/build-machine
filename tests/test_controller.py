import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from unittest.mock import MagicMock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
import winbuild


class ControllerResultTests(unittest.TestCase):
    def test_windows_failure_preserves_the_actual_reason_for_the_gui(self):
        with tempfile.TemporaryDirectory() as temporary:
            machine = winbuild.Machine.__new__(winbuild.Machine)
            machine.log_path = Path(temporary) / 'windows.log'
            machine.prlctl = 'test-prlctl'
            machine.vm = 'test-vm'
            child = MagicMock()
            child.__enter__.return_value = child
            child.stdout = io.BytesIO(b'Checking tools\nERROR: node: expected v22; found v24\n')
            child.wait.return_value = 1
            with patch.object(winbuild.subprocess, 'Popen', return_value=child), contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaisesRegex(RuntimeError, 'node: expected v22; found v24'):
                    machine.guest('test diagnosis')
            self.assertIn('test diagnosis', machine.log_path.read_text())
            self.assertIn('Exit code: 1', machine.log_path.read_text())

    def test_windows_setup_records_a_completed_result(self):
        """setup provisions and stops; it must still record a platform result."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            root.joinpath('machine.json').write_text(json.dumps({'platforms': {'windows': {}}, 'share': 'test'}))
            result_file = root / 'gui-result.json'
            provisioned = []

            class FakeMachine:
                def __init__(self, config, vm=None):
                    self.log_path = None

                def prepare(self):
                    pass

                def setup(self):
                    provisioned.append(True)

            argv = ['build.py', 'setup', '--os', 'windows', '--result-file', str(result_file)]
            with patch.object(build, 'ROOT', root), patch.object(build, 'STATE', root / '.state'), \
                 patch.object(build.sys, 'platform', 'darwin'), patch.object(sys, 'argv', argv), \
                 patch.object(build, 'Machine', FakeMachine), \
                 contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(build.main(), 0)
            self.assertEqual(provisioned, [True])
            report = json.loads(result_file.read_text())
            self.assertEqual(report['status'], 'success')
            self.assertIs(report['results']['windows']['success'], True)

    def test_an_unexpected_defect_is_recorded_as_that_platform_failure(self):
        """A defect must not abort the matrix without any report."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            root.joinpath('machine.json').write_text(json.dumps({'platforms': {'linux': {}, 'macos': {}}, 'share': 'test'}))
            result_file = root / 'gui-result.json'

            def execute(runner, args, source):
                if runner.platform == 'linux':
                    raise AttributeError('test defect in the worker path')

            argv = ['build.py', 'build', str(root), '--os', 'linux', 'macos', '--result-file', str(result_file)]
            with patch.object(build, 'ROOT', root), patch.object(build, 'STATE', root / '.state'), \
                 patch.object(build.sys, 'platform', 'darwin'), patch.object(sys, 'argv', argv), \
                 patch.object(build, 'snapshot_project', lambda args: {'project': str(root)}), \
                 patch.object(build.Runner, 'execute', execute), \
                 contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(build.main(), 1)
            report = json.loads(result_file.read_text())
            self.assertEqual(report['status'], 'failure')
            self.assertIs(report['results']['linux']['success'], False)
            self.assertIn('AttributeError: test defect in the worker path', report['results']['linux']['error'])
            self.assertIs(report['results']['macos']['success'], True)

    def test_tool_results_persist_per_environment_and_change_with_configuration(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(build, 'STATE', Path(temporary)):
            config = {'node': {'version': '22.test'}}
            result = {'success': True, 'finishedAt': '2026-09-09T11:00:00+00:00'}
            build.record_tool_status(config, 'doctor', 'linux', result, 'linux.log')
            build.record_tool_status(config, 'setup', 'macos', result, 'macos.log')
            build.record_tool_status(config, 'build', 'linux', {'success': False}, 'build.log')
            stored = json.loads((Path(temporary) / 'tool-status.json').read_text())
            self.assertEqual(set(stored['results']), {'linux', 'macos'})
            self.assertTrue(stored['results']['linux']['success'])
            self.assertEqual(stored['results']['linux']['finishedAt'], result['finishedAt'])
            self.assertEqual(stored['results']['linux']['action'], 'doctor')
            build.record_tool_status({'node': {'version': '24.test'}}, 'setup', 'macos', result, 'new.log')
            stored = json.loads((Path(temporary) / 'tool-status.json').read_text())
            self.assertEqual(set(stored['results']), {'macos'})

    def test_failed_command_is_identifiable_in_the_persistent_log(self):
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / 'operation.log'
            runner = build.Runner({'platforms': {'macos': {}}, 'share': 'test'}, 'macos', log)
            argv = [sys.executable, '-c', 'import sys; print("guest output"); sys.exit(7)']
            with contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(RuntimeError, 'Command failed \\(7\\)'):
                runner.call(argv)
            text = log.read_text()
            self.assertIn('sys.exit(7)', text)
            self.assertIn('guest output', text)
            self.assertIn('Exit code: 7', text)

    def test_multiple_platforms_share_a_snapshot_and_failure_does_not_skip_the_next(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'machine.json').write_text('{}')
            result_file = root / 'gui' / 'result.json'
            snapshot = {'sourceHash': 'shared-test-source', 'projectKey': 'test'}
            calls = []

            def execute(runner, args, source):
                calls.append((runner.platform, source))
                if runner.platform == 'linux':
                    raise RuntimeError('missing test dependency')

            argv = ['build.py', 'build', str(root), '--os', 'linux', 'macos', '--result-file', str(result_file)]
            with patch.object(build, 'ROOT', root), patch.object(build, 'STATE', root / '.state'), \
                 patch.object(build.sys, 'platform', 'darwin'), patch.object(sys, 'argv', argv), \
                 patch.object(build, 'snapshot_project', return_value=snapshot) as capture, \
                 patch.object(build.Runner, 'execute', execute), \
                 contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                # The test runner only needs the fields read by Runner.__init__.
                (root / 'machine.json').write_text(json.dumps({'platforms': {'linux': {}, 'macos': {}}, 'share':'test'}))
                self.assertEqual(build.main(), 1)
            capture.assert_called_once()
            self.assertEqual([platform for platform, _ in calls], ['linux', 'macos'])
            self.assertTrue(all(source is snapshot for _, source in calls))
            result = json.loads(result_file.read_text())
            self.assertFalse(result['results']['linux']['success'])
            self.assertEqual(result['results']['linux']['error'], 'missing test dependency')
            self.assertTrue(result['results']['macos']['success'])
            self.assertFalse(result_file.with_name('result.json.partial').exists())


if __name__ == '__main__':
    unittest.main()
