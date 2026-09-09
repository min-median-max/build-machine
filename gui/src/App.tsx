import { useEffect, useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Icon } from './Icons';
import { EnvironmentCard } from './EnvironmentCard';
import { LogPanel } from './LogPanel';
import { actionLabels, type Action, type JobOutcome, type OutputLine, type Overview, type Platform, type Preferences, type ResultState } from './types';

const platformNames: Record<Platform, string> = { windows: 'Windows', linux: 'Ubuntu', macos: 'macOS' };
const clean = (value: string) => value.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, '');

export default function App() {
  const [preferences, setPreferences] = useState<Preferences>({ controllerPath: '', projectPath: null, platforms: ['linux'] });
  const [overview, setOverview] = useState<Overview | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<Action | null>(null);
  const [error, setError] = useState('');
  const [lines, setLines] = useState<OutputLine[]>([]);
  const [launch, setLaunch] = useState(false);
  const [active, setActive] = useState<Platform | null>(null);
  const [results, setResults] = useState<Partial<Record<Platform, ResultState>>>({});
  const [lastOutcome, setLastOutcome] = useState<{ success: boolean; action: Action; seconds: number } | null>(null);
  const [elapsed, setElapsed] = useState(0);
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
  const choose = async (controller = false) => {
    try {
      const path = await open({ directory: true, multiple: false, title: controller ? '빌드 도구 폴더 선택' : '프로젝트 폴더 선택', defaultPath: controller ? preferences.controllerPath || undefined : preferences.projectPath || undefined });
      if (typeof path !== 'string') return;
      setError('');
      if (controller) { await save({ ...preferences, controllerPath: path }); await refresh(path); }
      else await save({ ...preferences, projectPath: path });
    } catch (e) { setError(String(e)); }
  };
  const start = async (action: Action) => {
    if (busy) return;
    setBusy(action); setError(''); setLines([]); setLastOutcome(null); setActive(null); setElapsed(0);
    pending.current = []; startTime.current = Date.now();
    const stream = new Channel<OutputLine>();
    const processErrors: string[] = [];
    stream.onmessage = (entry) => {
      const line = clean(entry.line); pending.current.push({ ...entry, line });
      if (entry.stream === 'stderr' || line.startsWith('ERROR:')) { processErrors.push(line); if (processErrors.length > 20) processErrors.shift(); }
      if (line.startsWith('PLATFORM: ')) setActive(line.slice(10).trim() as Platform);
    };
    try {
      const outcome = await invoke<JobOutcome>('start_job', { request: { ...preferences, action, launch: action === 'build' && launch }, onOutput: stream });
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
  const projectName = preferences.projectPath?.split('/').filter(Boolean).at(-1);
  const disabled = !!busy || loading || !overview || !preferences.platforms.length;

  return <div className="app-shell">
    <aside className="sidebar">
      <div className="brand"><div className="brand-symbol"><Icon name="box" size={23}/></div><div>build machine<span>DESKTOP WORKSPACE</span></div></div>
      <div className="sidebar-label">WORKSPACE</div>
      <div className="nav-active"><Icon name="box"/>빌드 작업실<span>01</span></div>
      <div className="recent-section"><div className="sidebar-label">최근 프로젝트</div>{overview?.recentProjects.length ? overview.recentProjects.map(path => <button key={path} className={`recent-project ${path === preferences.projectPath ? 'current' : ''}`} title={path} disabled={!!busy} onClick={() => void save({ ...preferences, projectPath: path })}><span className="project-dot"/>{path.split('/').filter(Boolean).at(-1)}<Icon name="arrow" size={13}/></button>) : <p className="sidebar-note">프로젝트를 선택하면<br/>여기에서 다시 찾을 수 있어요.</p>}</div>
      <div className="sidebar-bottom"><div className="local-badge"><span/><div>내 컴퓨터에서 실행<small>macOS · Parallels</small></div></div><button className="controller-settings" disabled={!!busy} onClick={() => void choose(true)} title={preferences.controllerPath}><Icon name="settings" size={15}/>빌드 도구 폴더<Icon name="arrow" size={13}/></button><div className="sidebar-version">BUILD MACHINE <span>0.0.1</span></div></div>
    </aside>
    <main>
      <header><div><div className="eyebrow"><span/>LOCAL BUILD CONTROL</div><h1>빌드 작업실</h1><p>하나의 프로젝트를, 각 운영체제에서.</p></div><button className="refresh-button" disabled={!!busy || loading || !preferences.controllerPath} onClick={() => { setError(''); void refresh(preferences.controllerPath); }}><Icon name="refresh" className={loading ? 'spin' : ''} size={15}/>연결 새로고침</button></header>
      <section className="project-selector"><span className="project-folder"><Icon name="folder" size={22}/></span><div className="project-details"><span className="field-label">프로젝트</span><strong>{projectName || '프로젝트를 선택해주세요'}</strong><div className="project-path" title={preferences.projectPath || ''}>{preferences.projectPath || '빌드할 Git 프로젝트 폴더를 연결하세요'}</div></div><button className="secondary-button" disabled={!!busy} onClick={() => void choose()}><Icon name="folder" size={15}/>폴더 선택</button></section>
      <section className="environments"><div className="section-title"><h2>실행 환경<span>{preferences.platforms.length}개 선택</span></h2><span>실행할 환경을 선택하세요</span></div><div className="environment-grid">{overview ? overview.environments.map(environment => <EnvironmentCard key={environment.id} environment={environment} selected={preferences.platforms.includes(environment.id)} disabled={!!busy} active={active === environment.id} result={results[environment.id]} onToggle={() => void save({ ...preferences, platforms: preferences.platforms.includes(environment.id) ? preferences.platforms.filter(os => os !== environment.id) : [...preferences.platforms, environment.id] })}/>) : <div className="environment-unavailable"><Icon name={loading ? 'refresh' : 'alert'}/>{loading ? '실행 환경을 확인하고 있어요…' : '빌드 도구 폴더를 연결하면 실행 환경이 표시돼요.'}</div>}</div><p className="tool-status-note">도구 상태는 마지막 검사 결과예요. 다시 확인하려면 환경 진단을 실행하세요.</p>{overview?.warning && <div className="connection-warning"><Icon name="alert" size={14}/>{overview.warning}</div>}</section>
      <section className="action-bar"><div className="utility-actions"><button className="secondary-button" disabled={disabled} onClick={() => void start('doctor')}><Icon name="pulse" size={16}/>환경 진단</button><button className="secondary-button" disabled={disabled} onClick={() => void start('setup')}><Icon name="tool" size={15}/>도구 준비</button><button className="text-button last-run" disabled={disabled || !preferences.projectPath} onClick={() => void start('run')}><Icon name="play" size={14}/>최근 빌드 실행</button></div><div className="build-actions"><label className="launch-option"><input type="checkbox" checked={launch} disabled={!!busy} onChange={event => setLaunch(event.target.checked)}/>빌드 후 실행</label><button className="primary-button" disabled={disabled || !preferences.projectPath} onClick={() => void start('build')}><Icon name={busy === 'build' ? 'refresh' : 'box'} className={busy === 'build' ? 'spin' : ''} size={17}/>{busy === 'build' ? '빌드 중' : '빌드 시작'}<Icon name="arrow" size={16}/></button></div></section>
      <div className="operation-summary" role="status"><div>{busy ? <><span className="spinner"/>{actionLabels[busy]} 중{active ? ` · ${platformNames[active]}` : ''}</> : lastOutcome ? <><span className={`result-symbol ${lastOutcome.success ? 'success' : 'failure'}`}><Icon name={lastOutcome.success ? 'check' : 'alert'} size={13}/></span>{actionLabels[lastOutcome.action]} {lastOutcome.success ? '완료' : '실패'}</> : <><span className="idle-dot"/>작업 대기 중<span className="summary-hint">{Object.keys(results).length ? '실행할 작업을 선택하세요' : '환경 진단부터 시작할 수 있어요'}</span></>}</div><span className="duration"><Icon name="clock" size={13}/>{busy ? `${elapsed}초` : lastOutcome ? `${lastOutcome.seconds}초` : '—'}</span></div>
      {error && <div className="error-banner" role="alert"><Icon name="alert" size={17}/><pre>{error}</pre><button aria-label="오류 메시지 닫기" onClick={() => setError('')}>×</button></div>}
      <LogPanel lines={lines} busy={!!busy} onOpenLogs={() => void openLogs()}/>
    </main>
  </div>;
}
