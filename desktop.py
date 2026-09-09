#!/usr/bin/env python3
"""Develop or build the Build Machine desktop application."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

from unix_tools import Tools, command

ROOT = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['dev', 'build', 'test'])
    parser.add_argument('--run', action='store_true')
    args = parser.parse_args()
    try:
        tools = Tools(json.loads((ROOT / 'machine.json').read_text()))
        if tools.os != 'macos':
            raise RuntimeError('The initial desktop controller runs on macOS.')
        tools.setup_system()
        tools.setup_user()
        gui = ROOT / 'gui'
        command(['pnpm','install','--frozen-lockfile'], env=tools.env, cwd=gui)
        if args.action == 'dev':
            command(['pnpm','exec','tauri','dev'], env=tools.env, cwd=gui)
        elif args.action == 'test':
            command(['pnpm','run','build'], env=tools.env, cwd=gui)
            command(['cargo','test','--locked'], env=tools.env, cwd=gui / 'src-tauri')
            command(['pnpm','exec','playwright','install','chromium'], env=tools.env, cwd=gui)
            command(['pnpm','test'], env=tools.env, cwd=gui)
        else:
            command(['pnpm','exec','tauri','build','--ci','--no-sign','--bundles','app','--','--locked'], env=tools.env, cwd=gui)
            application = gui / 'src-tauri/target/release/bundle/macos/Build Machine.app'
            if not application.is_dir():
                raise RuntimeError('The desktop application bundle was not produced.')
            print('Application: ' + str(application), flush=True)
            if args.run:
                command(['open', application])
        return 0
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print('ERROR:', error, file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
