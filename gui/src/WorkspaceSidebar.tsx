import { Icon } from './Icons';
import type { Preferences } from './types';

export function WorkspaceSidebar({ preferences, disabled, onSelect, onAdd, onSettings }: {
  preferences: Preferences; disabled: boolean;
  onSelect: (path: string) => void;
  onAdd: () => void; onSettings: () => void;
}) {
  return <aside className="sidebar">
    <div className="brand"><div className="brand-symbol"><Icon name="box" size={23}/></div><div>build machine</div></div>
    <section className="registered-section">
      <div className="registered-heading"><span className="sidebar-label">등록된 프로젝트</span><button className="add-project-button" aria-label="프로젝트 추가" title="프로젝트 등록" disabled={disabled} onClick={onAdd}><Icon name="plus" size={17}/></button></div>
      <div className="registered-list">
        {preferences.projects.map(project => <button key={project.path} className={`registered-project ${preferences.page === 'projects' && project.path === preferences.selectedProject ? 'current' : ''}`} title={project.path} disabled={disabled} onClick={() => onSelect(project.path)}><span className="project-dot"/><span>{project.path.split('/').filter(Boolean).at(-1)}</span><Icon name="arrow" size={13}/></button>)}
        {!preferences.projects.length && <p className="sidebar-note">+ 버튼으로 프로젝트를<br/>등록해주세요.</p>}
      </div>
    </section>
    <div className="sidebar-bottom">
      <div className="local-badge"><span/><div>내 컴퓨터에서 실행<small>macOS · Parallels</small></div></div>
      <button className={`controller-settings ${preferences.page === 'environment' ? 'current' : ''}`} disabled={disabled} onClick={onSettings}><Icon name="settings" size={15}/>설정<Icon name="arrow" size={13}/></button>
      <div className="sidebar-version">BUILD MACHINE <span>0.0.1</span></div>
    </div>
  </aside>;
}
