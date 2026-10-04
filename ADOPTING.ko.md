# 세 운영체제에서 빌드되는 프로젝트로 만들기

[English](ADOPTING.md) · [머신이 지원하는 범위](PROJECTS.ko.md)

이 머신이 당신의 저장소를 Windows·Ubuntu·macOS에서 빌드하고, 태그를 붙이기 전에
릴리즈를 리허설할 수 있게 하려면 무엇을 바꿔야 하는지 정리한 문서입니다.

이 머신에만 해당하는 요구는 없습니다. 세 운영체제로 출시한다고 말하는 프로젝트라면
어차피 답할 수 있어야 하는 것들입니다 — 어느 커밋을 빌드했는지, 테스트가 돌았는지,
앱이 실제로 뜨는지. 머신은 그 답이 없을 때 추측하기를 거부할 뿐입니다.

체크리스트를 따라간 뒤 `build-machine ci validate`를 실행하세요. 아직 충족하지 않은
항목을 정확히 알려줍니다.

---

## 1. 단위는 저장소입니다

- **Git 저장소 루트를 등록합니다.** 하위 디렉터리가 아닙니다. 모노레포의 하위 앱은
  workflow 단계의 `working-directory`로 선택합니다.
- **잠금 파일을 커밋하세요.** `pnpm-lock.yaml` 또는 `package-lock.json`, Tauri라면
  `src-tauri/Cargo.lock`. 여기서의 빌드는 `--frozen-lockfile`과 `--locked`입니다.
  잠금이 어긋나면 새로 해석하지 않고 실패합니다.
- **프로젝트 밖을 가리키는 심볼릭 링크와 서브모듈은 안 됩니다.** 스냅샷은 추적 파일과
  무시되지 않은 미추적 파일을, 커밋하지 않은 수정까지 포함해 복사합니다. 바깥으로
  나가는 것은 조용히 따라가지 않고 거부합니다.
- **기계에만 있는 비밀은 추적 파일에 두지 마세요.** 무시된 파일은 복사되지 않으며,
  그래서 빌드가 그런 파일에 의존해서도 안 됩니다.

## 2. 빌드는 사람 없이 끝나야 합니다

- 대화형 프롬프트, 개발 서버, 특정 개발자 기계의 절대 경로가 없어야 합니다.
- 프레임워크의 프로덕션 빌드를 쓰세요. Tauri라면 `tauri build`이고, 머신이
  `--ci --no-sign --locked`와 타깃을 붙입니다.
- 플랫폼당 **하나**의 식별 가능한 실행 파일을 만들거나 `--artifact`로 지목하세요.
  출력 디렉터리에 후보가 둘이면 임의로 고르지 않고 오류입니다.

## 3. 선언된 도구 버전에 맞추세요

머신은 [machine.json](machine.json)이 선언한 것만 설치하고 PATH 앞에 둡니다.
`build`는 `.nvmrc`나 `rust-toolchain.toml`을 읽지 않습니다.

workflow 재현은 다릅니다. `actions/setup-node`, `actions/setup-go`,
`shivammathur/setup-php`는 그 단계가 선언한 릴리스 — `node-version`, `go-version`,
`php-version` 또는 대응하는 `*-version-file` — 를 설치하고 이후 단계의 PATH 앞에
둡니다. 아카이브는 배포처(nodejs.org, go.dev, getcomposer.org)가 공개한 체크섬과
대조합니다. PHP는 hosted Ubuntu runner에서 setup-php가 가져오는 곳에서 가져옵니다. 머신에 이미
있는 릴리스는 그것으로 전환하고, 그 밖의 릴리스는 이 Ubuntu 버전과 아키텍처용
setup-php 캐시 빌드(shivammathur/php-ubuntu)를 GitHub가 공개한 sha256과 대조한 뒤
runner처럼 비밀번호 없는 `sudo`로 설치합니다. 그런 빌드가 없는 릴리스나 빌드에 없는
선언 확장은 이유와 함께 그 단계를 실패시키고, 다른 것으로 대신하지 않습니다. 그
릴리스는 `php`가 되기 전에 선언한 확장과 함께 실행되어야 하며, 실행되지 않으면 머신에
없는 라이브러리를 밝혀 단계를 실패시키고 이전 선택을 그대로 둡니다. `coverage: none`은
setup-php처럼 그 릴리스의 모든 SAPI에서 Xdebug와 PCOV를 끄고, `xdebug`(`xdebug3`)나
`pcov`는 그 드라이버를 켜고 다른 하나를 끕니다. 단계가 끝난 뒤 그 릴리스의 `php -m`이
이와 맞아야 하며, 맞지 않으면 그 드라이버를 밝혀 단계를 실패시킵니다. 그 밖의 값은
검증에서 실패합니다.

