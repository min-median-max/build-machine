export type Platform = 'windows' | 'linux' | 'macos';
export type Action = 'doctor' | 'setup' | 'build' | 'run';
export type WorkspacePage = 'dashboard' | 'environment' | 'projects';
export interface Project { path: string; platforms: Platform[]; launch: boolean }
export interface Preferences { controllerPath: string; environmentPlatforms: Platform[]; projects: Project[]; selectedProject: string | null; page: WorkspacePage }
export interface Environment { id: Platform; title: string; vm: string | null; target: string; status: string }
export interface Overview { controllerPath: string; environments: Environment[]; toolStatus: Partial<Record<Platform, ResultState>>; warning: string | null }
export interface OutputLine { stream: string; line: string }
export interface PlatformResult { success: boolean; error?: string; finishedAt: string }
export interface JobOutcome { exitCode: number; result: { results: Partial<Record<Platform, PlatformResult>>; log: string } | null; resultPath: string }
export interface ResultState extends PlatformResult { action: Action }
export interface BuildRecord { id: string; project: string; status: 'success' | 'failure' | 'incomplete'; platforms: Platform[]; results: Partial<Record<Platform, PlatformResult>> | null; recordedAt: number | null; finishedAt: string | null; log: string | null; error: string | null }
export interface DashboardData { projects: { path: string; latest: BuildRecord | null }[]; history: BuildRecord[]; warning: string | null }
export interface RunningJob { action: Action; projectPath: string | null; platform: Platform | null; seconds: number }
export const actionLabels: Record<Action, string> = { doctor: '환경 진단', setup: '도구 준비', build: '빌드', run: '앱 실행' };
export const platformNames: Record<Platform, string> = { windows: 'Windows', linux: 'Ubuntu', macos: 'macOS' };
