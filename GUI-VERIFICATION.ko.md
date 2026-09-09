# 데스크톱 GUI 검증

[English](GUI-VERIFICATION.md)

2026-09-09 macOS ARM64와 Parallels Desktop 27.0.1 (58670)에서 검증했습니다. `4f7465e` 기반 작업 트리에서 검사했으며 첫 GUI 런타임 소스 SHA-256은 `3b851171f79eb117b25daeef2fe0fae96e16c9d725de735c698268d7e605eefc`입니다. 빌드한 실행 파일 SHA-256은 `99e9ceeab09d2ccc2f8894bdb2303bea4038292c5e6f2416fabcdbffe27d5521`입니다. 앱은 `gui/src-tauri/target/release/bundle/macos/Build Machine.app`의 서명하지 않은 ARM64 `0.0.1` 번들입니다.

- Python 제어/소스/캐시 테스트 14개를 통과했습니다. 여러 OS의 빌드가 한 스냅샷을 사용하고, 한 환경 실패 후에도 다음 환경을 실행하며, 도구 결과를 환경과 설정별로 보존하고, Windows 오류에 실제 진단 원인을 포함합니다.
- Rust 연결 테스트 5개를 통과했습니다. 별도로 명시 실행한 실제 Ubuntu doctor 테스트도 통과했습니다. 인자 경계, 표준 출력/오류와 실패 종료 코드를 보존하며 결과 파일이 없으면 성공으로 간주하지 않습니다.
- Tauri API를 명시적으로 모의 처리한 브라우저 테스트 6개를 통과했습니다. 폴더/플랫폼 선택, 실행 옵션, 실시간 로그, 작업 중 컨트롤 비활성화, 실패 표시, 다시 열기/빌드 후 도구 결과 유지, 결과 파일 없는 실패를 검사합니다. 이 테스트는 실제 VM 실행을 입증하지 않습니다.
- `desktop.py build`로 잠긴 화면/Rust 의존성을 사용해 네이티브 앱을 만들었습니다. `desktop.py dev`는 화면과 Rust 개발을 함께 실행하는 유지보수 명령입니다.
- 실제 앱 컨트롤로 Ubuntu doctor와 준비를 실행했습니다. 20:07:44 KST 준비가 성공했고 앱을 다시 열어도 기록이 유지됐습니다. 로그는 `.state/logs/`의 `20260909-200711-792265-matrix.log`, `20260909-200743-118295-matrix.log`입니다.
- Windows doctor는 설정된 Node.js 22.23.2와 설치된 24.21.0의 불일치를 정확히 거부했습니다. 실제 GUI에 `node: expected v22.23.2; found v24.21.0`을 표시했으며 `.state/gui-native-windows-doctor.png`에 기록했습니다. 로그는 `20260909-201043-388895-matrix.log`입니다.
- 실제 Windows 준비가 Node.js 22.23.2를 설치하고 다른 도구를 재사용했으며 20:11:38에 누락 항목 없이 doctor를 통과했습니다. 로그는 `20260909-201120-666400-matrix.log`, 앱을 다시 연 화면은 `.state/gui-native-windows-setup-reopened.png`입니다.
- 세 OS를 함께 선택한 실제 GUI 준비가 20:13:29–30에 통과했습니다. Windows, Ubuntu, macOS가 정의된 모든 도구를 재사용했고 Windows PATH는 바뀌지 않았습니다. 다시 열어도 세 완료 표시와 시각이 유지됐습니다. 로그는 `20260909-201319-880035-matrix.log`, 화면은 `.state/gui-native-windows-linux-macos-setup-reopened.png`입니다.

네이티브 동작과 창 캡처는 `tests/native_gui.py`, `tests/gui_window.js`로 유지보수합니다. AIRDATA 소스, 저장 데이터와 실행 중인 게스트 앱은 보존했습니다. 준비 검사 중 GUI를 통한 앱 빌드/실행과 패키지 설치는 반복하지 않았습니다. 기존 AIRDATA 검증 범위는 [verification.md](verification.md)에 있습니다.

## 등록 프로젝트와 공통 설정

2026-09-09에 `7c78a15` 기반 작업 트리에서 검증했습니다. 검사한 `gui/` Git 트리는 `bfa44cc35dcb4db7f2d0593a24c75e9882555d3f`입니다. 배포한 실행 파일 SHA-256은 `202e71b2ed40139f7e3458e5581eda2dc9c5d33cfa56b4a512bce0d883f9c3d9`이며 위와 같은 서명하지 않은 ARM64 `0.0.1` 번들 경로에 있습니다.

- `python3 desktop.py test`: Rust 테스트 8개와 브라우저 테스트 9개를 통과했습니다. 실제 Ubuntu 연결 테스트는 별도 명시 실행 대상입니다. 설정 테스트는 최초 선택 프로젝트 보존, 실제 Git 폴더 검증, 실제 경로의 중복 등록, 원자적 저장, 환경/프로젝트별 독립 옵션을 검사합니다. 브라우저는 명시적 등록, 프로젝트 선택, 등록 해제, 옵션 보존, 공통 도구 결과와 실패 출력을 검사합니다. 최초 화면에는 워크스페이스 그룹이나 별도의 환경/프로젝트 탐색 항목이 없습니다.
- 빌드한 앱에서 `python3 tests/native_projects.py /Users/maxkwon/Work/airdata --restart`를 통과했습니다. 하단 설정 버튼에서 진단/준비를 열고, 등록된 AIRDATA 항목을 선택하면 빌드/실행을 바로 열며, `+`가 실제 네이티브 폴더 선택기를 엽니다. 선택기를 취소하고 앱을 다시 열어도 등록 목록과 옵션을 보존합니다. 증거는 `.state/gui-projects.json`, `.state/gui-native-settings.png`, `.state/gui-native-selected-project.png`입니다.
- 네이티브 폴더 선택기 검증 범위는 열기와 취소입니다. 폴더 선택부터 등록 저장까지의 전체 네이티브 UI 자동화 성공은 확인하지 못했습니다. 등록 저장은 실제 파일시스템을 사용하는 Rust 테스트와 모의 브라우저 통합 테스트로 별도 검증했습니다.
- 설정에서 실행한 `python3 tests/native_gui.py doctor --os windows linux macos --restart`가 20:55:09–15 KST에 통과했습니다. 실제 세 진단 결과와 완료 시각이 앱을 다시 열어도 유지됐습니다. 로그는 `.state/logs/20260909-205509-263367-matrix.log`, 화면은 `.state/gui-native-windows-linux-macos-doctor-reopened.png`입니다.

