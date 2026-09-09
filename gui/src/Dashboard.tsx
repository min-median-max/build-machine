import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Icon } from './Icons';
import { actionLabels, platformNames, type BuildRecord, type DashboardData, type Project, type RunningJob } from './types';

const name = (path: string) => path.split('/').filter(Boolean).at(-1) || path;
const labels = { success: '성공', passed_with_limits: '제한 포함', failure: '실패', incomplete: '완료 결과 없음' };
function RecordedTime({ record }: { record: BuildRecord }) {
  const stamp = record.finishedAt || record.recordedAt;
  if (!stamp) return <span className="muted">시각 없음</span>;
  const date = new Date(stamp);
  if (Number.isNaN(date.getTime())) return <span className="muted">시각 확인 불가</span>;
  return <time dateTime={date.toISOString()} title={date.toLocaleString('ko-KR')}>{record.finishedAt ? '완료' : '기록'} · {date.toLocaleString('ko-KR', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false })}</time>;
}
function Outcome({ record, running = false }: { record: BuildRecord | null; running?: boolean }) {
  if (running) return <span className="build-outcome running"><span className="spinner"/>빌드 중</span>;
  if (!record) return <span className="build-outcome unknown">기록 없음</span>;
  return <span className={`build-outcome ${record.status}`} title={record.error || Object.values(record.results || {}).map(result => result?.error).filter(Boolean).join('\n')}><Icon name={record.status === 'success' || record.status === 'passed_with_limits' ? 'check' : record.status === 'failure' ? 'alert' : 'clock'} size={13}/>{labels[record.status]}</span>;
}

