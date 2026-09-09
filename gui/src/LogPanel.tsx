import { useEffect, useRef } from 'react';
import { Icon } from './Icons';
import type { OutputLine } from './types';

export function LogPanel({ lines, busy, onOpenLogs }: { lines: OutputLine[]; busy: boolean; onOpenLogs: () => void }) {
  const viewport = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  useEffect(() => { if (viewport.current && follow.current) viewport.current.scrollTop = viewport.current.scrollHeight; }, [lines]);
  return <section className="log-panel" aria-label="실행 로그">
    <div className="log-toolbar"><span><Icon name="terminal" size={16}/>실행 로그{busy && <i className="live-dot"/>}</span><button className="text-button" onClick={onOpenLogs}><Icon name="folder" size={14}/>로그 폴더 열기<Icon name="arrow" size={13}/></button></div>
    <div className={`log-content ${lines.length ? '' : 'empty'}`} ref={viewport} onScroll={() => { const e = viewport.current; if (e) follow.current = e.scrollHeight - e.clientHeight - e.scrollTop < 32; }}>
      {lines.length ? lines.map((entry, index) => <div key={index} className={`log-line ${entry.stream === 'stderr' || entry.line.startsWith('ERROR:') ? 'log-error' : ''}`}><span className="line-number">{String(index + 1).padStart(3, '0')}</span><span>{entry.line || ' '}</span></div>) : <div className="log-placeholder"><Icon name="terminal" size={29}/><strong>작업을 실행하면 로그가 표시돼요</strong><span>진단과 빌드 과정이 여기에 실시간으로 표시돼요.</span></div>}
    </div>
    <div className="log-footer"><span><i className={busy ? 'working-dot' : ''}/>{busy ? '실행 중 · 작업이 끝날 때까지 앱을 열어두세요' : '전체 로그는 이 Mac에 저장됩니다'}</span><span>{lines.length.toLocaleString()} lines</span></div>
  </section>;
}
