#!/usr/bin/env python3
"""Compare installed Parallels execution clients using harmless Linux commands."""
import argparse
import json
from pathlib import Path
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--transport', choices=['prlctl', 'prlexec', 'all'], default='all')
    parser.add_argument('--iterations', type=int, default=20)
    args = parser.parse_args()
    if args.iterations < 1:
        parser.error('iterations must be positive')
    vm = json.loads((ROOT / 'machine.json').read_text())['platforms']['linux']['vm']
    transports = ['prlctl', 'prlexec'] if args.transport == 'all' else [args.transport]
    failures = []
    counts = {name: 0 for name in transports}
    script = 'id -u; printf "smoke-stdout\\n"; printf "smoke-stderr\\n" >&2; exit 7'
    guest_command = shlex.join(['/bin/sh', '-c', script])
    for iteration in range(args.iterations):
        for transport in transports:
            for root in [True, False]:
                if transport == 'prlctl':
                    command = ['prlctl', 'exec', vm] + ([] if root else ['--current-user'])
                else:
                    command = ['/Applications/Parallels Desktop.app/Contents/MacOS/prlexec', '--vm', vm]
                    if root:
                        command += ['--user', 'root']
                result = subprocess.run(command + [guest_command], stdin=subprocess.DEVNULL,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=30)
                output = result.stdout.splitlines()
                valid_user = bool(output) and output[0].isdigit() and ((output[0] == '0') == root)
                valid = result.returncode == 7 and valid_user and output[1:] == ['smoke-stdout'] and result.stderr.strip() == 'smoke-stderr'
                counts[transport] += 1
                if not valid:
                    failure = {'iteration': iteration + 1, 'transport': transport, 'root': root,
                               'exitCode': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr}
                    failures.append(failure)
                    print(json.dumps(failure), flush=True)
        print('Completed iteration %s/%s' % (iteration + 1, args.iterations), flush=True)
    print(json.dumps({'attempts': counts, 'failures': failures}, indent=2), flush=True)
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
