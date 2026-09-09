#!/usr/bin/env python3
"""Press a native GUI action, verify its result, and capture the app window."""
import argparse
import json
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / 'gui/src-tauri/target/release/bundle/macos/Build Machine.app'

FIND_BUTTON = '''
on findButton(anElement, labelText)
  tell application "System Events"
    if role of anElement is "AXButton" then
      if name of anElement is labelText or description of anElement is labelText then return anElement
    end if
    repeat with childItem in UI elements of anElement
      set found to my findButton(contents of childItem, labelText)
      if found is not missing value then return found
    end repeat
  end tell
  return missing value
end findButton
'''


def applescript(script, *args):
    return subprocess.check_output(['osascript', '-', *map(str, args)], input=script, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['doctor', 'setup'])
    parser.add_argument('--os', choices=['linux', 'windows', 'macos'], nargs='+', default=['linux'])
    parser.add_argument('--restart', action='store_true', help='Reopen the app before capturing its persisted result')
    args = parser.parse_args()
    subprocess.run(['open', str(APP)], check=True)
    time.sleep(1)
    button = {'doctor': '환경 진단', 'setup': '도구 준비'}[args.action]
    reports = ROOT / '.state/gui'
    before = set(reports.glob('*.json'))
    environment_names = {'windows': 'Windows 선택', 'linux': 'Ubuntu 선택', 'macos': 'macOS 선택'}
    select_commands = '\n'.join('if value of checkbox "%s" of workspace is %s then click checkbox "%s" of workspace' %
                                (name, 0 if os_name in args.os else 1, name) for os_name, name in environment_names.items())
    applescript(FIND_BUTTON + '''
tell application "System Events"
  tell first application process whose bundle identifier is "local.buildmachine.desktop"
    set frontmost to true
    set contentArea to UI element 1 of scroll area 1 of group 1 of group 1 of front window
    set navigationButton to my findButton(group 1 of contentArea, "설정")
    if navigationButton is missing value then error "Settings button is missing."
    if not (enabled of navigationButton) then error "Build Machine is busy."
    click navigationButton
    delay 0.3
    set workspace to group 2 of contentArea
    if not (enabled of button "환경 진단" of workspace) then error "An operation is running or the controller is unavailable."
    %s
    click button "%s" of workspace
  end tell
end tell
''' % (select_commands, button))
    deadline = time.monotonic() + 180
    result = None
    while time.monotonic() < deadline:
        new_reports = set(reports.glob('*.json')) - before
        for path in new_reports:
            report = json.loads(path.read_text())
            if report['action'] == args.action and report.get('status') != 'running' and set(report['results']) == set(args.os):
                result = report
                print('Native UI result: ' + str(path), flush=True)
                print(json.dumps(result, indent=2), flush=True)
                break
        if result:
            break
        time.sleep(0.25)
    if result is None:
        raise RuntimeError('The actual GUI action did not produce a result within 180 seconds.')
    # Allow the completed command to return through Tauri before closing/capturing.
    time.sleep(0.5)
    if args.restart:
        applescript('tell application id "local.buildmachine.desktop" to quit')
        time.sleep(0.5)
        subprocess.run(['open', str(APP)], check=True)
        time.sleep(2)
    screenshot = ROOT / '.state' / ('gui-native-%s-%s%s.png' % ('-'.join(args.os), args.action, '-reopened' if args.restart else ''))
    window_id = subprocess.check_output(['osascript', '-l', 'JavaScript', str(ROOT / 'tests/gui_window.js')], text=True).strip()
    subprocess.run(['/usr/sbin/screencapture', '-x', '-l', window_id, str(screenshot)], check=True)
    print('Native window capture: ' + str(screenshot), flush=True)
    if not all(item['success'] for item in result['results'].values()):
        raise RuntimeError('The real operation failed. No command was retried.')
    stored = json.loads((ROOT / '.state/tool-status.json').read_text())['results']
    for os_name in args.os:
        if stored[os_name]['action'] != args.action or stored[os_name]['finishedAt'] != result['results'][os_name]['finishedAt']:
            raise RuntimeError('The persisted tool result does not match the native GUI action.')


if __name__ == '__main__':
    main()
