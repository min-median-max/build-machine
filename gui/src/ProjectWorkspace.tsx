import { Icon } from './Icons';
import { platformNames, type Action, type ExecutionMode, type Platform, type Project } from './types';

export function ProjectWorkspace({ project, available, busy, onChange, onRemove, onAdd, onStart }: {
  project: Project | undefined; available: boolean; busy: Action | null;
  onChange: (project: Project) => void; onRemove: () => void; onAdd: () => void;
  onStart: (action: Action) => void;
}) {
  if (!project) return <section className="project-empty"><Icon name="folder" size={40}/><h2>프로젝트를 등록해주세요</h2><p>Git 프로젝트 폴더를 등록하면 빌드 설정을 저장할 수 있어요.</p><button className="primary-button" disabled={!!busy} onClick={onAdd}><Icon name="plus" size={17}/>폴더 등록</button></section>;
  const disabled = !!busy || !available || !project.platforms.length;
  // A workflow path turns the build button into a workflow replay, and the
  // replay has no launch step. Saying so beats a checkbox that does nothing.
  const replay = !!project.workflow?.trim();
  return <>
    <section className="project-selector">
      <span className="project-folder"><Icon name="folder" size={22}/></span>
      <div className="project-details"><span className="field-label">등록된 프로젝트</span><strong>{project.path.split('/').filter(Boolean).at(-1)}</strong><div className="project-path" title={project.path}>{project.path}</div></div>
      <button className="text-button" disabled={!!busy} onClick={onRemove}>등록 해제</button>
    </section>
    <section className="project-build-settings">
      <div className="section-title"><h2>빌드 대상<span>{project.platforms.length}개 선택</span></h2><span>프로젝트별로 저장됩니다</span></div>
      <div className="build-targets">{(['windows', 'linux', 'macos'] as Platform[]).map(platform => <button key={platform} className={`build-target ${project.platforms.includes(platform) ? 'selected' : ''}`} aria-pressed={project.platforms.includes(platform)} aria-label={`${platformNames[platform]} 빌드 대상`} disabled={!!busy} onClick={() => onChange({ ...project, platforms: project.platforms.includes(platform) ? project.platforms.filter(os => os !== platform) : [...project.platforms, platform] })}><Icon name={platform} size={21}/><span>{platformNames[platform]}</span><span className="selection-mark">{project.platforms.includes(platform) && <Icon name="check" size={13}/>}</span></button>)}</div>
      <p className="tool-status-note">설정에서 준비한 공통 도구로 빌드해요.</p>
    </section>
    <section className="workflow-settings" aria-label="GitHub Actions 재현 설정">
      <div className="section-title"><h2>워크플로 재현<span>선택 사항</span></h2><span>입력하면 저장소 workflow를 실행합니다</span></div>
      <div className="workflow-fields">
        <label>워크플로 경로<input aria-label="워크플로 경로" value={project.workflow || ''} placeholder=".github/workflows/release.yml" disabled={!!busy} onChange={event => onChange({ ...project, workflow: event.target.value || null })}/></label>
        <label>이벤트<select aria-label="워크플로 이벤트" value={project.event || 'workflow_dispatch'} disabled={!!busy} onChange={event => onChange({ ...project, event: event.target.value })}><option value="workflow_dispatch">workflow_dispatch</option><option value="push">push</option><option value="pull_request">pull_request</option></select></label>
        <label>ref<input aria-label="워크플로 ref" value={project.refName || ''} placeholder="현재 작업 트리" disabled={!!busy} onChange={event => onChange({ ...project, refName: event.target.value || null })}/></label>
      </div>
      <div className="execution-choice" role="group" aria-label="매트릭스 실행 방식"><span>환경 실행</span>{(['sequential', 'parallel'] as ExecutionMode[]).map(mode => <button key={mode} type="button" className={(project.execution || 'sequential') === mode ? 'selected' : ''} aria-pressed={(project.execution || 'sequential') === mode} disabled={!!busy} onClick={() => onChange({ ...project, execution: mode })}>{mode === 'sequential' ? '순차' : '병렬'}</button>)}</div>
    </section>
    <section className="action-bar">
      <button className="secondary-button" disabled={disabled} onClick={() => onStart('run')}><Icon name="play" size={14}/>최근 빌드 실행</button>
      <div className="build-actions"><label className="launch-option" title={replay ? '워크플로 재현에는 실행 단계가 없어요.' : undefined}><input type="checkbox" checked={project.launch && !replay} disabled={!!busy || replay} onChange={event => onChange({ ...project, launch: event.target.checked })}/>빌드 후 실행</label><button className="primary-button" disabled={disabled} onClick={() => onStart('build')}><Icon name={busy === 'build' ? 'refresh' : 'box'} className={busy === 'build' ? 'spin' : ''} size={17}/>{busy === 'build' ? (replay ? '재현 중' : '빌드 중') : (replay ? '워크플로 재현' : '빌드 시작')}<Icon name="arrow" size={16}/></button></div>
    </section>
  </>;
}
