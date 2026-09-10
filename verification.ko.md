# 검증 기록

[English](verification.md) · [GUI 검증](GUI-VERIFICATION.ko.md)

데스크톱 GUI, Windows Node.js 22.23.2 준비, 영구 환경 결과와 아직 해결되지 않은 Parallels 실행 오류는 [GUI-VERIFICATION.ko.md](GUI-VERIFICATION.ko.md)에 따로 기록합니다.

> **이 파일은 0.1.0이 제거한 Python·PowerShell 구현에서 실제로 일어난 일의 기록입니다.**
> 그 실행들의 기록으로 남겨 두며, Rust 구현을 설명하도록 고쳐 쓰지 않습니다. 그렇게 하면
> 기록이 거짓이 됩니다. Rust 재작성이 무엇을 보였고 무엇을 보이지 않았는지는 바로 아래에 있습니다.

## Rust 재작성

재작성은 macOS 호스트에서 `cargo test --workspace`(40건)와 `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm --dir gui run build`, `pnpm --dir gui test`(브라우저 14건)를 통과합니다. 워커는 `aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`, `aarch64-pc-windows-msvc` 세 타깃에서 경고 없이 컴파일됩니다.

### 실제 가상 머신에서 수행한 것

게스트: `Windows 11`(Windows 11 Pro ARM64), `Ubuntu 26.04 ARM64`. 둘 다 실행 중이고 데스크톱 사용자가 로그인해 있었습니다. 프로젝트: `/Users/maxkwon/Work/airdata`, 리비전 `c798dc27c569fdcd4ab83a6dad5722ab739f41f0`, 소스 해시 `39a73fa25283aabe62c239a5ff870ac19435a9868ea9e911b689dfe6d1e04eef`.

- `cargo xtask worker --os windows linux`가 각 게스트 워커를 그 VM 안에서 빌드해 가져왔습니다. cargo가 읽기 전용 공유에서 워크스페이스를 직접 읽고 게스트의 target 디렉터리에만 쓰므로 복사가 없습니다.
- `doctor`가 세 환경 모두, 개별 실행과 매트릭스 실행 양쪽에서 통과했습니다. Windows는 `Windows 11 Pro`, `ARM64`, MSVC `17.14.37628.2`, WebView2 `152.0.4191.66`을 보고했습니다. 레지스트리 읽기, vswhere 조회, 아키텍처 확인이 새로 작성한 Win32 직접 호출입니다.
- `build`가 Windows·Ubuntu·macOS에서 통과했습니다. Ubuntu는 ARM64 실행 파일과 `data_0.1.0_arm64.deb`를, macOS는 실제 유니버설 앱을 만들었고 `lipo -archs`가 `x86_64 arm64`를 보고했습니다. Ubuntu 빌드를 반복하면 실행 파일 체크섬이 그대로인 `REUSED BUILD`가 나왔습니다.
- `run`이 Windows와 Ubuntu에서 통과했고, 반복하면 실행 중인 프로세스를 재사용했습니다. Windows에서는 창 확인이 제목 `data`와 `responding: true`를 보고했고, `.state/…/windows-app.png`에 한국어 보드 화면이 렌더링된 모습이 남아 있습니다.
- `ci validate`는 `airdata/.github/workflows/release-macos.yml`을 **test** 게이트 누락으로 거부합니다. 바로잡은 계약과 일치합니다 — `actions/checkout`은 더 이상 테스트 단계를 대신하지 않습니다.
- 매트릭스 빌드 한 번에서 Ubuntu가 `PrlJob_GetResult: Invalid argument`로 실패로 기록됐습니다. 동일한 명령을 다시 실행하면 성공했고 컨트롤러로도 두 번 더 통과했습니다. 아래에 기록한 Parallels 27.0.1의 간헐적 실행 오류이며, 자동 재시도 없이 그대로 보고하는 것이 의도한 동작입니다.

### 이 수행으로 찾아 고친 결함

