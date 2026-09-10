import type { Page } from '@playwright/test';

export async function desktopMock(page: Page, options: { missingController?: boolean; noProjects?: boolean } = {}) {
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
      selectedProject: noProjects ? null : '/fixtures/airdata', page: 'dashboard',
    };
    w.__TAURI_INTERNALS__ = {
      transformCallback: (callback: (value: unknown) => void) => { callbacks.set(++nextId, callback); return nextId; },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any) => {
        w.desktopCalls.push({ command, args: command === 'start_job' ? args.request : args });
        if (command === 'load_preferences') return JSON.parse(sessionStorage.getItem('fixture-preferences') || JSON.stringify(initial));
        if (command === 'save_preferences') { sessionStorage.setItem('fixture-preferences', JSON.stringify(args.preferences)); return null; }
        if (command === 'open_log_folder' || command === 'open_settings_folder' || command === 'open_build_log') return null;
        if (command === 'get_dashboard') {
          if (args.controllerPath === '/fixtures/missing') throw new Error('빌드 도구 폴더를 찾을 수 없어요.');
          if (w.failHistory) throw new Error('test history cannot be read');
          const history = JSON.parse(sessionStorage.getItem('fixture-build-history') || '[]').filter((record: any) => args.projectPaths.includes(record.project));
          return { projects: args.projectPaths.map((path: string) => ({ path, latest: history.find((record: any) => record.project === path) || null })), history: history.slice(0, 6), warning: w.historyWarning || null };
        }
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
          await new Promise(resolve => setTimeout(resolve, w.jobDelay || 650));
          const fail = w.failJob;
          send(2, fail ? 'ERROR: missing test compiler' : 'REUSED BUILD: verified test artifact', fail ? 'stderr' : 'stdout');
          const results = Object.fromEntries(args.request.platforms.map((os: string) => [os, { success: !fail, finishedAt: '2026-09-09T11:00:00Z', ...(fail ? { error: 'missing test compiler' } : {}) }]));
          if (!w.noReport && ['doctor', 'setup'].includes(args.request.action)) {
            const previous = JSON.parse(sessionStorage.getItem('fixture-tool-status') || '{}');
            for (const [os, result] of Object.entries(results)) previous[os] = { ...(result as object), action: args.request.action };
            sessionStorage.setItem('fixture-tool-status', JSON.stringify(previous));
          }
          if (args.request.action === 'build') {
            const history = JSON.parse(sessionStorage.getItem('fixture-build-history') || '[]');
            history.unshift({ id: `build-${history.length}`, project: args.request.projectPath, status: fail ? 'failure' : 'success', action: args.request.workflow ? 'ci' : 'build', platforms: args.request.platforms, results, recordedAt: Date.now(), finishedAt: '2026-09-09T11:00:00Z', log: '/fixtures/build.log', error: null });
            sessionStorage.setItem('fixture-build-history', JSON.stringify(history));
          }
          return { exitCode: fail ? 1 : 0, resultPath: '/fixtures/result.json', result: w.noReport ? null : { log: '/fixtures/build.log', results } };
        }
        throw new Error(`Unexpected desktop call: ${command}`);
      },
    };
  }, options);
}