등록/설정 변경은 기존 앱 설정 디렉터리에 저장합니다. AIRDATA 소스는 변경되지 않았습니다. 탐색 검증은 AIRDATA를 다시 빌드하거나 실행하지 않았으며 세 OS 릴리즈 완료를 입증하지 않습니다.

## 대시보드와 작업 보고서

2026-09-09에 `8640de2` 기반 작업 트리에서 검증했습니다. 검사한 `gui/` Git 트리는 `ff571a6672f76673c85696580cc5cb954664b928`, `build.py` SHA-256은 `b3122b81dbab7067ce91d6e2607a068cbdf58fff6f0739240801d27568555d4a`입니다. 배포한 실행 파일 SHA-256은 `34b21efd55625f2ad8639bc4db8de591699d1069610149c5ab672827f5feb8e7`이며 같은 서명하지 않은 ARM64 `0.0.1` 번들 경로를 사용합니다.

- Python 테스트 16개를 통과했습니다. 새 격리 테스트로 소스 준비 실패를 실패 보고서에 보존하고 다음 플랫폼 실행 전에 완료한 플랫폼을 기록하는지 검사했습니다.
- Rust 테스트 13개를 통과했습니다. 실제 Ubuntu 연결 검사는 별도 명시 실행 대상입니다. 대시보드 테스트는 미등록 프로젝트와 전달 요청 제외, 최신 실패/미완료 결과 유지, 읽을 수 없는 기록 표시와 제어 도구 로그 디렉터리로 제한한 로그 열기를 검사합니다. 미완료 보고서만 남기고 종료 코드 0으로 끝나는 자식 프로세스도 거부합니다.
- 데스크톱 API를 명시적으로 모의 처리한 브라우저 테스트 12개를 통과했습니다. 대시보드는 요약 수치, 프로젝트 직접 탐색, 선택한 로그 열기, 빌드 진행 중 탐색, 변경 컨트롤 비활성화와 다시 열었을 때 실패 결과 보존을 검사합니다. 빈 기록이나 읽을 수 없는 기록으로 성공을 확정하지 않습니다.
- 빌드한 앱에서 `python3 tests/native_dashboard.py --restart`를 통과했습니다. 실제 대시보드는 등록된 AIRDATA 프로젝트 1개와 기존 Ubuntu 빌드 기록 3개를 표시했습니다. 가장 최근에 기록된 Ubuntu 빌드는 성공 상태였습니다. 프로젝트 행에서 AIRDATA 빌드 컨트롤을 열었고 앱을 다시 열어도 대시보드 선택을 유지했습니다. 증거는 `.state/gui-dashboard.json`, `.state/gui-native-dashboard.png`입니다.

브라우저 대시보드 화면 `.state/gui-browser-dashboard.png`는 테스트 데이터를 사용합니다. 네이티브 화면은 실제 제어 명령 보고서를 읽습니다. 이 검사로 AIRDATA를 다시 빌드하거나 게스트 앱을 다시 실행하거나 도구를 설치하거나 릴리즈를 만들지 않았습니다. 기존 Windows 전용 전달 요청은 완료된 빌드 보고서로 취급하지 않습니다. 네이티브에서 진행 중인 빌드 관찰은 반복하지 않았으며 진행 동작은 브라우저 테스트와 통제한 자식 프로세스 테스트로 검사했습니다.

## 알려진 Parallels 실행 오류

19:48:07 Ubuntu 준비가 종료 코드 255와 `PrlJob_GetResult: Invalid argument`로 실패했습니다. 실패한 실제 호출은 root의 `prlctl exec ... native.py setup-system`입니다. 이후 실행 도구를 수정하지 않은 상태에서 같은 작업이 성공했습니다.

`python3 tests/parallels_smoke.py --iterations 30`으로 무해한 명령을 직접 실행한 `prlctl` 60회 중 2회 실패를 재현했습니다. root에서 `PrlJob_GetResult`, 현재 사용자에서 `PrlJob_GetRetCode`가 각각 종료 코드 255로 실패했습니다. `prlexec` 60회는 통과했지만 설치된 `prlexec` 스크립트도 내부에서 `prlctl`을 호출하므로 수정이나 별도의 안정적인 전송 방식이 입증된 것은 아닙니다. 정확한 내부 원인은 확인하지 못했습니다. 현재 도구 준비는 성공하지만 실행 도구의 간헐적 실패는 미해결입니다.

제어 명령은 실패를 표시하고 명령·출력·종료 코드를 기록합니다. 게스트 실행 결과가 불확실한 경우 설치나 빌드를 자동 반복하지 않습니다. Parallels 업데이트, VM 초기화, 비공개 SDK 대체, GitHub 워크플로 실행, 릴리즈 게시는 수행하지 않았습니다.
