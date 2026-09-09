#!/usr/bin/env python3
"""Open the native dashboard and verify direct project navigation and persistence."""
import argparse
import json
from pathlib import Path
import subprocess
import time

from native_gui import APP, ROOT, FIND_BUTTON, applescript
from native_projects import capture


def open_dashboard():
    applescript(FIND_BUTTON + '''
tell application "System Events"
  tell first application process whose bundle identifier is "local.buildmachine.desktop"
    set frontmost to true
    set contentArea to UI element 1 of scroll area 1 of group 1 of group 1 of front window
    set dashboardButton to my findButton(group 1 of contentArea, "대시보드")
    if dashboardButton is missing value then error "Dashboard navigation is missing."
    click dashboardButton
    delay 0.5
    set dashboardArea to group 2 of contentArea
    set refreshButton to my findButton(dashboardArea, "빌드 기록 새로고침")
    if refreshButton is missing value then error "Dashboard did not open."
    if my findButton(dashboardArea, "환경 진단") is not missing value then error "Shared setup belongs in Settings."
  end tell
end tell
''')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--restart', action='store_true')
    args = parser.parse_args()
    settings_file = Path.home() / 'Library/Application Support/local.buildmachine.desktop/preferences.json'
    before = json.loads(settings_file.read_text())
    subprocess.run(['open', str(APP)], check=True)
    time.sleep(1)
    open_dashboard()
    if before['projects']:
        path = before['projects'][0]['path']
        applescript(FIND_BUTTON + '''
on run argv
  tell application "System Events"
    tell first application process whose bundle identifier is "local.buildmachine.desktop"
      set contentArea to UI element 1 of scroll area 1 of group 1 of group 1 of front window
      set projectButton to my findButton(group 2 of contentArea, item 1 of argv)
      if projectButton is missing value then error "The registered project is absent from Dashboard."
      if not (enabled of projectButton) then error "Build Machine is busy."
      click projectButton
      delay 0.3
      if my findButton(group 2 of contentArea, "빌드 시작") is missing value then error "Project controls did not open."
    end tell
  end tell
end run
''', Path(path).name + ' 빌드 화면')
        assert json.loads(settings_file.read_text())['selectedProject'] == path
        open_dashboard()
    after = json.loads(settings_file.read_text())
    assert after['page'] == 'dashboard'
    assert after['projects'] == before['projects']
    assert after['environmentPlatforms'] == before['environmentPlatforms']
    if args.restart:
        applescript('tell application id "local.buildmachine.desktop" to quit')
        time.sleep(0.5)
        subprocess.run(['open', str(APP)], check=True)
        time.sleep(2)
        assert json.loads(settings_file.read_text()) == after
    screenshot = capture('gui-native-dashboard.png')
    receipt = {'settingsFile': str(settings_file), 'registeredProjects': len(after['projects']),
               'projectNavigationVerified': bool(after['projects']), 'reopened': args.restart, 'screenshot': screenshot}
    (ROOT / '.state/gui-dashboard.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
