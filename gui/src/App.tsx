import { useEffect, useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Icon } from './Icons';
import { EnvironmentWorkspace } from './EnvironmentWorkspace';
import { ProjectWorkspace } from './ProjectWorkspace';
import { WorkspaceSidebar } from './WorkspaceSidebar';
import { LogPanel } from './LogPanel';
import { Dashboard } from './Dashboard';
import { actionLabels, platformNames, type Project, type WorkspacePage, type Action, type JobOutcome, type OutputLine, type Overview, type Platform, type Preferences, type ResultState } from './types';

const clean = (value: string) => value.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, '');

export default function App() {
  const [preferences, setPreferences] = useState<Preferences>({ controllerPath: '', environmentPlatforms: ['windows', 'linux', 'macos'], projects: [], selectedProject: null, page: 'dashboard' });
  const [overview, setOverview] = useState<Overview | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<Action | null>(null);
  const [error, setError] = useState('');
  const [lines, setLines] = useState<OutputLine[]>([]);
  const [active, setActive] = useState<Platform | null>(null);
  const [results, setResults] = useState<Partial<Record<Platform, ResultState>>>({});
  const [lastOutcome, setLastOutcome] = useState<{ success: boolean; action: Action; seconds: number } | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [jobProject, setJobProject] = useState<string | null>(null);
  const startTime = useRef(0);
  const pending = useRef<OutputLine[]>([]);

  const refresh = async (root: string) => {
    setLoading(true);
    try {
      const next = await invoke<Overview>('get_overview', { controllerPath: root });
      setOverview(next); setResults(next.toolStatus);
    }
    catch (e) { setOverview(null); setError(String(e)); }
    finally { setLoading(false); }
  };
  useEffect(() => { void (async () => {
    try { const saved = await invoke<Preferences>('load_preferences'); setPreferences(saved); await refresh(saved.controllerPath); }
    catch (e) { setError(String(e)); setLoading(false); }
  })(); }, []);
  useEffect(() => {
    if (!busy) return;
    const timer = window.setInterval(() => {
      setElapsed(Math.floor((Date.now() - startTime.current) / 1000));
      if (pending.current.length) { const next = pending.current.splice(0); setLines(old => [...old, ...next].slice(-10000)); }
    }, 100);
    return () => window.clearInterval(timer);
  }, [busy]);

  const save = async (next: Preferences) => {
    setPreferences(next);
    try { await invoke('save_preferences', { preferences: next }); }
    catch (e) { setError(`설정을 저장하지 못했어요: ${String(e)}`); }
  };
  const clearTask = () => { setError(''); setLines([]); setLastOutcome(null); };
  const navigate = (page: WorkspacePage, selectedProject = preferences.selectedProject) => {
    if (!busy) clearTask();
    void save({ ...preferences, page, selectedProject });
  };
  const chooseController = async () => {
    try {
      const path = await open({ directory: true, multiple: false, title: '빌드 도구 폴더 선택', defaultPath: preferences.controllerPath || undefined });
      if (typeof path !== 'string') return;
      clearTask(); await save({ ...preferences, controllerPath: path }); await refresh(path);
    } catch (e) { setError(String(e)); }
  };
  const addProject = async () => {
    try {
      const path = await open({ directory: true, multiple: false, title: '프로젝트 등록', defaultPath: preferences.selectedProject || undefined });
      if (typeof path !== 'string') return;
      const next = await invoke<Preferences>('register_project', { preferences, projectPath: path });
      clearTask(); setPreferences(next);
    } catch (e) { setError(String(e)); }
  };
  const changeProject = (project: Project) => void save({ ...preferences, projects: preferences.projects.map(item => item.path === project.path ? project : item) });
  const removeProject = () => {
    const projects = preferences.projects.filter(item => item.path !== preferences.selectedProject);
    clearTask(); void save({ ...preferences, projects, selectedProject: projects[0]?.path || null });
  };
  const project = preferences.projects.find(item => item.path === preferences.selectedProject);
  const start = async (action: Action) => {
    if (busy) return;
    const environmentAction = action === 'doctor' || action === 'setup';
    const platforms = environmentAction ? preferences.environmentPlatforms : project?.platforms || [];
    if (!platforms.length || (!environmentAction && !project)) return;
    setBusy(action); setError(''); setLines([]); setLastOutcome(null); setActive(null); setElapsed(0);
    setJobProject(environmentAction ? null : project!.path);
    pending.current = []; startTime.current = Date.now();
    const stream = new Channel<OutputLine>();
    const processErrors: string[] = [];
    stream.onmessage = (entry) => {
      const line = clean(entry.line); pending.current.push({ ...entry, line });
      if (entry.stream === 'stderr' || line.startsWith('ERROR:')) { processErrors.push(line); if (processErrors.length > 20) processErrors.shift(); }
      if (line.startsWith('PLATFORM: ')) setActive(line.slice(10).trim() as Platform);
    };
    try {
      const outcome = await invoke<JobOutcome>('start_job', { request: { controllerPath: preferences.controllerPath, projectPath: environmentAction ? null : project!.path, platforms, action, launch: action === 'build' && !!project?.launch,
        workflow: environmentAction ? null : (project?.workflow || null), event: environmentAction ? 'workflow_dispatch' : (project?.event || 'workflow_dispatch'), refName: environmentAction ? null : (project?.refName || null), execution: environmentAction ? 'sequential' : (project?.execution || 'sequential') }, onOutput: stream });
      setLastOutcome({ success: outcome.exitCode === 0, action, seconds: Math.floor((Date.now() - startTime.current) / 1000) });
      if (outcome.result && (action === 'doctor' || action === 'setup')) {
        setResults(old => { const next = { ...old }; for (const [os, result] of Object.entries(outcome.result!.results)) next[os as Platform] = { ...result!, action }; return next; });
      }
      if (outcome.exitCode !== 0) {
        const failures = Object.entries(outcome.result?.results || {}).filter(([, result]) => !result?.success).map(([os, result]) => `${platformNames[os as Platform]}: ${result?.error || '실패'}`);
        setError(failures.join('\n') || processErrors.join('\n') || `명령이 종료 코드 ${outcome.exitCode}로 끝났어요. 실행 로그를 확인해주세요.`);
      }
    } catch (e) { setError(String(e)); setLastOutcome({ success: false, action, seconds: Math.floor((Date.now() - startTime.current) / 1000) }); }
    finally {
      const remaining = pending.current.splice(0); if (remaining.length) setLines(old => [...old, ...remaining].slice(-10000));
      setBusy(null); setActive(null);
    }
  };
  const openLogs = async () => { try { await invoke('open_log_folder', { controllerPath: preferences.controllerPath }); } catch (e) { setError(String(e)); } };
  const openSettings = async () => { try { await invoke('open_settings_folder'); } catch (e) { setError(String(e)); } };
  const openBuildLog = async (path: string) => { try { await invoke('open_build_log', { controllerPath: preferences.controllerPath, path }); } catch (e) { setError(String(e)); } };
  const environmentPage = preferences.page === 'environment';
  const dashboardPage = preferences.page === 'dashboard';

  return <div className="app-shell">
    <WorkspaceSidebar preferences={preferences} disabled={!!busy} activeProject={jobProject} onSelect={path => navigate('projects', path)} onAdd={() => void addProject()} onSettings={() => navigate('environment')} onDashboard={() => navigate('dashboard')}/>
    <main>
      {dashboardPage ? <>
        {error && <div className="error-banner" role="alert"><Icon name="alert" size={17}/><pre>{error}</pre><button aria-label="오류 메시지 닫기" onClick={() => setError('')}>×</button></div>}
        <Dashboard showError={!error} controllerPath={preferences.controllerPath} projects={preferences.projects} job={busy ? { action: busy, projectPath: jobProject, platform: active, seconds: elapsed } : null} onSelect={path => navigate('projects', path)} onAdd={() => void addProject()} onViewJob={() => navigate(jobProject ? 'projects' : 'environment', jobProject || preferences.selectedProject)} onOpenLog={path => void openBuildLog(path)}/>
      </> : <>
      <header><div><h1>{environmentPage ? '설정' : project?.path.split('/').filter(Boolean).at(-1) || '프로젝트 등록'}</h1><p>{environmentPage ? '모든 프로젝트가 공유하는 빌드 환경을 관리하세요.' : project ? '빌드 대상을 설정하고, 준비된 환경에서 빌드·실행하세요.' : '+ 버튼으로 빌드할 프로젝트 폴더를 등록하세요.'}</p></div>
        {environmentPage ? <button className="refresh-button" disabled={!!busy || loading || !preferences.controllerPath} onClick={() => { setError(''); void refresh(preferences.controllerPath); }}><Icon name="refresh" className={loading ? 'spin' : ''} size={15}/>연결 새로고침</button> : null}
      </header>
      {environmentPage ? <EnvironmentWorkspace preferences={preferences} overview={overview} loading={loading} busy={busy} active={active} results={results} onToggle={platform => void save({ ...preferences, environmentPlatforms: preferences.environmentPlatforms.includes(platform) ? preferences.environmentPlatforms.filter(os => os !== platform) : [...preferences.environmentPlatforms, platform] })} onController={() => void chooseController()} onSettingsFolder={() => void openSettings()} onStart={action => void start(action)}/> : <ProjectWorkspace project={project} available={!!overview && !loading} busy={busy} onChange={changeProject} onRemove={removeProject} onAdd={() => void addProject()} onStart={action => void start(action)}/>}
      <div className="operation-summary" role="status"><div>{busy ? <><span className="spinner"/>{actionLabels[busy]} 중{active ? ` · ${platformNames[active]}` : ''}</> : lastOutcome ? <><span className={`result-symbol ${lastOutcome.success ? 'success' : 'failure'}`}><Icon name={lastOutcome.success ? 'check' : 'alert'} size={13}/></span>{actionLabels[lastOutcome.action]} {lastOutcome.success ? '완료' : '실패'}</> : <><span className="idle-dot"/>작업 대기 중<span className="summary-hint">{environmentPage && !Object.keys(results).length ? '환경 진단부터 시작할 수 있어요' : '실행할 작업을 선택하세요'}</span></>}</div><span className="duration"><Icon name="clock" size={13}/>{busy ? `${elapsed}초` : lastOutcome ? `${lastOutcome.seconds}초` : '—'}</span></div>
      {error && <div className="error-banner" role="alert"><Icon name="alert" size={17}/><pre>{error}</pre><button aria-label="오류 메시지 닫기" onClick={() => setError('')}>×</button></div>}
      <LogPanel lines={lines} busy={!!busy} onOpenLogs={() => void openLogs()}/>
      </>}
    </main>
  </div>;
}
