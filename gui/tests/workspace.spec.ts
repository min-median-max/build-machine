import { test, expect, type Page } from '@playwright/test';

async function desktopMock(page: Page, options: { missingController?: boolean; noProjects?: boolean } = {}) {
  await page.addInitScript(({ missingController, noProjects }) => {
    const w = window as any;
    const callbacks = new Map<number, (value: unknown) => void>();
    let nextId = 0;
    w.desktopCalls = [];
    w.failJob = false;
    const initial = {
      controllerPath: missingController ? '/fixtures/missing' : '/fixtures/build-machine',
      environmentPlatforms: ['linux'],
      projects: noProjects ? [] : [{ path: '/fixtures/airdata', platforms: ['linux'], launch: false }],
      selectedProject: noProjects ? null : '/fixtures/airdata', page: 'projects',
    };
    w.__TAURI_INTERNALS__ = {
      transformCallback: (callback: (value: unknown) => void) => { callbacks.set(++nextId, callback); return nextId; },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any) => {
        w.desktopCalls.push({ command, args: command === 'start_job' ? args.request : args });
        if (command === 'load_preferences') return JSON.parse(sessionStorage.getItem('fixture-preferences') || JSON.stringify(initial));
        if (command === 'save_preferences') { sessionStorage.setItem('fixture-preferences', JSON.stringify(args.preferences)); return null; }
        if (command === 'open_log_folder' || command === 'open_settings_folder') return null;
        if (command === 'plugin:dialog|open') return args.options.title.includes('빌드 도구') ? '/fixtures/build-machine' : "/fixtures/project with ' spaces";
        if (command === 'register_project') {
          const next = args.preferences;
          if (!next.projects.some((project: any) => project.path === args.projectPath)) next.projects.push({ path: args.projectPath, platforms: ['windows', 'linux', 'macos'], launch: false });
          next.selectedProject = args.projectPath; next.page = 'projects';
          sessionStorage.setItem('fixture-preferences', JSON.stringify(next));
          return next;
        }
        if (command === 'get_overview') {
          if (args.controllerPath === '/fixtures/missing') throw new Error('빌드 도구 폴더를 찾을 수 없어요.');
          return { controllerPath: args.controllerPath, warning: null, toolStatus: JSON.parse(sessionStorage.getItem('fixture-tool-status') || '{}'), environments: [
            { id: 'windows', title: 'Windows', vm: 'Windows 11', status: 'running', target: 'aarch64-pc-windows-msvc' },
            { id: 'linux', title: 'Ubuntu', vm: 'Ubuntu 26.04 ARM64', status: 'running', target: 'aarch64-unknown-linux-gnu' },
            { id: 'macos', title: 'macOS', vm: null, status: 'local', target: 'universal-apple-darwin' },
          ] };
        }
        if (command === 'start_job') {
          const send = (index: number, line: string, stream = 'stdout') => callbacks.get(args.onOutput.id)?.({ index, message: { line, stream } });
          send(0, `PLATFORM: ${args.request.platforms[0]}`);
          send(1, 'Checking declared tools from the shared controller');
          await new Promise(resolve => setTimeout(resolve, 650));
          const fail = w.failJob;
          send(2, fail ? 'ERROR: missing test compiler' : 'REUSED BUILD: verified test artifact', fail ? 'stderr' : 'stdout');
          const results = Object.fromEntries(args.request.platforms.map((os: string) => [os, { success: !fail, finishedAt: '2026-09-09T11:00:00Z', ...(fail ? { error: 'missing test compiler' } : {}) }]));
          if (!w.noReport && ['doctor', 'setup'].includes(args.request.action)) {
            const previous = JSON.parse(sessionStorage.getItem('fixture-tool-status') || '{}');
            for (const [os, result] of Object.entries(results)) previous[os] = { ...(result as object), action: args.request.action };
            sessionStorage.setItem('fixture-tool-status', JSON.stringify(previous));
          }
          return { exitCode: fail ? 1 : 0, resultPath: '/fixtures/result.json', result: w.noReport ? null : { log: '/fixtures/build.log', results } };
        }
        throw new Error(`Unexpected desktop call: ${command}`);
      },
    };
  }, options);
}

const navigate = (page: Page, name: string) => name === '환경'
  ? page.getByRole('button', { name: '설정', exact: true }).click()
  : page.locator('.registered-project').filter({ hasText: 'airdata' }).click();

test('environment diagnosis works without a registered project and contains no build controls', async ({ page }) => {
  await desktopMock(page, { noProjects: true });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '프로젝트 등록', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '환경 화면', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '프로젝트 화면', exact: true })).toHaveCount(0);
  await navigate(page, '환경');
  await expect(page.getByRole('heading', { name: '설정', exact: true })).toBeVisible();
  await expect(page.locator('body')).not.toContainText('WORKSPACE');
  await expect(page.getByRole('button', { name: '빌드 시작' })).toHaveCount(0);
  await page.getByRole('button', { name: '환경 진단', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('환경 진단 완료');
  const request = await page.evaluate(() => (window as any).desktopCalls.find((call: any) => call.command === 'start_job').args);
  expect(request.projectPath).toBeNull();
  expect(request.platforms).toEqual(['linux']);
});