그 빌드는 runner 이미지에 있는 라이브러리를 불러옵니다. 그래서 [machine.json](machine.json)의
Linux 프로필은 자신이 대신하는 이미지(`image`: `ubuntu-26.04-arm`, 공개된 toolset의 apt
패키지와 PHP 8.5 패키지)를 선언하고, `setup`이 설치하며 `doctor`가 확인합니다. 머신이
제공하지 않는 이미지 내용은 거기에 적고, 그 runner의 job을 재현할 때마다 제한으로
기록합니다. 이미지의 `/etc/environment`도 거기에 선언합니다. 단계는 머신이 관리하는 도구
디렉터리 뒤에 이미지의 `PATH`(`/usr/sbin`, `/sbin` 포함)를 두고, 이미지처럼
`DEBIAN_FRONTEND=noninteractive`, `ACCEPT_EULA=Y`, `XDG_CONFIG_HOME`과 함께 실행합니다. 재현은 Rust도 고정하지 않습니다. runner에서처럼 workflow의
`rust-toolchain.toml`이나 rustup 호출이 고릅니다.

선언된 Node.js·pnpm·Rust·Go에서 프로젝트가 동작하게 하거나, `machine.json`을 바꾸고
그 이유를 남기세요. `build-machine doctor`가 실제 설치된 것과 선언된 것을 비교해
보고합니다.

## 4. 플랫폼마다 필요한 것을 감안하세요

| 플랫폼 | 프로젝트가 견뎌야 하는 것 |
| --- | --- |
| Windows | WebView2, MSVC, ARM64. `run:` 블록은 PowerShell에서 실행되며 마지막 줄의 종료 코드만 보고합니다. |
| Ubuntu | WebKitGTK와 `machine.json`의 `packages`에 있는 시스템 라이브러리. |
| macOS | 유니버설 빌드는 두 아키텍처를 모두 담아야 합니다. 머신이 `lipo`로 확인하고 하나가 없으면 실패합니다. |

의도한 OS용으로 컴파일하는 것은 당신 코드의 몫입니다. 머신은 선언된 사전 요구사항을
설치할 뿐, 소스에서 임의의 네이티브 라이브러리를 추론하지 않습니다.

### 한 스텝에 한 명령을 둡니다

`run:` 블록은 러너마다 다른 셸에 전달되고, 그 셸들은 실패가 무엇인지에 합의하지
않습니다. Linux와 macOS의 `bash -e`는 실패한 첫 명령에서 멈춥니다. Windows의
PowerShell은 모든 줄을 실행하고 마지막 종료 코드만 보고하므로, 타입 검사가 실패하고
테스트가 통과하면 그 잡은 통과합니다.

```yaml
      # Windows는 마지막 줄의 종료 코드만 보고하므로, 그 위의 검사는 실패해도
      # 잡을 실패시키지 못합니다.
      - name: Test the frontend types and the Rust core
        run: |
          pnpm exec tsc --noEmit
          cargo test --locked --manifest-path src-tauri/Cargo.toml

      # 스텝당 명령 하나면 어디서든 같은 방식으로 실패합니다.
      - name: Check the frontend types
        run: pnpm exec tsc --noEmit

      - name: Test the Rust core
        run: cargo test --locked --manifest-path src-tauri/Cargo.toml
```

스텝을 나누면 리포트가 어느 쪽이 실패했는지 말해 줍니다. 한 블록의 출력을 읽어
가려낼 필요가 없습니다.

## 5. Workflow가 계약입니다

`ci validate`와 `ci run`은 `.github/workflows/*.yml` 하나를 읽고, 이 머신이 실제로
수행할 수 있는 부분을 재현합니다. 그 밖의 것은 조용히 다른 것으로 바꾸지 않고 검증에서
실패합니다.

