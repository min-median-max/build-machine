import { Icon } from './Icons';
import { actionLabels, type Environment, type ResultState } from './types';

export function EnvironmentCard({ environment, selected, disabled, active, result, onToggle }: {
  environment: Environment; selected: boolean; disabled: boolean; active: boolean;
  result?: ResultState; onToggle: () => void;
}) {
  const connected = ['running', 'local'].includes(environment.status);
  const status = environment.status === 'local' ? '이 Mac' : connected ? 'VM 실행 중' : environment.status === 'unavailable' ? '연결 확인 필요' : 'VM 중지됨';
  const checkedAt = result ? new Date(result.finishedAt) : null;
  const checkedTime = checkedAt?.toLocaleString('ko-KR', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
  const resultLabel = result ? `${actionLabels[result.action]} ${result.success ? result.action === 'doctor' ? '통과' : '완료' : '실패'}` : '아직 검사하지 않았어요';
  return <button type="button" className={`environment-card ${selected ? 'selected' : ''} ${active ? 'active-job' : ''}`} aria-pressed={selected} aria-label={`${environment.title} 선택`} onClick={onToggle} disabled={disabled}>
    <div className="environment-top"><span className={`os-icon ${environment.id}`}><Icon name={environment.id} size={24}/></span><span className="selection-mark">{selected && <Icon name="check" size={13}/>}</span></div>
    <div className="environment-name">{environment.title}<span className="architecture">{environment.id === 'macos' ? 'UNIVERSAL' : 'ARM64'}</span></div>
    <div className="environment-description">{environment.vm || '현재 사용 중인 컴퓨터'}</div>
    <div className="environment-bottom"><span className={`connection ${connected ? 'connected' : ''}`}><i/>{status}</span>{active && <span className="environment-progress"><span className="spinner"/>작업 중</span>}</div>
    <div className={`tool-result ${result ? result.success ? 'passed' : 'failed' : 'unchecked'}`}>
      <span className="tool-result-label"><Icon name={result ? result.success ? 'check' : 'alert' : 'pulse'} size={15}/>{resultLabel}</span>
      {result ? <time dateTime={result.finishedAt}>마지막 확인 · {checkedTime}</time> : <span className="tool-result-hint">환경 진단을 실행해주세요</span>}
    </div>
  </button>;
}
