// Wizard state machine — single useReducer owns all collected data.

import {
  createContext,
  useCallback,
  useContext,
  useReducer,
  type Dispatch,
  type ReactNode,
} from 'react';
import type { InstallTarget } from '../lib/tauri-target';

export interface NodeInfo {
  found: boolean;
  version: string | null;
}

export interface WizardState {
  step: number;
  installs: { game: string | null; server: string | null } | null;
  target: InstallTarget;
  installPath: string | null;
  logsDir: string;
  logsDirTestResult: { exists: boolean; hasLogFiles: boolean } | null;
  skipRcon: boolean;
  rconHost: string;
  rconPort: string;
  rconPassword: string;
  rconTestResult: { ok: boolean; error: string | null } | null;
  nodeInfo: NodeInfo | null;
  startCompanion: boolean;
  committing: boolean;
  commitError: string | null;
}

const STEP_COUNT = 5;

const initialState: WizardState = {
  step: 0,
  installs: null,
  target: 'server',
  installPath: null,
  logsDir: '',
  logsDirTestResult: null,
  skipRcon: false,
  rconHost: '127.0.0.1',
  rconPort: '27015',
  rconPassword: '',
  rconTestResult: null,
  nodeInfo: null,
  startCompanion: false,
  committing: false,
  commitError: null,
};

export type WizardAction =
  | { type: 'NEXT' }
  | { type: 'PREV' }
  | { type: 'GOTO'; step: number }
  | { type: 'SET_INSTALLS'; installs: WizardState['installs'] }
  | { type: 'SET_TARGET'; target: InstallTarget }
  | { type: 'SET_INSTALL_PATH'; path: string | null }
  | { type: 'SET_LOGS_DIR'; dir: string }
  | { type: 'SET_LOGS_DIR_TEST'; result: WizardState['logsDirTestResult'] }
  | { type: 'SET_SKIP_RCON'; skip: boolean }
  | { type: 'SET_RCON_HOST'; host: string }
  | { type: 'SET_RCON_PORT'; port: string }
  | { type: 'SET_RCON_PASSWORD'; password: string }
  | { type: 'SET_RCON_TEST'; result: WizardState['rconTestResult'] }
  | { type: 'SET_NODE_INFO'; info: NodeInfo }
  | { type: 'SET_START_COMPANION'; start: boolean }
  | { type: 'SET_COMMITTING'; committing: boolean }
  | { type: 'SET_COMMIT_ERROR'; error: string | null };

function reducer(state: WizardState, action: WizardAction): WizardState {
  switch (action.type) {
    case 'NEXT':
      return { ...state, step: Math.min(state.step + 1, STEP_COUNT - 1) };
    case 'PREV':
      return { ...state, step: Math.max(state.step - 1, 0) };
    case 'GOTO':
      return { ...state, step: action.step };
    case 'SET_INSTALLS':
      return { ...state, installs: action.installs };
    case 'SET_TARGET':
      return { ...state, target: action.target };
    case 'SET_INSTALL_PATH':
      return { ...state, installPath: action.path };
    case 'SET_LOGS_DIR':
      return { ...state, logsDir: action.dir, logsDirTestResult: null };
    case 'SET_LOGS_DIR_TEST':
      return { ...state, logsDirTestResult: action.result };
    case 'SET_SKIP_RCON':
      return { ...state, skipRcon: action.skip };
    case 'SET_RCON_HOST':
      return { ...state, rconHost: action.host, rconTestResult: null };
    case 'SET_RCON_PORT':
      return { ...state, rconPort: action.port, rconTestResult: null };
    case 'SET_RCON_PASSWORD':
      return { ...state, rconPassword: action.password, rconTestResult: null };
    case 'SET_RCON_TEST':
      return { ...state, rconTestResult: action.result };
    case 'SET_NODE_INFO':
      return { ...state, nodeInfo: action.info };
    case 'SET_START_COMPANION':
      return { ...state, startCompanion: action.start };
    case 'SET_COMMITTING':
      return { ...state, committing: action.committing };
    case 'SET_COMMIT_ERROR':
      return { ...state, commitError: action.error };
    default:
      return state;
  }
}

interface WizardContextValue {
  state: WizardState;
  dispatch: Dispatch<WizardAction>;
  stepCount: number;
  next: () => void;
  prev: () => void;
}

const WizardContext = createContext<WizardContextValue | null>(null);

export function WizardProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState);
  const next = useCallback(() => dispatch({ type: 'NEXT' }), []);
  const prev = useCallback(() => dispatch({ type: 'PREV' }), []);
  return (
    <WizardContext.Provider value={{ state, dispatch, stepCount: STEP_COUNT, next, prev }}>
      {children}
    </WizardContext.Provider>
  );
}

export function useWizard(): WizardContextValue {
  const ctx = useContext(WizardContext);
  if (!ctx) throw new Error('useWizard must be used inside <WizardProvider>');
  return ctx;
}