**지원하는 action** — `actions/checkout`, `pnpm/action-setup`,
`actions/setup-node`, `actions/setup-go`, `shivammathur/setup-php`,
`dtolnay/rust-toolchain`, `swatinem/rust-cache`, `actions/cache`,
`tauri-apps/tauri-action`, `actions/upload-artifact`,
`actions/download-artifact`, `actions/upload-pages-artifact`,
`actions/deploy-pages`, `softprops/action-gh-release`. action의 owner와 이름은 GitHub처럼
대소문자를 구분하지 않습니다. upload-artifact는 `path`의 파일(한 줄에 상대 경로 하나, 가장 가까운
공통 상위 폴더 아래 배치)을 이 Mac의 run의 artifact `name`으로 보관하며, 아무것도 맞지 않는
경로는 action의 기본 경고처럼 파일을 더하지 않습니다. download-artifact는 run이나 `run-id`가
가리키는 이전 재현의 artifact `name`을 `path`에 둡니다. GitHub에는 아무것도 보내지 않고 각각
제한으로 기록합니다. upload의 `name`, `path`, `retention-days`, download의 `name`, `path`,
`run-id`, `github-token` 외의 입력은 검증에서 실패하고, `name` 없는 download는 실패합니다. setup action의 `with` 입력 중 어댑터가 반영하지 않는 것은 검증에서 실패하며,
pages action도 업로드의 `path`, `name`, `retention-days`와 배포의 `artifact_name` 외의
입력은 실패합니다. 업로드는 단계가 실행될 때의 사이트를 재현이 끝날 때까지 보관합니다.
deploy-pages는 아무것도 배포하지 않고 그 산출물의 파일을 크기, SHA-256과 함께 결과의
`deployments`에 dry run으로 기록하며, 재현에서 앞서 업로드한 pages 산출물이 없으면
실패합니다. action-gh-release는 아무것도 게시하지 않습니다. tag는 재현한 tag나
`tag_name`에서 가져오고, 둘 다 없는 branch에서는 action처럼 실패합니다. `files`의 각 줄이
맞는 파일을 크기, SHA-256과 함께, `body_path` 파일과 같이 단계 출력에 dry run으로
기록합니다. `files` 패턴은 한 경로 조각 안의 `*`와 `?`만 쓰고, 다른 glob 형식은 단계를
실패시키며, 아무 파일에도 맞지 않는 패턴은 action이 경고하듯 출력에 밝힙니다.
`files`, `body_path`, `tag_name` 외의 입력은 검증에서 실패합니다. job의 `environment`(식 없는 이름, 또는 이름에는 식이 없고 url은 job 단계의 출력을 읽을 수 있는 `{name, url}`, url은 쓰인 그대로 기록)는 제한으로 기록하며
로컬에는 영향이 없습니다.

셸 `run` 단계는 쓰인 그대로, runner처럼 Linux와 macOS에서는 `bash -e`로,
Windows에서는 PowerShell로 실행합니다. 단계는 workflow 순서대로 실행합니다. 각 단계는
`$GITHUB_ENV`로 변수를, `$GITHUB_PATH`로 PATH를 이후 단계에 넘길 수 있고,
`GITHUB_WORKSPACE`, `RUNNER_TEMP`, `RUNNER_OS`, `RUNNER_ARCH`, `GITHUB_SHA`,
`GITHUB_REF`, `GITHUB_REF_NAME`, `GITHUB_EVENT_NAME`, `GITHUB_JOB`이 설정됩니다.

