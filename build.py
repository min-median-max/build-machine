#!/usr/bin/env python3
"""Diagnose, provision and build native projects from this Mac."""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import datetime
import fcntl
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys

import workflow
from winbuild import ROOT, STATE, Machine, decode_output, git_root, make_source_archive, project_key, snapshot_project


class Runner:
    def __init__(self, config, platform, log):
        self.config = config
        self.platform = platform
        self.log = log
        self.vm = config['platforms'][platform].get('vm')
        self.share = Path('/media/psf') / config['share']

    def call(self, args, capture=False, quiet=False):
        """Run a guest command. `capture` returns the output; it does not hide
        it. Only `quiet` suppresses the live stream, for machine-readable
        introspection whose payload would bury the operation output."""
        args = [str(arg) for arg in args]
        display = shlex.join(args)
        if not quiet:
            print('> ' + display, flush=True)
        with self.log.open('ab') as log:
            log.write(('> ' + display + '\n').encode())
            log.flush()
            chunks = []
            with subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
                for line in iter(process.stdout.readline, b''):
                    log.write(line)
                    log.flush()
                    chunks.append(line)
                    if not quiet:
                        print(decode_output(line).rstrip(), flush=True)
                code = process.wait()
            log.write(('Exit code: %s\n' % code).encode())
        output = decode_output(b''.join(chunks))
        if code:
            raise RuntimeError('Command failed (%s): %s\n%s\nLog: %s' % (code, display, output[-2500:], self.log))
        return output

    def prepare(self):
        if self.platform != 'linux':
            return
        info = json.loads(self.call(['prlctl','list','-i','--json',self.vm], capture=True, quiet=True))[0]
        if info['State'] != 'running' or info.get('GuestTools', {}).get('state') != 'installed':
            raise RuntimeError('Start the Linux VM, sign in to its desktop and install Parallels Tools.')
        folders = info.get('Host Shared Folders', {})
        existing = folders.get(self.config['share'])
        if existing and Path(existing['path']).resolve() != ROOT:
            raise RuntimeError('The named build-machine share already belongs to another directory.')
        if not existing:
            self.call(['prlctl','set',self.vm,'--shf-host-add',self.config['share'],'--path',ROOT,'--mode','ro'])
        elif existing.get('mode') != 'ro' or not existing.get('enabled'):
            self.call(['prlctl','set',self.vm,'--shf-host-set',self.config['share'],'--mode','ro','--enable'])
        if not folders.get('enabled'):
            self.call(['prlctl','set',self.vm,'--shf-host','on'])

    def worker(self, action, request=None, run=False, root=False, capture=False):
        if self.platform == 'linux':
            args = ['prlctl','exec',self.vm]
            if not root:
                args.append('--current-user')
            args += ['/usr/bin/python3',self.share / 'native.py']
        else:
            args = [sys.executable, ROOT / 'native.py']
        args += [action]
        if request:
            if self.platform == 'linux':
                request = self.share / request.relative_to(ROOT)
            args += ['--request',request]
        if run:
            args += ['--run']
        return self.call(args, capture=capture)

    def execute(self, args, snapshot):
        self.prepare()
        if args.action in ('setup','build','release') and self.platform == 'linux':
            self.worker('setup-system', root=True)
        request = None
        if snapshot:
            data = dict(snapshot)
            if data.get('archive') and self.platform == 'linux':
                data['archive'] = str(self.share / Path(data['archive']).relative_to(ROOT))
            request = STATE / 'projects' / data['projectKey'] / (self.platform + '-request.json')
            request.parent.mkdir(parents=True, exist_ok=True)
            request.write_text(json.dumps(data, indent=2) + '\n')
        self.worker(args.action, request=request, run=getattr(args, 'run', False))

    def execute_ci(self, args, snapshot, config):
        self.prepare()
        data = dict(snapshot)
        data['archive'] = str(self.share / Path(data['archive']).relative_to(ROOT))
        data['target'] = config['platforms'][self.platform]['target']
        data['bundle'] = config['platforms'][self.platform].get('bundle')
        request = STATE / 'projects' / data['projectKey'] / (self.platform + '-ci-request.json')
        request.parent.mkdir(parents=True, exist_ok=True)
        request.write_text(json.dumps(data, indent=2) + '\n')
        output = self.worker('ci', request=request, capture=True)
        decoder = json.JSONDecoder()
        if 'CI_REPORT_BEGIN' not in output or 'CI_REPORT_END' not in output:
            raise RuntimeError('Native workflow did not return a structured CI report.')
        payload = output.split('CI_REPORT_BEGIN', 1)[1].split('CI_REPORT_END', 1)[0]
        report = decoder.raw_decode(payload[payload.find('{'):])[0]
        return report


