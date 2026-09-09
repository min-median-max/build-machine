import { test, expect, type Page } from '@playwright/test';

async function desktopMock(page: Page, missingController = false) {
  await page.addInitScript(({ missingController }) => {
    const w = window as any;
    const callbacks = new Map<number, (value: unknown) => void>();
    let nextId = 0;
    w.desktopCalls = [];
    w.failJob = false;
    w.__TAURI_INTERNALS__ = {
      transformCallback: (callback: (value: unknown) => void) => { callbacks.set(++nextId, callback); return nextId; },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any) => {
        w.desktopCalls.push({ command, args: command === 'start_job' ? args.request : args });
        if (command === 'load_preferences') return { controllerPath: missingController ? '/fixtures/missing' : '/fixtures/build-machine', projectPath: '/fixtures/airdata', platforms: ['linux'] };
        if (command === 'save_preferences' || command === 'open_log_folder') return null;
        if (command === 'plugin:dialog|open') return args.options.title.includes('빌드 도구') ? '/fixtures/build-machine' : "/fixtures/project with ' spaces";
        if (command === 'get_overview') {
          if (args.controllerPath === '/fixtures/missing') throw new Error('빌드 도구 폴더를 찾을 수 없어요.');
          return { controllerPath: args.controllerPath, warning: null, recentProjects: ['/fixtures/airdata'], toolStatus: JSON.parse(sessionStorage.getItem('fixture-tool-status') || '{}'), environments: [
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
  }, { missingController });
}

test('project selection and multiple platforms reach the shared command, with streamed output', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'Ubuntu 선택' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByText('아직 검사하지 않았어요', { exact: true })).toHaveCount(3);
  await page.screenshot({ path: '../.state/gui-browser-initial.png' });
  await page.getByRole('button', { name: '폴더 선택', exact: true }).click();
  await expect(page.locator('.project-details strong')).toHaveText("project with ' spaces");
  await page.getByRole('button', { name: 'Windows 선택' }).click();
  await page.getByRole('checkbox', { name: '빌드 후 실행' }).check();
  await page.getByRole('button', { name: '빌드 시작' }).click();
  await expect(page.getByRole('button', { name: '환경 진단', exact: true })).toBeDisabled();
  await expect(page.getByText('Checking declared tools from the shared controller')).toBeVisible();
  await expect(page.getByRole('status')).toContainText('빌드 완료');
  const requests = await page.evaluate(() => (window as any).desktopCalls.filter((call: any) => call.command === 'start_job'));
  expect(requests).toHaveLength(1);
  expect(requests[0].args.projectPath).toBe("/fixtures/project with ' spaces");
  expect(requests[0].args.platforms).toEqual(['linux', 'windows']);
  expect(requests[0].args.launch).toBe(true);
  await expect(page.getByText('REUSED BUILD: verified test artifact')).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(1240);
});

test('failed diagnosis remains a failure and restores the controls', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await page.evaluate(() => { (window as any).failJob = true; });
  await page.getByRole('button', { name: '환경 진단', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('환경 진단 실패');
  await expect(page.getByRole('alert')).toContainText('missing test compiler');
  await expect(page.locator('.log-footer')).toBeInViewport({ ratio: 1 });
  await expect(page.getByRole('button', { name: 'Ubuntu 선택' })).toContainText('환경 진단 실패');
  await expect(page.getByRole('button', { name: '환경 진단', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: '로그 폴더 열기' }).click();
  expect(await page.evaluate(() => (window as any).desktopCalls.some((call: any) => call.command === 'open_log_folder'))).toBe(true);
  await page.reload();
  await expect(page.getByRole('button', { name: 'Ubuntu 선택' })).toContainText('환경 진단 실패');
});

test('completed setup remains visible with its check time after a build and app reload', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: '도구 준비', exact: true }).click();
  const environment = page.getByRole('button', { name: 'Ubuntu 선택' });
  await expect(environment).toContainText('도구 준비 완료');
  await expect(environment.locator('time')).toHaveAttribute('datetime', '2026-09-09T11:00:00Z');
  await expect(environment).toContainText('마지막 확인');
  await page.evaluate(() => { (window as any).failJob = true; });
  await page.getByRole('button', { name: '빌드 시작' }).click();
  await expect(page.getByRole('status')).toContainText('빌드 실패');
  await expect(environment).toContainText('도구 준비 완료');
  await page.reload();
  await expect(environment).toContainText('도구 준비 완료');
  await expect(environment.locator('time')).toHaveAttribute('datetime', '2026-09-09T11:00:00Z');
  await page.screenshot({ path: '../.state/gui-browser-tool-status.png' });
});

test('no selected platform disables operations', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Ubuntu 선택' }).click();
  await expect(page.getByRole('button', { name: '환경 진단', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: '빌드 시작' })).toBeDisabled();
  await expect(page.getByText('0개 선택')).toBeVisible();
});

test('a missing controller can be corrected through the native folder picker', async ({ page }) => {
  await desktopMock(page, true);
  await page.goto('/');
  await expect(page.getByRole('alert')).toContainText('빌드 도구 폴더를 찾을 수 없어요');
  await page.getByRole('button', { name: '빌드 도구 폴더', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Ubuntu 선택' })).toBeVisible();
  await expect(page.getByRole('button', { name: '환경 진단', exact: true })).toBeEnabled();
});


test('a failure before a result file exists shows the actual process error', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await page.evaluate(() => { (window as any).failJob = true; (window as any).noReport = true; });
  await page.getByRole('button', { name: '환경 진단', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('환경 진단 실패');
  await expect(page.getByRole('alert')).toContainText('missing test compiler');
});