**Checkout** — 모든 job은 runner처럼 빈 `GITHUB_WORKSPACE`
(runner가 `/home/runner/work/<저장소>/<저장소>`에 checkout하듯
`$HOME/work/<저장소>/<저장소>`, `RUNNER_WORKSPACE`는 `$HOME/work/<저장소>`, `RUNNER_TEMP`는
`$HOME/work/_temp` 아래)에서 시작하고, `actions/checkout`이 그 action의 명령으로
재현하는 revision의 Git 저장소를 만듭니다. 기본은 commit 하나, `fetch-depth: 0`은 모든
branch와 tag이며, 재현하는 branch나 tag를 checkout합니다(commit은 detached로 checkout).
기록은 Git bundle로 보낸 이 저장소 자신의 branch, tag, `HEAD`이고 `origin`인 로컬
mirror를 거쳐 가져옵니다. GitHub remote가 아니라는 점은 제한으로 기록합니다. 작업 트리를
재현하면 커밋하지 않은 수정·삭제·추적하지 않는 파일을 재현하는 commit 위에 stage해 두므로
`git ls-files`와 `git grep`은 그것을 보고 `git log`는 실제 commit만 봅니다. 이것도
제한으로 기록합니다. `path`는 workspace 아래의 상대 폴더에 checkout하며, 그 폴더는
비어 있거나 없어야 합니다. `working-directory`가 없는 단계는 그대로 workspace 루트에서
실행합니다. `repository`는 `machine.json`의 `repositories`가 로컬 clone에 대응시킨 다른
저장소를 그 clone의 commit된 기록에서 `ref`(branch, tag 또는 commit SHA, 없으면 clone의
`HEAD`)로 checkout합니다. `${{ github.event.pull_request.head.repo.full_name }}`처럼 표현식으로
준 `repository`는 재현의 `github` 값으로 읽은 뒤에 찾습니다. `repository` 없는 `ref`는 workflow
자신의 저장소를 그 ref로 commit된 기록에서 checkout하며, 작업 트리의 변경은 담지 않습니다.
`submodules`처럼 다른 것을 checkout하게 하는 입력은 검증에서 실패합니다. job이 끝나면 runner처럼 그 job의
`RUNNER_TRACKING_ID`를 가진 프로세스를 종료하고(Linux) workspace를 지웁니다. 결과에
적힌 산출물은 먼저 그 옆으로 옮기고 결과가 옮긴 위치를 가리킵니다.
[machine.json](machine.json) `retention`의 byte 상한은 재현이 끝난 뒤뿐 아니라 시작하기
전에도 적용합니다.

**Job** — job에는 `name`, `runs-on`, `needs`, `env`, `steps`, `timeout-minutes`,
`environment`, `if`, `permissions`(재현에는 없는 GitHub token에만 영향)를, workflow에는
`name`, `run-name`, `on`, `env`, `jobs`, `permissions`, `concurrency`를 쓸 수 있습니다.
workflow `env`는 job과 단계의 `env` 아래에서 모든 단계에 전달됩니다. job마다
`$GITHUB_ENV`, `$GITHUB_PATH`, 상태를 따로 둡니다. job은 GitHub처럼 `if:`로 실행 여부를
정합니다. `success()`는 앞선 모든 job(`needs`의 job과 그 job들의 `needs`)이 성공했을 때,
`failure()`는 그중 하나가 실패했을 때 참이고, `always()`, `needs`에 있는 job의
`needs.<job>.result`(`success`, `failure`, `cancelled`, `skipped`), 재현이 가진 `github`
값을 읽습니다. `if`가 없으면 `success()`입니다. 실행하지 않은 job은 이유와 함께
skipped로 기록하고, 그 job을 기다리는 job은 `skipped`를 봅니다. job의 `if`에 쓴
`needs.<job>.outputs`, 단계 출력과 그 밖의 context는 검증에서 실패합니다. 운영체제마다 따로 재현하므로 다른 운영체제의 job을
`needs`로 기다릴 수 없습니다.

**재사용 workflow** — `uses: ./.github/workflows/<file>`(그리고 `name`, `needs`,
`permissions`만)을 쓴 job은 그 자리에서 같은 저장소의 그 workflow의 job을 실행합니다. 그
workflow는 `on: workflow_call`을 선언해야 합니다. 그 job들은 `<부르는 job>/<불린 job>`으로
이름 붙고, 그중 처음 job은 부르는 job의 `needs`를 기다리며, 부르는 job을 기다리는 job은 그
job 모두를 기다리고, 부르는 쪽의 `github` 값을 봅니다. ref 재현은 불린 workflow를 그 ref에서
읽습니다. 부르는 job의 `with`, `secrets`, `if`, 다른 저장소의 workflow, 4단계를 넘는 중첩은
검증에서 실패합니다.

**시간 제한** — job은 자신의 `timeout-minutes`, 선언이 없으면 GitHub의 360분 동안
실행하고, 단계는 job에 남은 시간 안에서 자신의 `timeout-minutes`만큼 실행합니다. 그
밖의 제한은 없습니다. 자신의 제한을 넘은 단계는 실패하고, 제한을 넘은 job은 GitHub처럼
취소되어 그 뒤로는 취소 뒤에도 실행하는 `if:`(`always()`, `cancelled()`)의 단계만
실행합니다. 단계는 프로세스가 끝나면 끝납니다. 백그라운드 프로세스가 아직 잡고 있는
출력은 GitHub runner처럼 5초 더 읽고, 그 프로세스는 그대로 둡니다.

