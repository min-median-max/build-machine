import { test, expect, type Page } from '@playwright/test';

import { desktopMock } from './desktopMock';

const navigate = (page: Page, name: string) => name === '환경'
  ? page.getByRole('button', { name: '설정', exact: true }).click()
  : page.locator('.registered-project').filter({ hasText: 'airdata' }).click();

test('environment diagnosis works without a registered project and contains no build controls', async ({ page }) => {
  await desktopMock(page, { noProjects: true });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '대시보드', exact: true })).toBeVisible();
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

test('workflow replay settings pass event, ref and parallel execution to the controller', async ({ page }) => {
  await desktopMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'airdata 빌드 화면', exact: true }).click();
  await page.getByLabel('워크플로 경로').fill('.github/workflows/release.yml');
  await page.getByLabel('워크플로 이벤트').selectOption('push');
  await page.getByLabel('워크플로 ref').fill('v0.1.0');
  await page.getByRole('button', { name: '병렬', exact: true }).click();
  await page.getByRole('button', { name: '빌드 시작', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('빌드 완료');
  const request = await page.evaluate(() => (window as any).desktopCalls.find((call: any) => call.command === 'start_job').args);
  expect(request.workflow).toBe('.github/workflows/release.yml');
  expect(request.event).toBe('push');
  expect(request.refName).toBe('v0.1.0');
  expect(request.execution).toBe('parallel');
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