test('plus registration and multiple project targets reach the shared build command', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: '프로젝트 추가', exact: true }).click();
  await expect(page.locator('.project-details strong')).toHaveText("project with ' spaces");
  await expect(page.getByRole('button', { name: '환경 진단', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'macOS 빌드 대상' }).click();
  await page.getByRole('checkbox', { name: '빌드 후 실행' }).check();
  await page.getByRole('button', { name: '빌드 시작' }).click();
  await expect(page.getByRole('button', { name: '프로젝트 추가', exact: true })).toBeDisabled();
  await expect(page.getByText('Checking declared tools from the shared controller')).toBeVisible();
  await expect(page.getByRole('status')).toContainText('빌드 완료');
  const request = await page.evaluate(() => (window as any).desktopCalls.find((call: any) => call.command === 'start_job').args);
  expect(request.projectPath).toBe("/fixtures/project with ' spaces");
  expect(request.platforms).toEqual(['windows', 'linux']);
  expect(request.launch).toBe(true);
  await expect(page.locator('.log-footer')).toBeInViewport({ ratio: 1 });
  await page.screenshot({ path: '../.state/gui-browser-projects.png' });
});

test('registered projects retain independent targets and launch options after reload', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await navigate(page, '프로젝트');
  await page.getByRole('button', { name: 'Windows 빌드 대상' }).click();
  await page.getByRole('checkbox', { name: '빌드 후 실행' }).check();
  await page.getByRole('button', { name: '프로젝트 추가', exact: true }).click();
  await page.getByRole('button', { name: 'Ubuntu 빌드 대상' }).click();
  await page.getByRole('button', { name: '프로젝트 추가', exact: true }).click();
  await expect(page.locator('.registered-project')).toHaveCount(2);
  await expect(page.getByRole('button', { name: 'Ubuntu 빌드 대상' })).toHaveAttribute('aria-pressed', 'false');
  await page.locator('.registered-project').filter({ hasText: 'airdata' }).click();
  await expect(page.getByRole('button', { name: 'Windows 빌드 대상' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('checkbox', { name: '빌드 후 실행' })).toBeChecked();
  await page.reload();
  await expect(page.locator('.project-details strong')).toHaveText('airdata');
  await expect(page.getByRole('checkbox', { name: '빌드 후 실행' })).toBeChecked();
  await navigate(page, '환경');
  await expect(page.getByRole('button', { name: 'Windows 선택' })).toHaveAttribute('aria-pressed', 'false');
  await expect(page.getByRole('button', { name: 'Ubuntu 선택' })).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: '설정 폴더 열기', exact: true }).click();
  expect(await page.evaluate(() => (window as any).desktopCalls.some((call: any) => call.command === 'open_settings_folder'))).toBe(true);
});

test('removing a registration persists the empty project list while environment actions remain available', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await navigate(page, '프로젝트');
  await page.getByRole('button', { name: '등록 해제', exact: true }).click();
  await expect(page.getByRole('heading', { name: '프로젝트를 등록해주세요' })).toBeVisible();
  await page.reload();
  await expect(page.locator('.registered-project')).toHaveCount(0);
  await navigate(page, '환경');
  await expect(page.getByRole('button', { name: '도구 준비', exact: true })).toBeEnabled();
});

test('failed diagnosis remains visible after reload and restores the controls', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await navigate(page, '환경');
  await page.evaluate(() => { (window as any).failJob = true; });
  await page.getByRole('button', { name: '환경 진단', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('환경 진단 실패');
  await expect(page.getByRole('alert')).toContainText('missing test compiler');
  await expect(page.locator('.log-footer')).toBeInViewport({ ratio: 1 });
  await page.getByRole('button', { name: '로그 폴더 열기' }).click();
  expect(await page.evaluate(() => (window as any).desktopCalls.some((call: any) => call.command === 'open_log_folder'))).toBe(true);
  await page.reload();
  await expect(page.getByRole('button', { name: 'Ubuntu 선택' })).toContainText('환경 진단 실패');
});

test('shared setup result survives project build failure and app reload with its original time', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await navigate(page, '환경');
  await page.getByRole('button', { name: '도구 준비', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('도구 준비 완료');
  await navigate(page, '프로젝트');
  await page.evaluate(() => { (window as any).failJob = true; });
  await page.getByRole('button', { name: '빌드 시작' }).click();
  await expect(page.getByRole('status')).toContainText('빌드 실패');
  await navigate(page, '환경');
  await page.reload();
  const environment = page.getByRole('button', { name: 'Ubuntu 선택' });
  await expect(environment).toContainText('도구 준비 완료');
  await expect(environment.locator('time')).toHaveAttribute('datetime', '2026-09-09T11:00:00Z');
  await page.screenshot({ path: '../.state/gui-browser-environment.png' });
});

test('empty environment selection does not change a project target selection', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await navigate(page, '환경');
  await page.getByRole('button', { name: 'Ubuntu 선택' }).click();
  await expect(page.getByRole('button', { name: '환경 진단', exact: true })).toBeDisabled();
  await navigate(page, '프로젝트');
  await expect(page.getByRole('button', { name: '빌드 시작' })).toBeEnabled();
  await page.getByRole('button', { name: 'Ubuntu 빌드 대상' }).click();
  await expect(page.getByRole('button', { name: '빌드 시작' })).toBeDisabled();
});

test('a missing controller can be corrected through the native folder picker', async ({ page }) => {
  await desktopMock(page, { missingController: true });
  await page.goto('/');
  await expect(page.getByRole('alert')).toContainText('빌드 도구 폴더를 찾을 수 없어요');
  await navigate(page, '환경');
  await page.getByRole('button', { name: '빌드 도구 폴더', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Ubuntu 선택' })).toBeVisible();
  await expect(page.getByRole('button', { name: '환경 진단', exact: true })).toBeEnabled();
});

test('a failure before a result file exists shows the actual process error', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await navigate(page, '환경');
  await page.evaluate(() => { (window as any).failJob = true; (window as any).noReport = true; });
  await page.getByRole('button', { name: '환경 진단', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('환경 진단 실패');
  await expect(page.getByRole('alert')).toContainText('missing test compiler');
});