def snapshot_ci(args, config):
    project = git_root(args.project.expanduser().resolve())
    if args.ref:
        if args.workflow:
            relative_workflow = Path(args.workflow)
        else:
            current = workflow.discover(project, None, args.event, None)
            relative_workflow = Path(current.path).resolve().relative_to(project.resolve())
        try:
            workflow_text = subprocess.check_output(['git', '-C', str(project), 'show', f'{args.ref}:{relative_workflow.as_posix()}'])
        except subprocess.CalledProcessError as error:
            raise ValueError(f'고정 ref에서 workflow를 읽지 못했어요: {relative_workflow}') from error
        workflow_root = STATE / 'workflow-snapshots' / project_key(project) / str(args.ref).replace('/', '_')
        workflow_root.mkdir(parents=True, exist_ok=True)
        workflow_path = workflow_root / relative_workflow.name
        workflow_path.write_bytes(workflow_text)
        selected = workflow.load(workflow_path, args.event, args.ref)
    else:
        selected = workflow.discover(project, args.workflow, args.event, args.ref)
    transfer = STATE / 'projects' / project_key(project)
    transfer.mkdir(parents=True, exist_ok=True)
    temporary = transfer / 'source.pending.zip'
    source = make_source_archive(project, temporary, args.ref)
    archive = transfer / (source['sourceHash'] + '.zip')
    if archive.exists():
        temporary.unlink()
    else:
        temporary.replace(archive)
    stages = workflow.stage_steps(selected)
    for stage, stage_steps in stages.items():
        for step in stage_steps:
            step.setdefault('stage', stage)
    return dict(source, projectKey=project_key(project), project=str(project), archive=str(archive),
                workflow=selected.serializable(), stages=stages,
                event=args.event, requestedRef=args.ref, workflowPath=selected.path)


def record_tool_status(config, action, platform, result, log):
    if action not in ('doctor', 'setup'):
        return
    path = STATE / 'tool-status.json'
    status = json.loads(path.read_text()) if path.exists() else {}
    if status.get('configuration') != config:
        status = {'configuration': config, 'results': {}}
    status['results'][platform] = dict(result, action=action, log=str(log))
    partial = path.with_suffix('.partial')
    partial.write_text(json.dumps(status, indent=2) + '\n')
    partial.replace(path)


def write_report(report, path, result_file):
    for destination in dict.fromkeys([path] + ([result_file] if result_file else [])):
        destination.parent.mkdir(parents=True, exist_ok=True)
        partial = destination.with_name(destination.name + '.partial')
        partial.write_text(json.dumps(report, indent=2) + '\n')
        partial.replace(destination)


LEGACY_SUFFIX = '-result.json'
LOG_SUFFIXES = ('-matrix.log', '-windows.log', '-linux.log', '-macos.log')


def log_index():
    """Map each run stamp to its log files. A stamp contains hyphens, so the
    suffix set is matched explicitly instead of splitting on the separator."""
    index = {}
    logs = STATE / 'logs'
    if not logs.is_dir():
        return index
    for path in logs.iterdir():
        if path.is_symlink() or not path.is_file():
            continue
        for suffix in LOG_SUFFIXES:
            if path.name.endswith(suffix):
                index.setdefault(path.name[:-len(suffix)], []).append(path)
                break
    return index


def file_size(path):
    try:
        return path.stat().st_size if path.is_file() and not path.is_symlink() else 0
    except OSError:
        return 0


def tree_size(path):
    return sum(file_size(item) for item in path.rglob('*'))


