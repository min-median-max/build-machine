#!/usr/bin/env python3
"""Native macOS/Linux worker. This same entry point can run in GitHub Actions."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import sys
import threading
import time
import zipfile

from unix_tools import Tools, command


ROOT = Path(__file__).resolve().parent


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = path.with_name(path.name + '.partial')
    partial.write_text(json.dumps(value, indent=2) + '\n')
    partial.replace(path)


def local_workspace():
    return Path.home() / '.local' / 'state' / 'build-machine'


def project_root(request):
    import re
    if not re.fullmatch(r'[A-Za-z0-9._-]+', request['projectKey']):
        raise ValueError('Invalid project key.')
    return local_workspace() / request['projectKey']


def running_pid(executable):
    if sys.platform == 'linux':
        for process in Path('/proc').iterdir():
            if not process.name.isdecimal():
                continue
            try:
                if (process / 'exe').resolve(strict=True) == executable.resolve():
                    return int(process.name)
            except (OSError, RuntimeError):
                continue
        return None
    output = subprocess.check_output(['ps', '-axww', '-o', 'pid=,comm='], text=True)
    for line in output.splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) == 2 and Path(parts[1]).resolve() == executable.resolve():
            return int(parts[0])
    return None


def desktop_environment(base):
    env = base.copy()
    allowed = {'DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'XDG_RUNTIME_DIR',
               'DBUS_SESSION_BUS_ADDRESS', 'XDG_SESSION_TYPE'}
    if sys.platform == 'linux':
        result = subprocess.run(['systemctl', '--user', 'show-environment'],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        if result.returncode == 0:
            for line in result.stdout.splitlines():
                key, separator, value = line.partition('=')
                if separator and key in allowed:
                    env[key] = value
    return env


def launch(receipt, tools):
    executable = Path(receipt['executable'])
    if not executable.exists() or sha(executable) != receipt['executableSHA256']:
        raise RuntimeError('Executable is missing or differs from the build receipt.')
    existing = running_pid(executable)
    if not existing:
        if tools.os == 'macos':
            command(['open', '-n', receipt['app']], env=tools.env)
        else:
            env = desktop_environment(tools.env)
            if not env.get('DISPLAY') and not env.get('WAYLAND_DISPLAY'):
                raise RuntimeError('No Linux desktop display is available to the signed-in user.')
            log = executable.parent / 'launch.log'
            with log.open('ab') as stream:
                subprocess.Popen([str(executable)], env=env, stdin=subprocess.DEVNULL,
                                 stdout=stream, stderr=subprocess.STDOUT, start_new_session=True)
    process_id = existing
    for _ in range(20):
        process_id = running_pid(executable)
        if process_id:
            break
        time.sleep(0.5)
    if not process_id:
        raise RuntimeError('No running process was found for the built application.')
    time.sleep(3)
    if running_pid(executable) != process_id:
        raise RuntimeError('The application exited during its startup check. Inspect launch.log beside the executable.')
    report = {'executable': str(executable), 'processId': process_id, 'reusedProcess': bool(existing),
              'verification': 'process running; visual rendering requires the recorded platform screen check'}
    print(json.dumps(report, indent=2), flush=True)
    return report


def _step_env(base, values):
    env = base.copy()
    limits = []
    for key, value in (values or {}).items():
        value = str(value)
        if "secrets." in value or "github.token" in value or "GITHUB_TOKEN" in key:
            limits.append('GitHub secret values were replaced by an empty local adapter value.')
            env[key] = ''
        elif "${{" in value:
            env[key] = ''
            limits.append(f'Expression for {key} is not available in the local runner.')
        else:
            env[key] = value
    return env, limits


class StepOutput:
    """A finished step: its exit code and its combined output."""

    def __init__(self, returncode, output):
        self.returncode = returncode
        self.output = output


def _stream(args, cwd, env, timeout):
    """Run a step, echoing its output as it appears and returning it as well.

    A workflow runner shows a step's output while the step runs. Buffering a
    multi-minute compile until it exits hides the only progress signal there
    is, both from the terminal and from the desktop app's log panel.
    """
    captured = []
    expired = []

    def expire():
        expired.append(True)
        # A step runs through a shell, so its real work is a grandchild holding
        # the same pipe. Killing only the shell leaves the read blocked until
        # that grandchild finishes, which defeats the timeout entirely.
        try:
            os.killpg(os.getpgid(process.pid), signal.SIGKILL)
        except OSError:
            process.kill()

    with subprocess.Popen(args, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          start_new_session=True) as process:
        timer = threading.Timer(timeout, expire)
        timer.start()
        try:
            for line in iter(process.stdout.readline, b''):
                text = line.decode('utf-8', errors='replace')
                captured.append(text)
                print(text, end='', flush=True)
            code = process.wait()
        finally:
            timer.cancel()
    output = ''.join(captured)
    if expired:
        raise subprocess.TimeoutExpired(args, timeout, output=output)
    return StepOutput(code, output)


def _run_workflow_command(step, source, env, timeout=1800):
    command_text = str(step.get('run') or '').strip()
    if not command_text:
        raise RuntimeError(f"Workflow step {step.get('index')} has an empty run command.")
    working = source / str(step.get('working-directory') or '.')
    working = working.resolve()
    working.relative_to(source.resolve())
    print('> ' + command_text, flush=True)
    return _stream(['/bin/sh', '-eu', '-c', command_text], working, env, timeout)


def _condition_enabled(step):
    condition = str(step.get('if') or '').strip()
    if not condition:
        return True, []
    if 'secrets.' in condition or 'SIGNING_CONFIGURED' in condition or 'github.token' in condition:
        return False, ['A GitHub secret or token condition is false in the local runner.']
    if condition in ('false', "'false'", '0'):
        return False, []
    if condition in ('true', "'true'", '1', 'always()'):
        return True, []
    # The local runner only accepts conditions whose event/ref was already
    # resolved by the controller. Unknown expressions fail closed.
    raise RuntimeError(f"Unsupported workflow condition: {condition}")


def _workflow_tauri_action(step, source, request, tools, env):
    args = str((step.get('with') or {}).get('args') or '')
    target = request.get('target') or tools.profile['target']
    bundle = request.get('bundle') or tools.profile['bundle']
    if (source / 'pnpm-lock.yaml').is_file():
        install = _run_workflow_command({'run': 'pnpm install --frozen-lockfile', 'working-directory': step.get('working-directory')}, source, env)
        if install.returncode:
            raise RuntimeError(f"pnpm install failed with exit code {install.returncode}.")
        base = ['pnpm', 'exec', 'tauri', 'build']
    elif (source / 'package-lock.json').is_file():
        install = _run_workflow_command({'run': 'npm ci', 'working-directory': step.get('working-directory')}, source, env)
        if install.returncode:
            raise RuntimeError(f"npm ci failed with exit code {install.returncode}.")
        base = ['npm', 'exec', '--', 'tauri', 'build']
    else:
        raise RuntimeError('The Tauri action requires a pnpm or npm lockfile.')
    command_args = base + ['--ci', '--no-sign', '--target', target, '--bundles', bundle, '--', '--locked']
    if args:
        command_args.extend(args.split())
    print('> ' + ' '.join(command_args), flush=True)
    result = _stream(command_args, source, env, 3600)
    if result.returncode:
        raise RuntimeError(f"Tauri action failed with exit code {result.returncode}.")
    output = source / 'src-tauri' / 'target' / target / 'release'
    if tools.os == 'macos':
        applications = list((output / 'bundle' / 'macos').glob('*.app'))
        if len(applications) != 1:
            raise RuntimeError('Expected exactly one macOS app bundle from the Tauri action.')
        application = applications[0]
        info = plistlib.loads((application / 'Contents/Info.plist').read_bytes())
        executable = application / 'Contents/MacOS' / info['CFBundleExecutable']
        artifacts = list((output / 'bundle').glob('**/*.dmg'))
    else:
        application = None
        artifact = request.get('artifact')
        if artifact:
            executable = (source / artifact).resolve()
            executable.relative_to(source.resolve())
        else:
            candidates = [path for path in output.iterdir() if path.is_file() and os.access(path, os.X_OK) and not path.name.endswith(('.so', '.d', '.rlib', '.a'))]
            if len(candidates) != 1:
                raise RuntimeError('Specify --artifact when the workflow produces multiple executable candidates.')
            executable = candidates[0]
        artifacts = list((output / 'bundle').glob('**/*.deb'))
    if not executable.is_file():
        raise RuntimeError('The workflow did not produce the expected executable.')
    return executable, application, artifacts


def ci_run(request, tools):
    """Execute the supported workflow steps in the provided native workspace."""
    tools.setup_system()
    tools.setup_user()
    root = project_root(request)
    signature = hashlib.sha256((ROOT / 'native.py').read_bytes() + json.dumps(request.get('workflow'), sort_keys=True).encode() + request['sourceHash'].encode()).hexdigest()
    directory = root / ('ci-' + signature[:24])
    source = directory / 'source'
    archive = Path(request['archive'])
    if sha(archive) != request['sourceHash']:
        raise RuntimeError('Source snapshot checksum mismatch.')
    directory.mkdir(parents=True, exist_ok=True)
    if not (directory / 'source-ready').exists():
        if source.exists():
            raise RuntimeError('Incomplete source extraction at ' + str(source))
        with zipfile.ZipFile(archive) as zipped:
            for member in zipped.namelist():
                (source / member).resolve().relative_to(source.resolve())
            zipped.extractall(source)
            for member in zipped.infolist():
                if not member.is_dir():
                    mode = (member.external_attr >> 16) & 0o777
                    if mode:
                        (source / member.filename).chmod(mode)
        (directory / 'source-ready').write_text(request['sourceHash'])
    base_env = tools.env.copy()
    for key in tuple(base_env):
        if key.startswith('APPLE_') or key in ('TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD', 'GITHUB_TOKEN'):
            del base_env[key]
    report = {'platform': tools.os, 'target': tools.profile['target'], 'success': False, 'status': 'failed',
              'sourceHash': request['sourceHash'], 'revision': request.get('revision'), 'dirty': request.get('dirty', False),
              'stages': {}, 'artifacts': [], 'limits': ['Local CI never signs, notarizes or uploads to GitHub.'], 'commands': [],
              'signing': 'unverified', 'publication': {'mode': 'local', 'uploaded': False}, 'attempts': 1}
    executable = None
    application = None
    try:
        doctor = tools.doctor()
        report['stages']['doctor'] = {'status': 'passed' if doctor['ready'] else 'failed', 'startedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'finishedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'tools': doctor.get('tools', {}), 'missing': doctor.get('missing', [])}
        if not doctor['ready']:
            raise RuntimeError('Native tool diagnosis failed: ' + ', '.join(doctor.get('missing', [])))
        for stage in ('setup', 'test', 'build', 'smoke', 'release'):
            stage_steps = (request.get('stages') or {}).get(stage, [])
            if not stage_steps:
                continue
            started = datetime.datetime.now(datetime.timezone.utc).isoformat()
            stage_result = {'status': 'passed', 'startedAt': started, 'steps': []}
            for step in stage_steps:
                step_started = datetime.datetime.now(datetime.timezone.utc).isoformat()
                adapter = step.get('adapter')
                result = {'index': step.get('index'), 'name': step.get('name'), 'adapter': adapter, 'startedAt': step_started}
                enabled, condition_limits = _condition_enabled(step)
                report['limits'].extend(condition_limits)
                if not enabled:
                    result.update(status='passed_with_limits', skipped=True, reason='if condition evaluated false locally')
                    stage_result['status'] = 'passed_with_limits'
                elif adapter == 'skip':
                    result.update(status='passed_with_limits', reason=step.get('reason'))
                    stage_result['status'] = 'passed_with_limits'
                    report['limits'].append(f"{stage} skipped: {step.get('reason')}")
                elif adapter in ('checkout', 'pnpm-setup', 'node-setup', 'rust-setup', 'cache'):
                    result.update(status='passed', localAdapter=True)
                elif adapter in ('artifact-upload', 'release'):
                    result.update(status='passed_with_limits', localAdapter=True)
                    stage_result['status'] = 'passed_with_limits'
                    report['limits'].append(f"{step.get('name')}: external GitHub service replaced by local artifact store")
                elif adapter == 'tauri-build':
                    executable, application, artifacts = _workflow_tauri_action(step, source, request, tools, base_env)
                    result.update(status='passed', executable=str(executable), artifacts=[str(path) for path in artifacts])
                    for path in artifacts:
                        report['artifacts'].append({'path': str(path), 'sha256': sha(path), 'size': path.stat().st_size})
                elif adapter == 'run':
                    env, limits = _step_env(base_env, {**(request.get('jobEnv') or {}), **(step.get('env') or {})})
                    report['limits'].extend(limits)
                    result['command'] = str(step.get('run') or '')
                    report['commands'].append({'stage': stage, 'command': result['command'], 'workingDirectory': step.get('working-directory') or '.'})
                    try:
                        completed = _run_workflow_command(step, source, env, timeout={'test': 1800, 'build': 3600, 'smoke': 600}.get(stage, 900))
                        # `output` is the step's interleaved stdout and stderr,
                        # the order a reader actually needs to diagnose it.
                        result.update(status='passed' if completed.returncode == 0 else 'failed',
                                      exitCode=completed.returncode, output=completed.output)
                        if completed.returncode:
                            stage_result['status'] = 'failed'
                            stage_result['error'] = f"step {step.get('index')} exited with {completed.returncode}"
                    except subprocess.TimeoutExpired as error:
                        output = error.output.decode(errors='replace') if isinstance(error.output, bytes) else (error.output or '')
                        result.update(status='timeout', timeoutSeconds={'test': 1800, 'build': 3600, 'smoke': 600}.get(stage, 900), output=output)
                        stage_result['status'] = 'failed'
                        stage_result['error'] = f"step {step.get('index')} timed out"
                else:
                    raise RuntimeError(f"Unsupported workflow adapter: {adapter}")
                result['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
                stage_result['steps'].append(result)
                if stage_result['status'] == 'failed':
                    break
            stage_result['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            report['stages'][stage] = stage_result
            if stage_result['status'] == 'failed':
                raise RuntimeError(stage_result.get('error', f'{stage} stage failed'))
        if executable:
            report['executable'] = str(executable)
            report['executableSHA256'] = sha(executable)
            report['app'] = str(application) if application else None
        report['success'] = True
        report['status'] = 'passed_with_limits' if report['limits'] else 'passed'
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        report['error'] = str(error)
        report['status'] = 'failed'
    report['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    print('CI_REPORT_BEGIN', flush=True)
    print(json.dumps(report, indent=2), flush=True)
    print('CI_REPORT_END', flush=True)
    return report


def build(request, tools, release=False, run=False):
    root = project_root(request)
    signature = hashlib.sha256((ROOT / 'native.py').read_bytes() + (ROOT / 'unix_tools.py').read_bytes() +
                               json.dumps(tools.config, sort_keys=True).encode() +
                               json.dumps({key: request[key] for key in ('sourceHash','framework','command','artifact')}, sort_keys=True).encode()).hexdigest()
    directory = root / signature[:24]
    source = directory / 'source'
    receipt_path = directory / 'result.json'
    latest = root / 'latest.json'
    if receipt_path.exists() and not release:
        previous = json.loads(receipt_path.read_text())
        executable = Path(previous['executable'])
        if executable.exists() and sha(executable) == previous['executableSHA256']:
            write_json(latest, previous)
            print('REUSED BUILD: ' + str(executable), flush=True)
            if run:
                write_json(root / 'running.json', launch(previous, tools))
            return previous
    archive = Path(request['archive'])
    if sha(archive) != request['sourceHash']:
        raise RuntimeError('Source snapshot checksum mismatch.')
    directory.mkdir(parents=True, exist_ok=True)
    if not (directory / 'source-ready').exists():
        if source.exists():
            raise RuntimeError('Incomplete source extraction at ' + str(source))
        with zipfile.ZipFile(archive) as zipped:
            for member in zipped.namelist():
                (source / member).resolve().relative_to(source.resolve())
            zipped.extractall(source)
            for member in zipped.infolist():
                if not member.is_dir():
                    mode = (member.external_attr >> 16) & 0o777
                    if mode:
                        (source / member.filename).chmod(mode)
        (directory / 'source-ready').write_text(request['sourceHash'])
    env = tools.env.copy()
    # Local rehearsal does not use production signing credentials inherited by
    # the shell. The original environment and credential stores are untouched.
    for key in tuple(env):
        if key.startswith('APPLE_') or key in ('TAURI_SIGNING_PRIVATE_KEY','TAURI_SIGNING_PRIVATE_KEY_PASSWORD'):
            del env[key]
    framework = request['framework']
    target = tools.profile['target']
    commands = []
    def checked(args):
        commands.append([str(arg) for arg in args])
        return command(args, env=env, cwd=source)
    if framework == 'tauri':
        if not (source / 'src-tauri/Cargo.lock').is_file():
            raise RuntimeError('Tauri builds require Cargo.lock.')
        if (source / 'pnpm-lock.yaml').is_file():
            checked(['pnpm','install','--frozen-lockfile'])
            base = ['pnpm','exec','tauri','build']
        elif (source / 'package-lock.json').is_file():
            checked(['npm','ci'])
            base = ['npm','exec','--','tauri','build']
        else:
            raise RuntimeError('Use a custom recipe for projects without an npm or pnpm lockfile.')
        bundle = tools.profile['bundle'] if release else ('app' if tools.os == 'macos' else 'deb')
        checked(base + ['--ci','--no-sign','--target',target,'--bundles',bundle,'--','--locked'])
        output = source / 'src-tauri' / 'target' / target / 'release'
        if tools.os == 'macos':
            applications = list((output / 'bundle' / 'macos').glob('*.app'))
            if len(applications) != 1:
                raise RuntimeError('Expected exactly one macOS app bundle.')
            application = applications[0]
            info = plistlib.loads((application / 'Contents/Info.plist').read_bytes())
            executable = application / 'Contents/MacOS' / info['CFBundleExecutable']
            architectures = command(['lipo','-archs',executable], capture=True)
            if 'arm64' not in architectures or 'x86_64' not in architectures:
                raise RuntimeError('The macOS universal application is missing a required architecture.')
        else:
            application = None
            artifact = request.get('artifact')
            if artifact:
                executable = (source / artifact).resolve()
                executable.relative_to(source.resolve())
            else:
                candidates = [path for path in output.iterdir() if path.is_file() and os.access(path, os.X_OK) and not path.name.endswith(('.so','.d','.rlib','.a'))]
                if len(candidates) != 1:
                    raise RuntimeError('Specify --artifact for multiple Linux executable candidates.')
                executable = candidates[0]
            architectures = 'aarch64'
        artifacts = list((output / 'bundle').glob('**/*.dmg')) if tools.os == 'macos' else list((output / 'bundle').glob('**/*.deb'))
    elif framework == 'custom':
        if not request.get('command') or not request.get('artifact'):
            raise RuntimeError('Custom builds require command and artifact.')
        checked(['/bin/sh','-eu','-c',request['command']])
        executable = (source / request['artifact']).resolve()
        executable.relative_to(source.resolve())
        application = None
        architectures = tools.os
        artifacts = [executable]
    else:
        raise RuntimeError('Native automatic recipes currently cover Tauri; supply a custom command for Wails.')
    if not executable.is_file():
        raise RuntimeError('The build did not produce the expected executable.')
    receipt = dict(request, platform=tools.os, builtAt=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                   workerSHA256=signature, commands=commands, executable=str(executable), executableSHA256=sha(executable),
                   app=str(application) if application else None, architectures=architectures, tools=tools.doctor()['tools'],
                   artifacts=[{'path':str(path),'sha256':sha(path)} for path in artifacts])
    if release:
        if not artifacts:
            raise RuntimeError('Release rehearsal did not produce an installer package.')
        if tools.os == 'macos':
            mount = directory / 'mounted-package'
            mount.mkdir(exist_ok=True)
            command(['hdiutil','attach','-readonly','-nobrowse','-mountpoint',mount,artifacts[0]])
            try:
                mounted_apps = list(mount.glob('*.app'))
                if len(mounted_apps) != 1:
                    raise RuntimeError('The disk image does not contain exactly one app.')
                installed = directory / 'installed' / mounted_apps[0].name
                installed.parent.mkdir(exist_ok=True)
                command(['ditto',mounted_apps[0],installed])
            finally:
                command(['hdiutil','detach',mount])
            receipt['app'] = str(installed)
            receipt['executable'] = str(installed / 'Contents/MacOS' / executable.name)
            receipt['executableSHA256'] = sha(receipt['executable'])
            receipt['installationCheck'] = 'DMG mounted read-only; bundled app copied to an isolated installation directory'
        else:
            # Inspect package metadata without pretending extraction verifies
            # maintainer scripts or a system-level package installation.
            command(['dpkg-deb','--info',artifacts[0]])
            receipt['installationCheck'] = 'package metadata checked; system installation not verified'
        receipt['publication'] = {'mode':'mock','uploaded':False,'productionSigningVerified':False}
        write_json(directory / 'mock-release.json', receipt)
    write_json(receipt_path, receipt)
    write_json(latest, receipt)
    print(json.dumps(receipt, indent=2), flush=True)
    if run or (release and tools.os == 'macos'):
        write_json(root / 'running.json', launch(receipt, tools))
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['doctor','setup-system','setup','build','release','run','ci'])
    parser.add_argument('--config', type=Path, default=ROOT / 'machine.json')
    parser.add_argument('--request', type=Path)
    parser.add_argument('--run', action='store_true')
    args = parser.parse_args()
    try:
        tools = Tools(json.loads(args.config.read_text()))
        if args.action == 'doctor':
            report = tools.doctor()
            print(json.dumps(report, indent=2))
            return 0 if report['ready'] else 1
        if args.action == 'setup-system':
            tools.setup_system()
            return 0
        if args.action == 'setup':
            tools.setup_system()
            tools.setup_user()
            return 0
        if not args.request:
            parser.error('--request is required for build, release and run')
        request = json.loads(args.request.read_text())
        if args.action == 'run':
            receipt = json.loads((project_root(request) / 'latest.json').read_text())
            write_json(project_root(request) / 'running.json', launch(receipt, tools))
        elif args.action == 'ci':
            report = ci_run(request, tools)
            return 0 if report['success'] else 1
        else:
            tools.setup_system()
            tools.setup_user()
            build(request, tools, release=args.action == 'release', run=args.run)
        return 0
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print('ERROR:', error, file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