전부 코드를 읽어서가 아니라 실행해서 찾은 것입니다:

- machine/user 권한 분리를 옮기지 않았습니다. 시스템 전역 설치에는 데스크톱 사용자에게 없는 권한이 필요하므로, 컨트롤러가 `setup-system`을 먼저 상승된 권한으로(Windows는 SYSTEM, Linux는 root) 실행한 뒤 데스크톱 사용자의 작업을 진행합니다. 권한 상승은 실제로 설치할 것이 있을 때만 요구합니다.
- 실행한 Windows 앱이 `prlctl exec`이 만든 job 객체 안에 남아, 컨트롤러가 앱 종료를 기다리며 반환하지 않았습니다. 이제 셸에 넘겨 실행합니다. 사람이 더블클릭하는 것과 같고, PowerShell의 `Start-Process`가 가던 경로입니다. Linux는 `setsid`로 같은 곳에 도달합니다.
- `/`를 포함한 문자열을 이어 붙여 경로를 만들어 Windows에서 구분자가 섞였습니다. 셸이 그런 경로를 실행하지 못했고 실행 중 프로세스 매칭도 같지 않다고 판정했습니다.
- Windows의 Tauri는 `--bundles none`을 받지 않습니다. 개발 빌드는 `--no-bundle`을 씁니다.
- `prlctl exec`은 인자 인용을 보존하지 않고 이어 붙인 뒤 게스트가 재파싱하므로, 여러 문장으로 된 스크립트가 첫 `;`에서 쪼개졌습니다. 이제 게스트의 각 단계는 단일 명령입니다.
- 게스트 빌드가 `.git`을 포함한 공유 전체를 1.7GB tmpfs로 복사해 소진시켰고, 복사가 공유의 읽기 전용 권한까지 보존해 지울 수 없는 파일을 남겼습니다.
- 레지스트리의 `ProductName`은 Windows 11에서도 "Windows 10"으로 남아 있어 진단이 잘못된 시스템을 보고했습니다. 이제 빌드 번호로 판정합니다.

### 앱 번들

`cargo xtask build`가 `Build Machine.app`을 만들었습니다. macOS 워커를 `Contents/MacOS`에 사이드카로(`lipo -archs`: `x86_64 arm64`), Windows·Ubuntu·macOS 워커 3개와 `machine.json`을 리소스로 싣습니다.

- 앱이 실행돼 대시보드를 네이티브로 렌더링했고, 기록된 실행과 플랫폼, 단계별 출력을 표시했습니다.
- 워크스페이스 밖으로 복사해 실행하니 번들 페이로드를 `~/Library/Application Support/local.buildmachine.desktop/machine`에 배치했습니다. 머신 정의와 워커 3개입니다. 번들은 쓸 수 없고 가상 머신에 공유할 수도 없기 때문입니다.
- 이어서 컨트롤러가 그 디렉터리를 root로, 번들 워커만으로 `doctor --os macos`를 통과했습니다. 릴리즈된 앱은 이 체크아웃이 필요 없습니다.

여기서 결함 두 개를 찾아 고쳤습니다. 번들 페이로드를 싣기만 하고 쓰지 않아 앱이 `~/Work/build-machine`을 추측으로 찾고 있었고, `xtask`가 툴체인을 자식에게 넘기지 않아 `cargo`를 셸 아웃하는 Tauri CLI가 실패했습니다. `universal-apple-darwin`은 rustc가 빌드하는 타깃이 아니라 Tauri의 번들 타깃이므로, macOS 워커는 릴리즈 러너처럼 아키텍처별로 빌드해 합칩니다.

제거된 Python 구현이 남긴 실행 기록 15건을 삭제했습니다. 현재 판독기가 파싱할 수 없어 새로고침마다 읽지 못했다고 보고했고, 그것을 위한 호환 경로는 두지 않으며, 그 실행들의 기록은 `verification.ko.md`에 있습니다.

### 워크플로 재현