def remove_tree(path):
    for child in sorted(path.rglob('*'), key=lambda item: len(item.parts), reverse=True):
        try:
            if child.is_dir() and not child.is_symlink():
                child.rmdir()
            else:
                child.unlink(missing_ok=True)
        except OSError:
            return False
    try:
        path.rmdir()
    except OSError:
        return False
    return True


def collect_runs(runs, stamp, logs):
    """Describe every retained run except the current one.

    A run is its report plus the logs it points at, so both are sized and
    evicted together. The report file's own mtime is used: the directory's
    mtime moves on every atomic replace and would misreport age.
    """
    entries = []
    if not runs.is_dir():
        return entries
    for path in runs.iterdir():
        if path.name == stamp or path.name.startswith('.') or path.is_symlink() or not path.is_dir():
            continue
        report_file = path / 'report.json'
        try:
            value = json.loads(report_file.read_text())
        except (OSError, ValueError):
            value = None
        if not isinstance(value, dict):
            value = None
        try:
            modified = (report_file if report_file.is_file() else path).stat().st_mtime
        except OSError:
            continue
        source = (value or {}).get('source')
        run_logs = logs.get(path.name, [])
        entries.append({'name': path.name, 'path': path, 'modified': modified, 'logs': run_logs,
                        'size': tree_size(path) + sum(file_size(file) for file in run_logs),
                        'status': (value or {}).get('status'),
                        'project': (value or {}).get('project') or (source or {}).get('project')})
    return entries


def prune_orphan_logs(horizon, live):
    """Remove expired logs that no retained run points at.

    `winbuild.py` writes its own logs and keeps no run report, and a run pruned
    by an earlier policy could leave its logs behind. Neither is reachable from
    a report, so only age bounds them.
    """
    logs = STATE / 'logs'
    if not logs.is_dir():
        return
    for path in logs.iterdir():
        try:
            if path.is_symlink() or not path.is_file() or path.suffix != '.log':
                continue
            if any(path.name.startswith(stamp) for stamp in live):
                continue
            if datetime.datetime.fromtimestamp(path.stat().st_mtime, datetime.timezone.utc) < horizon:
                path.unlink(missing_ok=True)
        except OSError:
            continue


def prune_gui_results(horizon, keep=()):
    """The `--result-file` copies are transient. The current run's copy is never
    removed: the caller reads it after this process exits."""
    directory = STATE / 'gui'
    if not directory.is_dir():
        return
    protected = set()
    for path in keep:
        if not path:
            continue
        try:
            protected.add(Path(path).resolve())
        except OSError:
            continue
    for path in directory.iterdir():
        try:
            if path.is_symlink() or not path.is_file() or path.resolve() in protected:
                continue
            if datetime.datetime.fromtimestamp(path.stat().st_mtime, datetime.timezone.utc) < horizon:
                path.unlink(missing_ok=True)
        except OSError:
            continue


def retain_run(report, stamp, config, keep=()):
    """Bound controller state without removing the current run.

    Age, per-project count and total bytes are applied independently. Eviction
    is oldest first: the newest inspectable run must survive a full disk.
    """
    policy = config.get('retention', {}) if isinstance(config, dict) else {}
    max_runs = max(int(policy.get('maxRunsPerProject', 20)), 1)
    max_bytes = max(int(policy.get('maxBytes', 20 * 1024 * 1024 * 1024)), 0)
    horizon = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(days=int(policy.get('days', 30)))
    runs = STATE / 'runs'
    logs = log_index()
    entries = collect_runs(runs, stamp, logs)
    entries.sort(key=lambda entry: (entry['modified'], entry['name']))
    doomed = set()
    # Age. An abandoned `running` report is reclaimed here and nowhere else:
    # main() holds the exclusive lock, so no other run is still alive.
    for entry in entries:
        if entry['modified'] < horizon.timestamp():
            doomed.add(entry['name'])
    # Per project keep the newest runs; the current run occupies one slot.
    kept = {report.get('project'): 1}
    for entry in reversed(entries):
        if entry['name'] in doomed or entry['status'] in ('running', 'incomplete'):
            continue
        count = kept.get(entry['project'], 0)
        if count >= max_runs:
            doomed.add(entry['name'])
        else:
            kept[entry['project']] = count + 1
    # Total bytes, evicting the oldest first until the cap is satisfied.
    total = tree_size(runs / stamp) + sum(file_size(file) for file in logs.get(stamp, []))
    total += sum(entry['size'] for entry in entries if entry['name'] not in doomed)
    for entry in entries:
        if total <= max_bytes:
            break
        if entry['name'] in doomed or entry['status'] in ('running', 'incomplete'):
            continue
        doomed.add(entry['name'])
        total -= entry['size']
    survivors = 0
    live = {stamp}
    for entry in entries:
        if entry['name'] not in doomed:
            survivors += 1
            live.add(entry['name'])
        elif remove_tree(entry['path']):
            for file in entry['logs']:
                file.unlink(missing_ok=True)
        else:
            survivors += 1
            live.add(entry['name'])
    prune_orphan_logs(horizon, live)
    prune_gui_results(horizon, keep)
    return survivors


