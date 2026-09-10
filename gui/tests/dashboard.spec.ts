import { test, expect } from '@playwright/test';
import { desktopMock } from './desktopMock';

test('dashboard summarizes registered project results and opens project controls and exact logs', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: '프로젝트 추가', exact: true }).click();
  await page.evaluate(() => {
    sessionStorage.setItem('fixture-build-history', JSON.stringify([
      { id: 'failed', project: "/fixtures/project with ' spaces", status: 'failure', action: 'build', platforms: ['windows'], results: { windows: { success: false, error: 'compiler unavailable' } }, recordedAt: 1788954600000, finishedAt: null, log: '/fixtures/failed.log', error: null },
      { id: 'passed', project: '/fixtures/airdata', status: 'success', action: 'build', platforms: ['linux'], results: { linux: { success: true } }, recordedAt: 1788954000000, finishedAt: null, log: '/fixtures/passed.log', error: null },
      { id: 'unregistered', project: '/fixtures/another', status: 'success', action: 'build', platforms: ['macos'], results: { macos: { success: true } }, recordedAt: 1788953000000, finishedAt: null, log: null, error: null },
    ]));
  });
  await page.getByRole('button', { name: '대시보드', exact: true }).click();
  await expect(page.locator('.dashboard-counts>div').first().locator('strong')).toHaveText('2개');
  await expect(page.locator('.count-success strong')).toHaveText('1개');
  await expect(page.locator('.count-failure strong')).toHaveText('1개');
  await expect(page.locator('.history-row')).toHaveCount(2);
  await expect(page.getByRole('button', { name: '환경 진단', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: "project with ' spaces failed 로그 열기", exact: true }).click();
  expect(await page.evaluate(() => (window as any).desktopCalls.find((call: any) => call.command === 'open_build_log').args.path)).toBe('/fixtures/failed.log');
  await page.screenshot({ path: '../.state/gui-browser-dashboard.png' });
  await page.getByRole('button', { name: 'airdata 빌드 화면', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'airdata', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '빌드 시작', exact: true })).toBeEnabled();
});

test('running build remains visible across dashboard navigation and its failed result survives reload', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'airdata 빌드 화면', exact: true }).click();
  await page.evaluate(() => { (window as any).failJob = true; (window as any).jobDelay = 3000; });
  await page.getByRole('button', { name: '빌드 시작', exact: true }).click();
  await page.getByRole('button', { name: '대시보드', exact: true }).click();
  await expect(page.getByRole('region', { name: '현재 작업' })).toContainText('airdata · 빌드 중');
  await expect(page.getByRole('region', { name: '현재 작업' })).toContainText('Ubuntu');
  await expect(page.getByRole('button', { name: '프로젝트 추가', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: '설정', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: '실행 화면', exact: true }).click();
  await expect(page.getByText('Checking declared tools from the shared controller')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Ubuntu 빌드 대상', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: '대시보드', exact: true }).click();
  await expect(page.getByRole('region', { name: '현재 작업' })).toHaveCount(0);
  await expect(page.locator('.count-failure strong')).toHaveText('1개');
  await page.reload();
  await expect(page.getByRole('heading', { name: '대시보드', exact: true })).toBeVisible();
  await expect(page.locator('.count-failure strong')).toHaveText('1개');
  await expect(page.locator('.count-success strong')).toHaveText('0개');
});

test('empty and unreadable history are shown without invented successful builds', async ({ page }) => {
  await desktopMock(page, { noProjects: true });
  await page.goto('/');
  await expect(page.getByText('첫 프로젝트를 등록해주세요', { exact: true })).toBeVisible();
  await expect(page.locator('.count-success strong')).toHaveText('0개');
  await page.getByRole('button', { name: '프로젝트 추가', exact: true }).click();
  await page.getByRole('button', { name: '대시보드', exact: true }).click();
  await expect(page.getByText('기록 없음', { exact: true })).toBeVisible();
  await page.evaluate(() => { (window as any).historyWarning = '빌드 기록을 읽지 못했어요. 표시된 결과가 최신이 아닐 수 있어요.'; });
  await page.getByRole('button', { name: '빌드 기록 새로고침', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('최신이 아닐 수');
  await page.evaluate(() => { (window as any).failHistory = true; });
  await page.getByRole('button', { name: '빌드 기록 새로고침', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('test history cannot be read');
  await expect(page.locator('.count-success strong')).toHaveText('—개');
});

test('workflow replay records are labelled and expose each step output', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: '프로젝트 추가', exact: true }).click();
  await page.evaluate(() => {
    sessionStorage.setItem('fixture-build-history', JSON.stringify([
      {
        id: '20260101-120000-000000', project: "/fixtures/project with ' spaces", status: 'passed_with_limits',
        action: 'ci', platforms: ['linux'], recordedAt: 1788954600000, finishedAt: null,
        log: '/fixtures/replay.log', error: null,
        source: { revision: 'abc123def456789', dirty: false, workflowPath: '.github/workflows/release.yml', event: 'workflow_dispatch' },
        results: {
          linux: {
            success: true, status: 'passed_with_limits', finishedAt: '2026-01-01T12:10:00Z',
            log: '/fixtures/replay-linux.log', limits: ['Local CI never signs, notarizes or uploads to GitHub.'],
            stages: {
              setup: { status: 'passed', steps: [{ index: 1, name: 'actions/checkout@v4', adapter: 'checkout', status: 'passed', localAdapter: true }] },
              test: { status: 'passed', steps: [{ index: 2, name: 'unit tests', adapter: 'run', status: 'passed', exitCode: 0, command: 'pnpm test', output: 'Ran 12 tests\nOK\n' }] },
            },
          },
        },
      },
    ]));
  });
  await page.getByRole('button', { name: '대시보드', exact: true }).click();
  await expect(page.locator('.replay-badge')).toHaveText('워크플로 재현');
  await expect(page.locator('.history-platforms')).toContainText('.github/workflows/release.yml');
  await expect(page.locator('.step-name').first()).toBeHidden();
  await page.locator('.history-detail > summary').click();
  await expect(page.locator('.step-name').first()).toHaveText('actions/checkout@v4');
  await expect(page.locator('.step-output')).toBeHidden();
  await page.locator('.step-row > summary').click();
  await expect(page.locator('.step-output')).toBeVisible();
  await expect(page.locator('.step-output')).toContainText('Ran 12 tests');
  await expect(page.locator('.step-command')).toContainText('pnpm test');
  await expect(page.locator('.step-limits li')).toContainText('never signs');
  await page.screenshot({ path: '../.state/gui-browser-replay.png' });
});
