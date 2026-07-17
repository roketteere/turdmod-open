import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button, PageHeader, Section } from '@turdmod/turdmod-ui';

type TestResult = {
  ok: boolean;
  welcome: string;
  listingCount: number;
};

type ReadResult = {
  content: string;
  bytes: number;
};

// Default remote path for SCUM's Notifications.json on managed hosts —
// G-Portal exposes it under /SCUM/Saved/Config/WindowsServer/.
const DEFAULT_NOTIFICATIONS_PATH =
  'SCUM/Saved/Config/WindowsServer/Notifications.json';

export function ServersPage() {
  const [host, setHost] = useState('');
  const [port, setPort] = useState('21');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [remotePath, setRemotePath] = useState(DEFAULT_NOTIFICATIONS_PATH);

  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [testError, setTestError] = useState<string | null>(null);

  const [reading, setReading] = useState(false);
  const [readResult, setReadResult] = useState<ReadResult | null>(null);
  const [readError, setReadError] = useState<string | null>(null);

  const onTest = async () => {
    setTesting(true);
    setTestResult(null);
    setTestError(null);
    try {
      const result = await invoke<TestResult>('lite_ftp_test', {
        host,
        port: Number(port),
        username,
        password,
      });
      setTestResult(result);
    } catch (e) {
      setTestError(String(e));
    } finally {
      setTesting(false);
    }
  };

  const onRead = async () => {
    setReading(true);
    setReadResult(null);
    setReadError(null);
    try {
      const result = await invoke<ReadResult>('lite_ftp_read_text', {
        host,
        port: Number(port),
        username,
        password,
        remotePath,
      });
      setReadResult(result);
    } catch (e) {
      setReadError(String(e));
    } finally {
      setReading(false);
    }
  };

  const canTest = host && port && username && password && !testing;
  const canRead = canTest && remotePath && !reading;

  return (
    <div className="space-y-4">
      <PageHeader
        title="Servers"
        subtitle="Connect to a SCUM host over FTP and read its config."
      />

      <Section title="Connection">
        <div className="grid grid-cols-2 gap-3 text-sm">
          <Input label="Host" value={host} onChange={setHost} placeholder="ftp.g-portal.net" />
          <Input
            label="Port"
            value={port}
            onChange={setPort}
            placeholder="21"
            width="narrow"
          />
          <Input label="Username" value={username} onChange={setUsername} />
          <Input
            label="Password"
            value={password}
            onChange={setPassword}
            type="password"
          />
        </div>
        <div className="mt-3 flex gap-2">
          <Button onClick={onTest} disabled={!canTest}>
            {testing ? 'Testing…' : 'Test connection'}
          </Button>
        </div>
        {testError && (
          <p className="mt-2 text-xs text-red-400">{testError}</p>
        )}
        {testResult && (
          <p className="mt-2 text-xs text-turd-green">
            Connected — {testResult.listingCount} entries in root.{' '}
            {testResult.welcome && (
              <span className="text-turd-cream-dim">
                Welcome: {testResult.welcome.trim()}
              </span>
            )}
          </p>
        )}
      </Section>

      <Section title="Read remote file">
        <Input
          label="Remote path"
          value={remotePath}
          onChange={setRemotePath}
          placeholder={DEFAULT_NOTIFICATIONS_PATH}
        />
        <div className="mt-3 flex gap-2">
          <Button onClick={onRead} disabled={!canRead}>
            {reading ? 'Reading…' : 'Read file'}
          </Button>
        </div>
        {readError && (
          <p className="mt-2 text-xs text-red-400">{readError}</p>
        )}
        {readResult && (
          <div className="mt-3 space-y-2">
            <p className="text-xs text-turd-cream-dim">
              {readResult.bytes.toLocaleString()} bytes read.
            </p>
            <pre className="max-h-[40vh] overflow-auto rounded bg-turd-bg-deep/60 p-3 font-mono text-xs text-turd-cream">
              {readResult.content}
            </pre>
          </div>
        )}
      </Section>
    </div>
  );
}

function Input({
  label,
  value,
  onChange,
  type = 'text',
  placeholder,
  width = 'normal',
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: string;
  placeholder?: string;
  width?: 'normal' | 'narrow';
}) {
  return (
    <label
      className={[
        'flex flex-col gap-1',
        width === 'narrow' ? 'max-w-[120px]' : '',
      ].join(' ')}
    >
      <span className="text-[10px] uppercase tracking-wider text-turd-cream-dim/60">
        {label}
      </span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-sm text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard-bright focus:outline-none"
      />
    </label>
  );
}
