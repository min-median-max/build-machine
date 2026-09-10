# 검증 기록

[English](verification.md) · [GUI 검증](GUI-VERIFICATION.ko.md)

데스크톱 GUI, Windows Node.js 22.23.2 준비, 영구 환경 결과와 아직 해결되지 않은 Parallels 실행 오류는 [GUI-VERIFICATION.ko.md](GUI-VERIFICATION.ko.md)에 따로 기록합니다.

> **이 파일은 0.1.0이 제거한 Python·PowerShell 구현에서 실제로 일어난 일의 기록입니다.**
> 그 실행들의 기록으로 남겨 두며, Rust 구현을 설명하도록 고쳐 쓰지 않습니다. 그렇게 하면
> 기록이 거짓이 됩니다. Rust 재작성이 무엇을 보였고 무엇을 보이지 않았는지는 바로 아래에 있습니다.

## Rust 재작성

재작성은 macOS 호스트에서 `cargo test --workspace`(40건)와 `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm --dir gui run build`, `pnpm --dir gui test`(브라우저 14건)를 통과합니다. 워커는 `aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`, `aarch64-pc-windows-msvc` 세 타깃에서 경고 없이 컴파일됩니다.

이 맥에서 실제로 수행한 것:

- `build-machine doctor --os macos`가 실제 설치된 툴체인에 대해 실제 워커 바이너리를 거쳐 `ready: true`를 보고했습니다.
- `build-machine ci validate ~/Work/airdata --workflow .github/workflows/release-macos.yml`이 **test** 게이트 누락으로 거부합니다. 바로잡은 계약과 일치합니다 — `actions/checkout`은 더 이상 테스트 단계를 대신하지 않습니다.
- 성공한 실행이 가리키는 작업 로그에 실행 요약과 플랫폼별 명령 출력 위치가 들어 있습니다.

**수행하지 않은 것. 아래 어느 것도 실행된 적이 없으며 동작한다고 읽어서는 안 됩니다:**

- Windows와 Ubuntu 게스트 경로 전부: 진단, 프로비저닝, 빌드, 실행, 워크플로 재현. 전부 새 코드이고, 그것을 상대로 VM을 띄운 적이 없습니다.
- 특히 Windows 프로비저닝. MSVC 설치, Authenticode 검증, 레지스트리 읽기·쓰기, PATH 브로드캐스트, 창 확인이 .NET 래퍼에서 Win32 직접 호출로 옮겨졌고, 실제 Windows ARM64 기계만이 그 동작을 보여줄 수 있습니다.
- 릴리즈 워크플로. 한 번도 실행되지 않았습니다. 어떤 러너도 워커를 빌드한 적이 없고, 그것으로 앱을 조립한 적도 없습니다.
- 게스트 VM 안에서 워커를 빌드하는 `cargo xtask worker --os windows linux`.
- 다시 빌드한 macOS 앱 번들, 사이드카 배치, 네이티브 대시보드 렌더링.

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