def migrate_legacy_runs():
    """Move pre-consolidation `.state/<stamp>-result.json` reports under `.state/runs/`.

    The new file is durable before the legacy copy is unlinked, so an
    interrupted migration never loses a report.
    """
    runs = STATE / 'runs'
    moved = 0
    for path in sorted(STATE.glob('*' + LEGACY_SUFFIX)):
        if path.is_symlink() or not path.is_file():
            continue
        stamp = path.name[:-len(LEGACY_SUFFIX)]
        if not stamp or stamp in ('.', '..') or os.sep in stamp or (os.altsep and os.altsep in stamp):
            continue
        target = runs / stamp / 'report.json'
        if target.parent.parent != runs:
            continue
        try:
            if target.is_file() and target.stat().st_size:
                # An earlier migration already wrote the canonical report; the
                # legacy copy is redundant and must not replace a newer file.
                path.unlink(missing_ok=True)
                continue
            data = path.read_bytes()
            stat = path.stat()
            target.parent.mkdir(parents=True, exist_ok=True)
            partial = target.with_name(target.name + '.partial')
            partial.write_bytes(data)
            os.utime(partial, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            partial.replace(target)
            os.utime(target, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            if target.stat().st_size == len(data):
                path.unlink(missing_ok=True)
                moved += 1
        except OSError:
            continue
    return moved


def note(log, text):
    """Append one line to the operation log. This file is what the desktop app
    opens for a run, so it must describe the run even when nothing fails."""
    try:
        log.parent.mkdir(parents=True, exist_ok=True)
        with log.open('a') as output:
            output.write('%s %s\n' % (datetime.datetime.now(datetime.timezone.utc).isoformat(), text))
    except OSError:
        pass


def execute_operation(args, platforms, stamp, log):
    # write_report creates the parent, so the run directory and its first report
    # appear one atomic rename apart rather than leaving an empty directory.
    report_path = STATE / 'runs' / stamp / 'report.json'
    report = {'runId': stamp, 'action': args.action, 'project': str(args.project.expanduser().resolve()) if hasattr(args, 'project') else None,
              'platforms': platforms, 'executionMode': getattr(args, 'execution', 'sequential'), 'status': 'running',
              'startedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'source': None, 'results': {}, 'log': str(log)}
    write_report(report, report_path, args.result_file)
    note(log, 'START run=%s action=%s platforms=%s execution=%s' % (stamp, args.action, ','.join(platforms), report['executionMode']))
    if report['project']:
        note(log, 'PROJECT ' + report['project'])
    note(log, 'REPORT ' + str(report_path))
    config = {}
    try:
        config = json.loads((ROOT / 'machine.json').read_text())
        snapshot = snapshot_ci(args, config) if args.action == 'ci' else (snapshot_project(args) if args.action in ('build', 'release') else None)
        if args.action == 'run':
            snapshot = {'projectKey': project_key(args.project.expanduser().resolve())}
        report['source'] = snapshot
        if snapshot and snapshot.get('project'):
            report['project'] = snapshot['project']
        write_report(report, report_path, args.result_file)
        if snapshot and snapshot.get('sourceHash'):
            note(log, 'SOURCE revision=%s dirty=%s hash=%s' % (snapshot.get('revision'), snapshot.get('dirty'), snapshot['sourceHash']))

        def execute_platform(platform):
            platform_log = STATE / 'logs' / (stamp + '-' + platform + '.log')
            platform_log.parent.mkdir(parents=True, exist_ok=True)
            print('PLATFORM: ' + platform, flush=True)
            note(log, 'PLATFORM %s started; command output goes to %s' % (platform, platform_log))
            try:
                if platform == 'windows':
                    machine = Machine(config)
                    machine.log_path = platform_log
                    machine.prepare()
                    if args.action == 'doctor':
                        machine.script('Doctor.ps1')
                        result = {'success': True, 'status': 'passed'}
                    elif args.action == 'run':
                        machine.run(args.project)
                        result = {'success': True, 'status': 'passed'}
                    elif args.action == 'ci':
                        result = machine.ci(args, snapshot, config)
                    elif args.action == 'release':
                        raise RuntimeError('Windows installer rehearsal is not implemented yet. Use ci run for workflow artifact validation.')
                    elif args.action in ('setup', 'build'):
                        # Both provision first; setup stops there.
                        machine.setup()
                        if args.action == 'build':
                            machine.build(args, snapshot)
                        result = {'success': True, 'status': 'passed'}
                    else:
                        raise RuntimeError('Unsupported Windows action: ' + str(args.action))
                else:
                    runner = Runner(config, platform, platform_log)
                    if args.action == 'ci':
                        result = runner.execute_ci(args, snapshot, config)
                    else:
                        runner.execute(args, snapshot)
                        result = {'success': True, 'status': 'passed'}
                if not isinstance(result, dict):
                    result = {'success': True, 'status': 'passed'}
                result.setdefault('success', result.get('status') in ('passed', 'passed_with_limits', 'success'))
            # A defect in one platform's code path must be recorded as that
            # platform's failure, not abort the whole matrix without a report.
            # The type name is kept so an unexpected defect stays identifiable.
            # KeyboardInterrupt and SystemExit are not Exception, so an
            # interrupted run still stops immediately.
            except Exception as error:
                reason = str(error) if isinstance(error, (OSError, ValueError, RuntimeError, subprocess.CalledProcessError)) else '%s: %s' % (type(error).__name__, error)
                result = {'success': False, 'status': 'failed', 'error': reason}
                print('ERROR: ' + reason, file=sys.stderr, flush=True)
            result['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            result.setdefault('attempts', 1)
            result.setdefault('log', str(platform_log))
            note(log, 'PLATFORM %s %s%s' % (platform, result.get('status', 'unknown'),
                                            '' if result.get('success') else ' error=' + str(result.get('error', ''))))
            return platform, result

        if getattr(args, 'execution', 'sequential') == 'parallel' and len(platforms) > 1:
            with ThreadPoolExecutor(max_workers=len(platforms), thread_name_prefix='build-machine') as pool:
                futures = {pool.submit(execute_platform, platform): platform for platform in platforms}
                for future in as_completed(futures):
                    platform, result = future.result()
                    report['results'][platform] = result
                    record_tool_status(config, args.action, platform, result, STATE / 'logs' / (stamp + '-' + platform + '.log'))
                    write_report(report, report_path, args.result_file)
        else:
            for platform in platforms:
                platform, result = execute_platform(platform)
                report['results'][platform] = result
                record_tool_status(config, args.action, platform, result, STATE / 'logs' / (stamp + '-' + platform + '.log'))
                write_report(report, report_path, args.result_file)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        report['error'] = str(error)
        note(log, 'ERROR: ' + str(error))
        print('ERROR: ' + str(error), file=sys.stderr, flush=True)
    success = not report.get('error') and len(report['results']) == len(platforms) and all(result.get('success') for result in report['results'].values())
    limited = success and any(result.get('status') == 'passed_with_limits' for result in report['results'].values())
    report['status'] = 'passed_with_limits' if limited else ('success' if success else 'failure')
    report['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    write_report(report, report_path, args.result_file)
    note(log, 'FINISH %s' % report['status'])
    try:
        retain_run(report, stamp, config, keep=(getattr(args, 'result_file', None),))
    except (OSError, TypeError, ValueError):
        # A retention failure must remain visible in the operation log but cannot
        # turn an already completed build into an invented build failure.
        note(log, 'WARNING: could not apply run retention policy.')
    print(json.dumps(report, indent=2), flush=True)
    return 0 if success else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='action', required=True)
    for action in ('doctor','setup','build','run','release'):
        command = commands.add_parser(action)
        command.add_argument('--os', nargs='+', choices=['windows','linux','macos','all'], default=['all'])
        command.add_argument('--result-file', type=Path, help='Write the structured operation result to this file')
        command.add_argument('--execution', choices=['sequential', 'parallel'], default='sequential', help='Run selected environments sequentially or in parallel')
        if action in ('build','run','release'):
            command.add_argument('project', type=Path)
        if action in ('build','release'):
            command.add_argument('--framework', choices=['auto','tauri','wails2','custom'], default='auto')
            command.add_argument('--command', help='Explicit command in the selected platform shell')
            command.add_argument('--artifact', help='Executable path relative to the source snapshot')
            command.add_argument('--run', action='store_true')
    ci = commands.add_parser('ci', help='Validate or replay a repository GitHub Actions workflow')
    ci_commands = ci.add_subparsers(dest='ci_action', required=True)
    validate = ci_commands.add_parser('validate', help='Validate a workflow contract without running it')
    run = ci_commands.add_parser('run', help='Run supported workflow steps in the selected environments')
    for command in (validate, run):
        command.add_argument('project', type=Path)
        command.add_argument('--workflow', help='Workflow path relative to the repository root')
        command.add_argument('--event', default='workflow_dispatch', choices=['workflow_dispatch', 'push', 'pull_request'])
        command.add_argument('--ref', help='Commit, branch or tag to replay; omitted means current working tree')
        command.add_argument('--os', nargs='+', choices=['windows','linux','macos','all'], default=['all'])
        command.add_argument('--execution', choices=['sequential', 'parallel'], default='sequential')
        command.add_argument('--result-file', type=Path, help='Write the structured operation result to this file')
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('Run this controller on macOS; native.py and windows/*.ps1 are the native workers.')
    if args.action == 'ci' and args.ci_action == 'validate':
        try:
            config = json.loads((ROOT / 'machine.json').read_text())
            snapshot = snapshot_ci(args, config)
            print(json.dumps({'status': 'valid', 'project': snapshot['project'], 'workflow': snapshot['workflowPath'],
                              'event': snapshot['event'], 'ref': snapshot.get('requestedRef'), 'revision': snapshot['revision'],
                              'dirty': snapshot['dirty'], 'stages': {key: len(value) for key, value in snapshot['stages'].items()}}, indent=2, ensure_ascii=False))
            return 0
        except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
            print('ERROR:', error, file=sys.stderr)
            return 1
    if 'all' in args.os and len(args.os) != 1:
        parser.error('Use --os all alone, or list the individual platforms.')
    platforms = ['windows','linux','macos'] if args.os == ['all'] else list(dict.fromkeys(args.os))
    STATE.mkdir(exist_ok=True)
    try:
        with (STATE / 'machine.lock').open('w') as lock:
            try:
                fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise RuntimeError('Another build-machine command is running. Wait for its result.')
            # Reports written before the run directories existed stay visible.
            # The check is one directory read and turns itself off afterwards.
            if next(STATE.glob('*' + LEGACY_SUFFIX), None) is not None:
                try:
                    moved = migrate_legacy_runs()
                    print('Moved %s earlier run report(s) under .state/runs.' % moved, flush=True)
                except OSError as error:
                    print('WARNING: could not migrate earlier run reports:', error, file=sys.stderr, flush=True)
            stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
            log = STATE / 'logs' / (stamp + '-matrix.log')
            log.parent.mkdir(parents=True, exist_ok=True)
            print('Host log:', log, flush=True)
            return execute_operation(args, platforms, stamp, log)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print('ERROR:', error, file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