**지원하지 않는 것** — 컨테이너, 서비스, 재사용 workflow, `strategy`, 그 밖의 job 키
(`continue-on-error`, `defaults`, `outputs` 등), workflow
`defaults`, 어댑터가 없는 action, 단계 키 `shell`, `continue-on-error`. 검증에서
실패합니다.

**표현식** — `if:`에는 `${{ }}` 안이든 밖이든 `success()`, `failure()`,
`always()`, `cancelled()`, 리터럴, `!`, `&&`, `||`, `==`, `!=`, 괄호를 쓸 수 있습니다.
어떤 단계가 실패하면 그 뒤 단계는 runner에서처럼 `if:`가 허용할 때만 실행하므로,
`if: ${{ !cancelled() }}`는 계속 결과를 보고합니다.

`id`가 있는 `run` 단계는 `$GITHUB_OUTPUT`으로 출력을 정하고, 같은 job의 이후 단계는
`if:`, `run:`, `env`, `working-directory`, 어댑터가 읽는 `with` 입력에서
`${{ steps.<id>.outputs.<name> }}`으로 읽습니다. 검증은 그 단계가 job 안에서 앞에
있는지 확인하고, 값은 단계 실행 직전에 채웁니다. 단계가 쓰지 않은 출력은 GitHub처럼
빈 값입니다. 같은 곳에서 `github.event_name`(재현의 event), `github.sha`(재현한 revision),
`github.ref`와 `github.ref_name`(재현이 checkout하는 branch나 tag: `--ref`의 branch나 tag,
또는 작업 트리의 현재 branch)을 읽습니다. branch나 tag 이름이 없는 commit의 재현은
`github.ref`나 `github.ref_name`을 읽는 workflow를 거부합니다. 또한 GitHub이 event를 보내는
형태의 JSON 객체를 `--event-payload <file>`로 주면 그 event payload에서 `github.event.<path>`를
읽습니다. 단계는 그 파일을 `GITHUB_EVENT_PATH`로 받고, payload가 없거나 workflow가 읽는 경로를
담지 않으면 검증에서 실패합니다. 그 밖의 context를 읽는 표현식은 검증에서 실패합니다. 비밀에 의존하는 조건은 거짓으로 두고 제한으로 기록하므로, 서명
단계는 어중간하게 시도되지 않고 건너뜁니다.

### 세 개의 게이트

workflow에는 **build**, **test**, **smoke**가 있거나 없는 이유를 밝혀야 합니다:

```yaml
# build-machine: skip smoke reason=데스크톱 실행은 로그인된 세션에서 따로 확인합니다
```

이유는 플랫폼 결과에 저장됩니다. 게이트의 요점이 이것입니다 — 빠진 검사가 아무도
눈치채지 못한 누락이 아니라 기록에 남은 결정이 됩니다.

건너뛰기보다 실제 단계를 두세요. 저장소에 이미 테스트가 있다면 그걸 실행하는 것으로
대개 끝납니다.

### 단계가 스테이지에 배정되는 방식

지원 action을 쓰는 단계는 그 어댑터가 배정합니다. checkout, pnpm/Node/Rust 준비와
캐시는 `setup`, Tauri action은 `build`, 산출물 업로드와 릴리스는 `release`입니다.
셸 `run` 단계만 자기 이름과 명령으로 읽으며, `smoke`/`launch`/`health`/`e2e`,
그다음 `test`/`lint`/`check`/`verify`, 그다음 `build`/`package`/`compile` 순서입니다.

그러니 셸 단계 이름은 하는 일대로 지으세요. action의 이름은 절대 세지 않습니다 —
`actions/checkout`에 "check"가 들어 있지만 test 스테이지가 되지 않습니다.

스테이지는 단계를 보고하는 방식이지 실행 시점이 아닙니다. 단계는 workflow에 쓰인
순서대로 실행하므로, 앞 단계가 만든 것을 읽는 단계 — `make test-servers` 다음에 그것이
쓴 환경 파일을 읽는 단계 — 가 그것을 찾습니다.

### 한 플랫폼을 못박은 workflow는 그곳에서만 재현됩니다

`runs-on`이 그 job이 어디서 돌도록 쓰였는지 말하고, `--target universal-apple-darwin`
같은 인자가 한 번 더 말합니다. 그런 workflow를 다른 곳에서 재현하는 것은 시작 전에
거부합니다:

