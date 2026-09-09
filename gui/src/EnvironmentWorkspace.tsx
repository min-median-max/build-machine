import { EnvironmentCard } from './EnvironmentCard';
import { Icon } from './Icons';
import type { Action, Overview, Platform, Preferences, ResultState } from './types';

export function EnvironmentWorkspace({ preferences, overview, loading, busy, active, results, onToggle, onController, onSettingsFolder, onStart }: {
  preferences: Preferences; overview: Overview | null; loading: boolean; busy: Action | null;
  active: Platform | null; results: Partial<Record<Platform, ResultState>>;
  onToggle: (platform: Platform) => void; onController: () => void; onSettingsFolder: () => void; onStart: (action: Action) => void;
}) {
  const disabled = !!busy || loading || !overview || !preferences.environmentPlatforms.length;
  return <>
    <section className="shared-environment-note"><span className="project-folder"><Icon name="tool" size={22}/></span><div><strong>모든 프로젝트가 함께 쓰는 환경</strong><p>한 번 준비한 도구를 등록된 프로젝트들이 공유해요.</p></div></section>
    <section className="environments">
      <div className="section-title"><h2>실행 환경<span>{preferences.environmentPlatforms.length}개 선택</span></h2><span>진단하거나 준비할 환경을 선택하세요</span></div>
      <div className="environment-grid">
        {overview ? overview.environments.map(environment => <EnvironmentCard key={environment.id} environment={environment} selected={preferences.environmentPlatforms.includes(environment.id)} disabled={!!busy} active={active === environment.id} result={results[environment.id]} onToggle={() => onToggle(environment.id)}/>) : <div className="environment-unavailable"><Icon name={loading ? 'refresh' : 'alert'}/>{loading ? '실행 환경을 확인하고 있어요…' : '빌드 도구 폴더를 연결하면 실행 환경이 표시돼요.'}</div>}
      </div>
      <p className="tool-status-note">도구 상태는 마지막 검사 결과예요. 다시 확인하려면 환경 진단을 실행하세요.</p>
      {overview?.warning && <div className="connection-warning"><Icon name="alert" size={14}/>{overview.warning}</div>}
    </section>
    <section className="action-bar">
      <div className="utility-actions">
        <button className="secondary-button" disabled={disabled} onClick={() => onStart('doctor')}><Icon name="pulse" size={16}/>환경 진단</button>
        <button className="primary-button" disabled={disabled} onClick={() => onStart('setup')}><Icon name="tool" size={15}/>도구 준비</button>
      </div>
      <div className="utility-actions">
        <button className="text-button" title={preferences.controllerPath} disabled={!!busy} onClick={onController}><Icon name="folder" size={14}/>빌드 도구 폴더</button>
        <button className="text-button" disabled={!!busy} onClick={onSettingsFolder}><Icon name="folder" size={14}/>설정 폴더 열기</button>
      </div>
    </section>
  </>;
}