`ADOPTING.ko.md`를 프로젝트가 따를 가이드로 작성했고, AIRDATA를 그 첫 적용 대상으로 삼았습니다. 이 프로젝트의 릴리즈 workflow는 한 번도 돌리지 않는 Rust 테스트 38개를 갖고 있었고 smoke 단계가 없어 검증에서 거부됐습니다. 이미 있던 테스트를 실행하는 단계를 추가하고, 호스티드 러너가 할 수 없는 smoke 단계에 이유를 남겨 계약을 충족했습니다.

`build-machine ci run ~/Work/airdata --workflow .github/workflows/release-macos.yml --os macos`가 `passed_with_limits`로 완료됐습니다:

- `setup` 제한 포함 — checkout·pnpm·Node.js·Rust·캐시를 로컬 어댑터로, `pnpm install --frozen-lockfile`은 실제 실행, 서명 단계는 조건이 비밀에 의존해 건너뜀.
- `test` 통과 — Rust 테스트 38개와 프런트엔드 타입 검사를 스냅샷 안에서 실행.
- `build` 통과 — `tauri-apps/tauri-action`이 `data_0.1.0_universal.dmg`를 만들었고 SHA-256과 크기를 기록.
- `smoke` 제한 포함 — workflow 주석의 이유를 그대로 담음.
- `signing: unverified`이며, 서명·공증·업로드가 없었다는 것이 제한에 기록됨.

이 실행으로 결함 두 개를 찾아 고쳤습니다. `adapter` 필드에 `#[serde(skip)]`이 걸려 있어 모든 단계가 셸 단계로 워커에 도착했고 첫 단계가 존재하지 않는 명령에서 실패했습니다. 이제 직렬화되며, 모든 어댑터가 요청 문서를 건너 살아남는지 테스트가 확인합니다. 그리고 workflow의 `args`가 이미 `--target`을 지정했는데 머신이 자기 것을 또 붙였습니다. 이제 workflow의 인자가 우선합니다.

같은 workflow를 Ubuntu에서 재현하자 거기서만 실패하는 테스트가 나왔습니다. 글이 반영되기를 기다리지 않고 `items[0]`을 읽고 있었고, macOS에서는 순전히 속도 덕에 통과하고 있었습니다. 같은 파일에 이미 기다리는 패턴이 있었습니다. 고친 뒤 두 곳 모두에서 38개가 통과합니다.

그 실행은 머신이 엉뚱한 자리에서 실패한다는 것도 드러냈습니다. workflow의 `runs-on`은 `macos-14`이고 `args`는 `universal-apple-darwin`을 지정하므로 Ubuntu 재현은 애초에 성립하지 않는데, 거부가 빌드 깊숙한 `rustup`에서 나왔습니다. 이제 workflow가 쓰이지 않은 플랫폼에서의 재현은 환경에 손대기 전에 거부하며, `ci validate`가 재현 가능한 플랫폼을 보고합니다.

AIRDATA의 workflow와 테스트 변경은 그 저장소의 작업 트리에 있으며 커밋하지 않았습니다.

### 수행하지 않은 것

- Windows·Ubuntu에서의 `ci run`. macOS에서만 재현했습니다.
- `release`와 설치 프로그램·패키지 인수 확인.
- 릴리즈 워크플로. 한 번도 실행되지 않았습니다.
- 심어진 앱 디렉터리에서 게스트를 구동하는 것. 이름이 같은 Parallels 공유는 한 디렉터리만 가질 수 있어, 옮기면 이 체크아웃에서 공유를 빼앗게 됩니다.
- Ubuntu의 화면 렌더링. 프로세스 실행과 재사용은 확인했지만 게스트 화면이 잠겨 있어 창 자체는 보지 못했습니다.
- 실제로 무언가를 설치하는 프로비저닝. 모든 환경이 이미 `machine.json`을 만족해 MSVC·WebView2·apt·관리 툴체인 설치 경로는 "설치 없음"만 보고하고 실행되지 않았습니다.

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
