export function Icon({ name, size = 18, className = '' }: { name: string; size?: number; className?: string }) {
  const paths: Record<string, React.ReactNode> = {
    plus: <path d="M12 5v14M5 12h14"/>,
    dashboard: <><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></>,
    box: <><path d="m12 3 9 5v8l-9 5-9-5V8l9-5Z"/><path d="m3 8 9 5 9-5M12 13v8M7.5 5.5l9 5"/></>,
    folder: <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>,
    terminal: <><path d="m5 7 5 5-5 5M13 17h6"/><rect x="2" y="3" width="20" height="18" rx="3"/></>,
    refresh: <><path d="M20 7v5h-5M4 17v-5h5"/><path d="M5.5 7a7 7 0 0 1 11-2L20 8M4 16l3.5 3A7 7 0 0 0 18.5 17"/></>,
    check: <path d="m5 12 4 4 10-10"/>,
    arrow: <path d="M5 12h14m-5-5 5 5-5 5"/>,
    play: <path d="m8 4 12 8-12 8Z"/>,
    tool: <><path d="m13 7 4 4M9 11l-6 6a3 3 0 0 0 4 4l6-6"/><path d="M21 3a6 6 0 0 0-8 8 6 6 0 0 0 8-8l-4 4-2-2Z"/></>,
    pulse: <><path d="M2 12h5l3-8 4 16 3-8h5"/></>,
    windows: <><path d="m3 5 8-1v8H3Zm10-1 8-1v9h-8ZM3 14h8v8l-8-1Zm10 0h8v9l-8-1Z"/></>,
    linux: <><circle cx="12" cy="12" r="7"/><circle cx="12" cy="3" r="2"/><circle cx="4" cy="16.5" r="2"/><circle cx="20" cy="16.5" r="2"/></>,
    macos: <><rect x="3" y="3" width="6" height="6" rx="3"/><rect x="15" y="3" width="6" height="6" rx="3"/><rect x="3" y="15" width="6" height="6" rx="3"/><rect x="15" y="15" width="6" height="6" rx="3"/><path d="M9 6h6M6 9v6m3 3h6m3-9v6"/></>,
    settings: <><path d="M4 7h16M4 17h16"/><circle cx="9" cy="7" r="3"/><circle cx="15" cy="17" r="3"/></>,
    alert: <><path d="m12 3 10 18H2Z"/><path d="M12 9v5m0 3v.1"/></>,
    clock: <><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></>,
  };
  return <svg className={className} width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name] || paths.box}</svg>;
}
