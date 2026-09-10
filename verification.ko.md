# 검증 기록

[English](verification.md) · [GUI 검증](GUI-VERIFICATION.ko.md)

데스크톱 GUI, Windows Node.js 22.23.2 준비, 영구 환경 결과와 아직 해결되지 않은 Parallels 실행 오류는 [GUI-VERIFICATION.ko.md](GUI-VERIFICATION.ko.md)에 따로 기록합니다.

## 결함 수정

기록된 결함 8건을 고쳤고, 각각은 수정 없이는 실패하는 테스트로 고정했습니다. macOS 호스트에서 `python3 -m unittest discover -s tests`가 41건, `~/.cargo/bin/cargo test --manifest-path gui/src-tauri/Cargo.toml`이 16건(설정된 Ubuntu doctor 테스트는 계속 제외), `pnpm --dir gui test`가 브라우저 테스트 14건, `pnpm --dir gui run build`가 TypeScript·Vite 빌드를 통과합니다.

이 테스트가 증명하는 것과 증명하지 않는 것:

- Windows `setup` 크래시는 스텁 머신 객체를 쓰는 컨트롤러 테스트로 덮었습니다. 그 명령이 거쳤을 게스트 경로는 여전히 실행된 적이 없습니다. Windows 준비는 `winbuild.py setup`으로만 수행했고 `build.py setup --os windows`는 실제 VM에서 실행하지 않았습니다.
- 보존 정책, 실행 기록 배치, 이전 기록 이전은 임시 상태 디렉터리 테스트로 덮었습니다. 이전은 이 체크아웃의 실제 `.state`에서 한 번 수행했습니다. 이전 보고서 20건이 기록 시각을 유지한 채 `.state/runs/` 아래로 옮겨졌고 남은 레거시 파일은 없습니다.
- 작업 로그에 실행 요약과 플랫폼별 명령 출력 위치가 남는 것을 확인했습니다. 게스트가 아니라 로컬 프로세스를 작업자로 둔 컨트롤러 실행으로 확인했습니다.
- 단계 출력 스트리밍, 종료 코드, 시간 초과의 프로세스 그룹 종료는 macOS에서 실제 자식 프로세스를 쓰는 테스트로 덮었습니다. `prlctl exec`로 게스트를 거치는 스트리밍은 다시 실행하지 않았습니다.
- 대시보드 변경은 보고서 픽스처를 쓰는 Rust 테스트와 목 API를 쓰는 브라우저 테스트로 덮었습니다. 네이티브 렌더링을 확인한 것은 아닙니다. 이 변경으로 macOS 앱을 다시 빌드해 확인하지 않았습니다.
- 스테이지 게이트 수정은 유지 중인 `airdata/.github/workflows/release-macos.yml`로 확인했습니다. 수정 전에는 `ci validate`가 **smoke** 게이트 누락만으로 거부했습니다. `actions/checkout` 단계가 test 스테이지로 읽히고 있었기 때문입니다. 수정 후에는 **test** 게이트 누락으로 거부합니다. 이 워크플로에는 두 단계가 모두 없으므로, 기존에 기록한 거부 결과 자체는 맞았지만 test 게이트는 실제로 작동한 적이 없습니다.

AIRDATA 소스, GitHub workflow, 서명 자격 증명, 업로드, 릴리스는 변경하거나 호출하지 않았습니다.

## Workflow 재현 구현

지원하는 GitHub Actions subset을 엄격하게 읽고 로컬에서 실행하는 workflow 재현을 추가했습니다. 테스트는 지원하지 않는 action·게이트 누락의 명시적 실패, 변경할 수 없는 ref 스냅샷, 저장소 루트 검증, 순차·병렬 보고서 동일성, 네이티브 workflow 단계 실행을 포함합니다. 현재 테스트 수는 위 "결함 수정"에 기록했습니다. 이 항목을 처음 기록할 당시 수는 Python 23개, Rust 10개, 브라우저 13개였습니다.

현재 유지하는 AIRDATA workflow에는 test나 smoke 단계와 `build-machine: skip ... reason=...` 주석이 없습니다. 따라서 `ci validate`는 필요한 게이트 누락 메시지로 거부하며 저장소가 생략 사유를 문서화한 뒤에 재현할 수 있습니다. 이 검사에서는 AIRDATA 소스·GitHub workflow·서명 자격 증명·업로드·릴리즈를 변경하거나 실행하지 않았습니다. Windows와 Ubuntu 재현 코드는 통제된 테스트로 검증했으며 실제 재현은 설정한 게스트에서 별도 실행해야 합니다.
