#!/usr/bin/env python3
"""Verify native project/settings navigation and folder-picker cancellation."""
import argparse
import json
from pathlib import Path
import subprocess
import time

from native_gui import APP, ROOT, FIND_BUTTON, applescript


def capture(name):
    screenshot = ROOT / '.state' / name
    window_id = subprocess.check_output(['osascript', '-l', 'JavaScript', str(ROOT / 'tests/gui_window.js')], text=True).strip()
    subprocess.run(['/usr/sbin/screencapture', '-x', '-l', window_id, str(screenshot)], check=True)
    return str(screenshot)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('project', type=Path, help='An already registered project with a unique folder name')
    parser.add_argument('--restart', action='store_true')
    args = parser.parse_args()
    project = str(args.project.expanduser().resolve(strict=True))
    settings_file = Path.home() / 'Library/Application Support/local.buildmachine.desktop/preferences.json'
    before = json.loads(settings_file.read_text())
    assert any(item['path'] == project for item in before['projects']), 'Register this project before running the check.'
    assert sum(Path(item['path']).name == Path(project).name for item in before['projects']) == 1, 'This UI check requires a unique project folder name.'
    subprocess.run(['open', str(APP)], check=True)
    time.sleep(1)
    applescript(FIND_BUTTON + '''
tell application "System Events"
  tell first application process whose bundle identifier is "local.buildmachine.desktop"
    set frontmost to true
    if exists sheet 1 of front window then error "Close the current dialog before running this check."
    set contentArea to UI element 1 of scroll area 1 of group 1 of group 1 of front window
    set sidebar to group 1 of contentArea
    if my findButton(sidebar, "프로젝트 화면") is not missing value then error "Unexpected Projects menu."
    if my findButton(sidebar, "환경 화면") is not missing value then error "Unexpected Environment menu."
    set settingsButton to my findButton(sidebar, "설정")
    if settingsButton is missing value then error "Settings button is missing."
    if not (enabled of settingsButton) then error "Build Machine is busy."
    click settingsButton
    delay 0.4
    set settingsArea to group 2 of contentArea
    if not (exists button "환경 진단" of settingsArea) then error "Settings has no diagnosis action."
    if not (exists button "도구 준비" of settingsArea) then error "Settings has no setup action."
    if exists button "빌드 시작" of settingsArea then error "Settings contains project build controls."
  end tell
end tell
''')
    settings_screenshot = capture('gui-native-settings.png')
    applescript(FIND_BUTTON + '''
on run argv
  tell application "System Events"
    tell first application process whose bundle identifier is "local.buildmachine.desktop"
      set contentArea to UI element 1 of scroll area 1 of group 1 of group 1 of front window
      set projectButton to my findButton(group 1 of contentArea, item 1 of argv)
      if projectButton is missing value then error "Registered project is missing."
      click projectButton
      delay 0.4
      set projectArea to group 2 of contentArea
      if not (exists button "빌드 시작" of projectArea) then error "Selected project has no build action."
      if exists button "환경 진단" of projectArea then error "Project contains shared diagnosis controls."
      set addButton to my findButton(group 1 of contentArea, "프로젝트 추가")
      click addButton
      delay 0.5
      if not (exists sheet 1 of front window) then error "Native folder picker did not open."
      set pickerArea to splitter group 1 of sheet 1 of front window
      if exists button "Cancel" of pickerArea then
        click button "Cancel" of pickerArea
      else
        click button "취소" of pickerArea
      end if
    end tell
  end tell
end run
''', Path(project).name)
    time.sleep(0.5)
    after = json.loads(settings_file.read_text())
    assert after['selectedProject'] == project and after['page'] == 'projects'
    assert after['projects'] == before['projects'], 'Navigation or picker cancellation changed registrations/options.'
    assert after['environmentPlatforms'] == before['environmentPlatforms']
    if args.restart:
        applescript('tell application id "local.buildmachine.desktop" to quit')
        time.sleep(0.5)
        subprocess.run(['open', str(APP)], check=True)
        time.sleep(2)
        assert json.loads(settings_file.read_text()) == after
    project_screenshot = capture('gui-native-selected-project.png')
    receipt = {'project': project, 'settingsFile': str(settings_file), 'projectOptionsPreserved': True,
               'nativePickerCancelled': True, 'reopened': args.restart,
               'screenshots': [settings_screenshot, project_screenshot]}
    (ROOT / '.state/gui-projects.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
