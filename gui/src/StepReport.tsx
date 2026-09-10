import { Icon } from './Icons';
import { platformNames, type BuildRecord, type Platform, type PlatformResult, type WorkflowStep } from './types';

const stageNames: Record<string, string> = { doctor: '환경 진단', setup: '준비', test: '테스트', build: '빌드', smoke: '스모크', release: '릴리스' };
const stepMarks: Record<string, 'check' | 'alert' | 'clock'> = { passed: 'check', passed_with_limits: 'check', failed: 'alert', timeout: 'alert' };

function stepLabel(step: WorkflowStep) {
  if (step.status === 'timeout') return `시간 초과 (${step.timeoutSeconds ?? '?'}초)`;
  if (step.skipped) return '로컬에서 건너뜀';
  if (step.status === 'passed_with_limits') return step.reason || '제한 포함';
  if (step.localAdapter) return '로컬 어댑터로 대체';
  if (typeof step.exitCode === 'number') return `종료 코드 ${step.exitCode}`;
  return step.status || '';
}

function Step({ step }: { step: WorkflowStep }) {
  const body = step.output?.trimEnd();
  const mark = stepMarks[step.status || ''] || 'clock';
  const heading = <>
    <span className={`step-mark ${step.status || 'unknown'}`}><Icon name={mark} size={12}/></span>
    <span className="step-name">{step.name || `단계 ${step.index ?? ''}`}</span>
    <span className="step-note">{stepLabel(step)}</span>
  </>;
  if (!body && !step.command) return <div className="step-row">{heading}</div>;
  return <details className="step-row">
    <summary>{heading}<Icon name="arrow" size={12}/></summary>
    {step.command && <pre className="step-command">$ {step.command}</pre>}
    {body ? <pre className="step-output">{body}</pre> : <p className="step-empty">기록된 출력이 없어요.</p>}
  </details>;
}

export function StepReport({ record, onOpenLog }: { record: BuildRecord; onOpenLog: (path: string) => void }) {
  const entries = record.platforms
    .map(platform => [platform, record.results?.[platform]] as [Platform, PlatformResult | undefined])
    .filter(([, result]) => result);
  if (!entries.length) return null;
  return <div className="step-report">
    {entries.map(([platform, result]) => {
      const stages = Object.entries(result?.stages || {});
      return <section key={platform} className="step-platform">
        <header>
          <strong>{platformNames[platform] || platform}</strong>
          {result?.log && <button className="text-button" onClick={() => onOpenLog(result.log!)}><Icon name="terminal" size={13}/>실행 로그</button>}
        </header>
        {result?.error && <p className="history-error">{result.error}</p>}
        {stages.map(([stage, value]) => <div key={stage} className="step-stage">
          <div className="step-stage-title"><span>{stageNames[stage] || stage}</span><small>{value?.status || ''}</small></div>
          {(value?.steps || []).map((step, index) => <Step key={step.index ?? index} step={step}/>)}
          {value?.error && <p className="history-error">{value.error}</p>}
        </div>)}
        {!stages.length && <p className="step-empty">이 환경은 단계별 기록을 남기지 않았어요. 실행 로그를 확인해주세요.</p>}
        {!!result?.limits?.length && <ul className="step-limits">{result.limits.map((limit, index) => <li key={index}>{limit}</li>)}</ul>}
      </section>;
    })}
  </div>;
}