export function Dashboard({ controllerPath, projects, job, showError, onSelect, onAdd, onViewJob, onOpenLog }: {
  controllerPath: string; projects: Project[]; job: RunningJob | null; showError: boolean;
  onSelect: (path: string) => void; onAdd: () => void; onViewJob: () => void; onOpenLog: (path: string) => void;
}) {
  const [data, setData] = useState<DashboardData | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const requestId = useRef(0);
  const paths = JSON.stringify(projects.map(project => project.path));
  const running = !!job;
  const refresh = async () => {
    const id = ++requestId.current;
    setLoading(true); setError('');
    try {
      const next = await invoke<DashboardData>('get_dashboard', { controllerPath, projectPaths: JSON.parse(paths) });
      if (id === requestId.current) setData(next);
    }
    catch (e) { if (id === requestId.current) { setData(null); setError(String(e)); } }
    finally { if (id === requestId.current) setLoading(false); }
  };
  useEffect(() => {
    if (controllerPath) void refresh(); else setLoading(false);
    return () => { requestId.current += 1; };
  }, [controllerPath, paths, running]);
  const building = (path: string) => job?.action === 'build' && job.projectPath === path;
  const count = (status: string) => data?.projects.filter(row => !building(row.path) && row.latest?.status === status).length || 0;
  const passed = (data?.projects.filter(row => !building(row.path) && (row.latest?.status === 'success' || row.latest?.status === 'passed_with_limits')).length || 0);
  const unavailable = data?.projects.filter(row => !row.latest || row.latest.status === 'incomplete' || building(row.path)).length || 0;
  const pending = !data || loading;

  return <div className="dashboard-content">
    <header><div><h1>대시보드</h1><p>등록된 프로젝트와 최근 빌드 결과를 확인하세요.</p></div><button className="refresh-button" disabled={loading || !controllerPath} onClick={() => void refresh()}><Icon name="refresh" className={loading ? 'spin' : ''} size={15}/>빌드 기록 새로고침</button></header>
    {job && <section className="running-operation" aria-label="현재 작업"><span className="spinner"/><div><strong>{job.projectPath ? name(job.projectPath) : '공통 환경'} · {actionLabels[job.action]} 중</strong><p>{job.platform ? platformNames[job.platform] : '명령 준비 중'} · {job.seconds}초</p></div><button className="secondary-button" onClick={onViewJob}>실행 화면<Icon name="arrow" size={14}/></button></section>}
    {error && showError && <div className="error-banner" role="alert"><Icon name="alert"/><pre>{error}</pre></div>}
    {data?.warning && <div className="dashboard-warning" role="alert"><Icon name="alert" size={16}/>{data.warning}</div>}
    <section className="dashboard-counts" aria-label="빌드 요약">
      <div><span>등록 프로젝트</span><strong>{projects.length}<small>개</small></strong><Icon name="folder" size={19}/></div>
      <div className="count-success"><span>최근 빌드 성공</span><strong>{pending ? '—' : passed}<small>개</small></strong><Icon name="check" size={19}/></div>
      <div className="count-failure"><span>최근 빌드 실패</span><strong>{pending ? '—' : count('failure')}<small>개</small></strong><Icon name="alert" size={19}/></div>
      <div><span>완료 결과 없음</span><strong>{pending ? '—' : unavailable}<small>개</small></strong><Icon name="clock" size={19}/></div>
    </section>
    <section className="dashboard-projects" aria-label="프로젝트별 최근 빌드">
      <div className="section-title"><h2>프로젝트별 최근 빌드</h2><span>가장 최근에 기록한 빌드 기준</span></div>
      {!projects.length ? <div className="dashboard-empty"><Icon name="folder" size={31}/><strong>첫 프로젝트를 등록해주세요</strong><p>프로젝트를 등록하면 빌드 결과를 한곳에서 볼 수 있어요.</p><button className="secondary-button" disabled={!!job} onClick={onAdd}><Icon name="plus" size={15}/>프로젝트 등록</button></div> : <div className="dashboard-project-list">
        {projects.map(project => {
          const latest = data?.projects.find(row => row.path === project.path)?.latest || null;
          return <button key={project.path} className="dashboard-project-row" aria-label={`${name(project.path)} 빌드 화면`} disabled={!!job && job.projectPath !== project.path} onClick={() => onSelect(project.path)}>
            <span className="dashboard-project-name"><span className="project-folder"><Icon name="folder" size={18}/></span><span><strong>{name(project.path)}</strong><small title={project.path}>{project.path}</small></span></span>
            <span className="dashboard-project-result">{pending && !building(project.path) ? <span className="muted">{loading ? '기록 확인 중' : '기록 확인 불가'}</span> : <Outcome record={latest} running={building(project.path)}/>}<small>{latest ? latest.platforms.map(os => platformNames[os] || os).join(' · ') : '—'}</small></span>
            <span className="dashboard-project-time">{latest ? <RecordedTime record={latest}/> : '—'}</span><Icon name="arrow" size={15}/>
          </button>;
        })}
      </div>}
    </section>
    <section className="dashboard-history" aria-label="최근 빌드 기록"><div className="section-title"><h2>최근 빌드 기록</h2><span>최대 6개</span></div>
      {!data?.history.length ? <p className="history-empty">{loading ? '빌드 기록을 확인하고 있어요…' : error ? '빌드 기록을 불러오지 못했어요.' : '아직 기록된 빌드가 없어요.'}</p> : data.history.map(record => <div className="history-row" key={record.id}>
        <span className={`history-mark ${record.status}`}><Icon name={record.status === 'success' || record.status === 'passed_with_limits' ? 'check' : record.status === 'failure' ? 'alert' : 'clock'} size={15}/></span>
        <div className="history-description"><strong>{name(record.project)}<Outcome record={record}/></strong><div className="history-platforms">{record.platforms.map(os => <span key={os}>{platformNames[os] || os} · {record.results?.[os]?.success === true ? (record.results?.[os]?.status === 'passed_with_limits' ? '제한 포함' : '성공') : record.results?.[os]?.success === false ? '실패' : '결과 없음'}</span>)}{record.executionMode && <span>{record.executionMode === 'parallel' ? '병렬' : '순차'} 실행</span>}{record.source?.revision && <span>ref {String(record.source.revision).slice(0, 12)}{record.source.dirty ? ' · 변경 있음' : ''}</span>}</div>{record.error && <p className="history-error">{record.error}</p>}</div>
        <RecordedTime record={record}/><button className="text-button" disabled={!record.log} aria-label={`${name(record.project)} ${record.id} 로그 열기`} onClick={() => record.log && onOpenLog(record.log)}><Icon name="terminal" size={14}/>로그<Icon name="arrow" size={12}/></button>
      </div>)}
    </section>
    <p className="dashboard-footnote">기록된 실행 대상의 결과예요. 현재 소스나 실행 중인 앱 상태는 별도로 확인해야 해요.</p>
  </div>;
}
