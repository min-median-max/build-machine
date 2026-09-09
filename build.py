#!/usr/bin/env python3
"""Diagnose, provision and build native projects from this Mac."""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import datetime
import fcntl
import json
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

    def call(self, args, capture=False):
        args = [str(arg) for arg in args]
        display = shlex.join(args)
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
                    if not capture:
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
        info = json.loads(self.call(['prlctl','list','-i','--json',self.vm], capture=True))[0]
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
    for stage_steps in stages.values():
        for step in stage_steps:
            step.setdefault('stage', next((stage for stage, values in stages.items() if step in values), 'setup'))
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


def retain_run(report, stamp, config):
    """Keep inspectable run reports while bounding controller state."""
    runs = STATE / 'runs'
    current = runs / stamp
    current.mkdir(parents=True, exist_ok=True)
    write_report(report, current / 'report.json', None)
    manifest = {'runId': stamp, 'project': report.get('project'), 'status': report.get('status'),
                'executionMode': report.get('executionMode'), 'source': report.get('source'),
                'platforms': report.get('platforms'), 'results': report.get('results')}
    write_report(manifest, current / 'manifest.json', None)
    policy = config.get('retention', {}) if isinstance(config, dict) else {}
    max_runs = int(policy.get('maxRunsPerProject', 20))
    max_bytes = int(policy.get('maxBytes', 20 * 1024 * 1024 * 1024))
    max_age = datetime.timedelta(days=int(policy.get('days', 30)))
    now = datetime.datetime.now(datetime.timezone.utc)
    candidates = []
    for path in runs.iterdir():
        if not path.is_dir() or path.name == stamp:
            continue
        try:
            modified = datetime.datetime.fromtimestamp(path.stat().st_mtime, datetime.timezone.utc)
        except OSError:
            continue
        report_file = path / 'report.json'
        value = None
        if report_file.is_file():
            try:
                value = json.loads(report_file.read_text())
            except (OSError, ValueError):
                value = None
        if value and value.get('status') in ('running', 'incomplete'):
            continue
        size = sum(file.stat().st_size for file in path.rglob('*') if file.is_file())
        candidates.append((modified, path, size, value))
    candidates.sort(key=lambda item: item[0], reverse=True)
    kept = 0
    total = sum(item[2] for item in candidates)
    for modified, path, size, value in candidates:
        project = value.get('project') if isinstance(value, dict) else None
        same_project = [item for item in candidates if (item[3] or {}).get('project') == project]
        expired = now - modified > max_age
        over_runs = same_project.index((modified, path, size, value)) >= max_runs if (modified, path, size, value) in same_project else False
        over_bytes = total > max_bytes
        if expired or over_runs or over_bytes:
            for child in sorted(path.rglob('*'), reverse=True):
                if child.is_file() or child.is_symlink():
                    child.unlink(missing_ok=True)
                elif child.is_dir():
                    child.rmdir()
            path.rmdir()
            total -= size
        else:
            kept += 1
    return kept


def execute_operation(args, platforms, stamp, log):
    report_path = STATE / (stamp + '-result.json')
    report = {'runId': stamp, 'action': args.action, 'project': str(args.project.expanduser().resolve()) if hasattr(args, 'project') else None,
              'platforms': platforms, 'executionMode': getattr(args, 'execution', 'sequential'), 'status': 'running',
              'startedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'source': None, 'results': {}, 'log': str(log)}
    write_report(report, report_path, args.result_file)
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

        def execute_platform(platform):
            platform_log = STATE / 'logs' / (stamp + '-' + platform + '.log')
            platform_log.parent.mkdir(parents=True, exist_ok=True)
            print('PLATFORM: ' + platform, flush=True)
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
                    else:
                        machine.setup()
                        if args.action == 'build':
                            machine.build(args, snapshot)
                            result = {'success': True, 'status': 'passed'}
                        elif args.action == 'release':
                            raise RuntimeError('Windows installer rehearsal is not implemented yet. Use ci run for workflow artifact validation.')
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
            except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
                result = {'success': False, 'status': 'failed', 'error': str(error)}
                print('ERROR: ' + str(error), file=sys.stderr, flush=True)
            result['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            result.setdefault('attempts', 1)
            result.setdefault('log', str(platform_log))
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
        with log.open('a') as output:
            output.write('ERROR: ' + str(error) + '\n')
        print('ERROR: ' + str(error), file=sys.stderr, flush=True)
    success = not report.get('error') and len(report['results']) == len(platforms) and all(result.get('success') for result in report['results'].values())
    limited = success and any(result.get('status') == 'passed_with_limits' for result in report['results'].values())
    report['status'] = 'passed_with_limits' if limited else ('success' if success else 'failure')
    report['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    write_report(report, report_path, args.result_file)
    try:
        retain_run(report, stamp, config)
    except (OSError, ValueError):
        # A retention failure must remain visible in the operation log but cannot
        # turn an already completed build into an invented build failure.
        with log.open('a') as output:
            output.write('WARNING: could not apply run retention policy.\n')
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