```
이 워크플로는 macos 에서 실행되도록 작성됐어요. linux에서는 재현할 수 없습니다.
```

`ci validate`가 어느 플랫폼에서 재현 가능한지 보고하므로 실행하지 않고도 알 수 있습니다.

세 운영체제를 모두 리허설하려면 **타깃을 직접 적지 마세요.** 머신이 빌드하는 플랫폼의
타깃을 `machine.json`에서 가져와 붙입니다. 그 한 플랫폼만을 뜻할 때만 직접 적으세요.

### 재현이 증명하지 않는 것

서명, 공증, 산출물 업로드, 릴리스 게시는 로컬 어댑터로 대체하고 제한으로 기록합니다.
성공한 재현은 `passed_with_limits`이며, 실제 릴리스가 게시된다는 증거가 아닙니다.

---

## 체크리스트

```
[ ] 등록하는 것은 Git 저장소 루트
[ ] 잠금 파일이 커밋돼 있음
[ ] 외부 심볼릭 링크와 서브모듈 없음
[ ] 프롬프트와 개발자별 경로 없이 빌드가 끝남
[ ] 플랫폼당 식별 가능한 실행 파일 하나, 또는 --artifact로 지목
[ ] machine.json이 선언한 버전에서 프로젝트가 동작함
[ ] workflow가 지원 action만 사용, matrix·컨테이너 없음
[ ] build 스테이지가 있음
[ ] test 스테이지가 있거나, 없는 이유가 skip 주석에 있음
[ ] smoke 스테이지가 있거나, 없는 이유가 skip 주석에 있음
[ ] build-machine ci validate 통과
```

---

## 적용 예시

AIRDATA는 macOS 앱을 빌드해 게시하는 릴리즈 workflow를 가진 Tauri 2 앱입니다.
Rust 테스트 38개를 갖고 있었지만 workflow가 한 번도 돌리지 않았고, smoke 단계도
없었습니다. 검증이 거부했습니다:

```
ERROR: test 단계가 없어요. 워크플로에 '# build-machine: skip test reason=...' 주석을 추가해야 해요.
```

두 가지 변경으로 계약을 충족했습니다. 테스트는 이미 있었으므로 실행만 시키면 됐습니다:

```yaml
      - name: Test the frontend types and the Rust core
        run: |
          pnpm exec tsc --noEmit
          cargo test --locked --manifest-path src-tauri/Cargo.toml
```

그리고 호스티드 러너가 실제로 할 수 없는 단계에는 이유를 남겼습니다:

```yaml
# build-machine: skip smoke reason=the desktop launch is verified by `build-machine run` against a signed-in desktop session, which a hosted runner does not have
```

그러자 재현이 끝까지 돌았습니다:

```
전체: passed_with_limits
  setup    passed_with_limits   checkout · pnpm · node · rust · cache · install · 서명(건너뜀)
  test     passed               Rust 테스트 38개와 프런트엔드 타입 검사
  build    passed               tauri-action → data_0.1.0_universal.dmg
  smoke    passed_with_limits   skip smoke=…
```

같은 workflow를 Ubuntu에서 재현하자 macOS 전용 workflow로는 볼 수 없던 것이 나왔습니다.
테스트 38개 중 하나가 거기서 실패했습니다. 첫 클라이언트의 글이 반영되기를 기다리지
않고 두 번째 클라이언트를 붙여 `items[0]`을 읽고 있었고, 기계가 충분히 빠를 때만
통과하고 있었습니다. 같은 파일에 이미 기다리는 패턴이 있었는데 그 테스트만 쓰지
않았습니다. 고친 뒤 두 곳 모두에서 38개가 통과합니다.

이것이 세 OS 리허설의 요점이고, 동시에 이 workflow를 Ubuntu에서 재현하는 것을 이제
더 일찍 거부하는 이유이기도 합니다. `runs-on`과 `--target universal-apple-darwin`이
둘 다 macOS를 가리킵니다. 세 플랫폼을 위한 workflow라면 타깃을 직접 적어서는 안 됩니다.

이제 태그를 붙이기 전에 테스트가 돌아갑니다. 이 머신이 강요한 형식이 아니라 그
프로젝트 자신의 릴리즈 품질이 달라진 것입니다.
