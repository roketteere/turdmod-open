import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';

// ---------------------------------------------------------------------------
// ReflectionDatabasePage — disk-read browser for the scumdump pipeline.
//
// SchemaPage reads live UE state via the bridge RPC (named pipe →
// SCUMServer.exe) and is therefore capped + server-dependent.
// This page reads scumdump's on-disk output (~14.5k classes, ~1.7k enums,
// ~3.4k structs from Phase A, plus Phase B/C summaries) so the full type
// system is browsable with no live server required.
// ---------------------------------------------------------------------------

type DbInfo = {
  root: string;
  build: string;
  classesPath: string;
  enumsPath: string;
  structsPath: string;
  widgetsDir: string;
  datatablesDir: string;
  stringsDir: string;
  sdkDir: string;
  metaJson: string;
};

type ReadTextResult = { content: string; existed: boolean };

type PhaseAResult = {
  count: number;
  ok: boolean;
  path: string;
};

type MetaShape = {
  dumpedAt?: string;
  scumBuild?: string;
  phase?: string;
  results?: {
    dumpAllClasses?: PhaseAResult;
    dumpAllEnums?: PhaseAResult;
    dumpAllStructs?: PhaseAResult;
  };
  phaseB?: {
    dumpedAt?: string;
    fileCount?: number;
    byteCount?: number;
    source?: string;
    sdkPath?: string;
  };
  phaseC?: {
    dumpedAt?: string;
    paksDir?: string;
    extractor?: string;
    widgets?: { count?: number; bytes?: number };
    datatables?: { count?: number; bytes?: number };
    strings?: { count?: number; bytes?: number; files?: number };
    errors?: number;
  };
};

type ClassFunction = {
  name: string;
  numParms: number;
  paramsSize: number;
  returnOffset: number;
  params: { name: string; type: string }[];
};

type ClassProperty = {
  name: string;
  type: string;
  offset: number;
  size: number;
  arrayDim: number;
};

type ClassEntry = {
  name: string;
  kind: string;
  parent: string;
  size: number;
  classFlags: number;
  properties: ClassProperty[];
  functions: ClassFunction[];
};

type EnumEntry = {
  name: string;
  cppForm?: string;
  flags?: number;
  entries: { name: string; value: number }[];
};

type StructEntry = {
  name: string;
  parent?: string;
  size: number;
  flags?: number;
  fields: ClassProperty[];
};

type Kind = 'classes' | 'enums' | 'structs';

function formatBytes(bytes: number | undefined): string {
  if (!bytes) return '—';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatTimestamp(iso: string | undefined): string {
  if (!iso) return '—';
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}

async function loadJson<T>(path: string): Promise<T> {
  const result = await invoke<ReadTextResult>('manager_read_text_file', { path });
  if (!result.existed) {
    throw new Error(`file not found: ${path}`);
  }
  return JSON.parse(result.content) as T;
}

export function ReflectionDatabasePage() {
  const dbQuery = useQuery({
    queryKey: ['scumdump-db'],
    queryFn: () => invoke<DbInfo>('manager_find_scumdump_database'),
    retry: false,
  });

  const meta = useMemo<MetaShape | null>(() => {
    if (!dbQuery.data) return null;
    try {
      return JSON.parse(dbQuery.data.metaJson);
    } catch {
      return null;
    }
  }, [dbQuery.data]);

  const [kind, setKind] = useState<Kind>('classes');
  const [filter, setFilter] = useState('');
  const [selected, setSelected] = useState<string | null>(null);

  const dataQuery = useQuery({
    queryKey: ['scumdump-data', dbQuery.data?.build, kind],
    queryFn: async () => {
      if (!dbQuery.data) throw new Error('database not located');
      const path =
        kind === 'classes'
          ? dbQuery.data.classesPath
          : kind === 'enums'
            ? dbQuery.data.enumsPath
            : dbQuery.data.structsPath;
      return loadJson<ClassEntry[] | EnumEntry[] | StructEntry[]>(path);
    },
    enabled: !!dbQuery.data,
    staleTime: Infinity,
  });

  const list = useMemo(() => {
    const data = dataQuery.data ?? [];
    const needle = filter.trim().toLowerCase();
    if (!needle) return data;
    return (data as { name: string }[]).filter((row) =>
      row.name.toLowerCase().includes(needle),
    );
  }, [dataQuery.data, filter]);

  const selectedEntry = useMemo(() => {
    if (!selected || !dataQuery.data) return null;
    return (dataQuery.data as { name: string }[]).find((row) => row.name === selected) ?? null;
  }, [selected, dataQuery.data]);

  if (dbQuery.isPending) {
    return <PageShell title="Reflection Database">Locating scumdump database…</PageShell>;
  }

  if (dbQuery.isError) {
    return (
      <PageShell title="Reflection Database">
        <div className="rounded border border-turd-bronze/40 bg-turd-bg-mid/40 p-4 text-sm text-turd-cream">
          <p className="font-medium text-turd-mustard-bright">No scumdump database found.</p>
          <p className="mt-2 text-turd-cream-dim">
            Run the scumdump extraction pipeline first. Expected path:
          </p>
          <code className="mt-2 block rounded bg-turd-bg-deep/60 px-2 py-1 font-mono text-xs">
            C:/Development/Claude/scumdump/data/extracted/v*/
          </code>
          <p className="mt-2 text-turd-cream-dim">
            Or set the <code className="font-mono">SCUMDUMP_DATA_DIR</code> env var to a
            custom <code className="font-mono">extracted/</code> parent.
          </p>
          <p className="mt-3 text-xs text-turd-cream-dim/60">
            {String((dbQuery.error as Error)?.message ?? dbQuery.error)}
          </p>
        </div>
      </PageShell>
    );
  }

  const db = dbQuery.data!;

  return (
    <PageShell title="Reflection Database">
      <section className="mb-4 rounded glass p-4">
        <div className="grid grid-cols-2 gap-x-6 gap-y-1 text-xs text-turd-cream-dim md:grid-cols-3">
          <Field label="SCUM build" value={meta?.scumBuild ?? db.build} />
          <Field label="Dumped at" value={formatTimestamp(meta?.dumpedAt)} />
          <Field label="Root" value={db.root} mono />
          <Field label="Classes" value={meta?.results?.dumpAllClasses?.count?.toLocaleString()} />
          <Field label="Enums" value={meta?.results?.dumpAllEnums?.count?.toLocaleString()} />
          <Field label="Structs" value={meta?.results?.dumpAllStructs?.count?.toLocaleString()} />
          <Field
            label="SDK files (Phase B)"
            value={meta?.phaseB?.fileCount?.toLocaleString()}
          />
          <Field label="SDK size" value={formatBytes(meta?.phaseB?.byteCount)} />
          <Field
            label="Widgets (Phase C)"
            value={`${meta?.phaseC?.widgets?.count ?? '—'} (${formatBytes(meta?.phaseC?.widgets?.bytes)})`}
          />
          <Field
            label="DataTables (Phase C)"
            value={`${meta?.phaseC?.datatables?.count ?? '—'} (${formatBytes(meta?.phaseC?.datatables?.bytes)})`}
          />
          <Field
            label="Strings (Phase C)"
            value={`${meta?.phaseC?.strings?.files ?? '—'} files (${formatBytes(meta?.phaseC?.strings?.bytes)})`}
          />
          <Field label="Phase C errors" value={String(meta?.phaseC?.errors ?? '—')} />
        </div>
      </section>

      <div className="grid grid-cols-[1fr_2fr] gap-4">
        <section className="flex flex-col rounded glass">
          <div className="flex items-center gap-1 border-b border-turd-bronze/20 p-2">
            {(['classes', 'enums', 'structs'] as Kind[]).map((k) => (
              <button
                key={k}
                onClick={() => {
                  setKind(k);
                  setSelected(null);
                }}
                className={[
                  'rounded px-2 py-1 text-xs uppercase tracking-wider transition-colors',
                  kind === k
                    ? 'bg-turd-bg-soft text-turd-mustard-bright'
                    : 'text-turd-cream-dim hover:text-turd-cream',
                ].join(' ')}
              >
                {k}
              </button>
            ))}
            <span className="ml-auto pr-2 text-[10px] text-turd-cream-dim">
              {dataQuery.isPending ? 'loading…' : `${list.length.toLocaleString()} shown`}
            </span>
          </div>
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={`filter ${kind}…`}
            className="m-2 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-sm text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard-bright focus:outline-none"
          />
          <div className="max-h-[60vh] overflow-y-auto px-2 pb-2">
            {dataQuery.isError && (
              <p className="p-2 text-xs text-red-400">
                {String((dataQuery.error as Error)?.message ?? dataQuery.error)}
              </p>
            )}
            {list.slice(0, 1000).map((row) => {
              const name = (row as { name: string }).name;
              return (
                <button
                  key={name}
                  onClick={() => setSelected(name)}
                  className={[
                    'block w-full truncate rounded px-2 py-1 text-left font-mono text-xs',
                    selected === name
                      ? 'bg-turd-bg-soft text-turd-mustard-bright'
                      : 'text-turd-cream hover:bg-turd-bg-soft/40',
                  ].join(' ')}
                >
                  {name}
                </button>
              );
            })}
            {list.length > 1000 && (
              <p className="px-2 py-1 text-[10px] text-turd-cream-dim">
                showing first 1000 of {list.length.toLocaleString()} — narrow the filter to see more
              </p>
            )}
          </div>
        </section>

        <section className="rounded glass p-4">
          {!selectedEntry ? (
            <p className="text-sm text-turd-cream-dim">Pick a {kind.replace(/es$/, '').replace(/s$/, '')} from the left.</p>
          ) : kind === 'classes' ? (
            <ClassDetail entry={selectedEntry as ClassEntry} />
          ) : kind === 'enums' ? (
            <EnumDetail entry={selectedEntry as EnumEntry} />
          ) : (
            <StructDetail entry={selectedEntry as StructEntry} />
          )}
        </section>
      </div>
    </PageShell>
  );
}

function PageShell({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h1 className="mb-4 font-display text-lg tracking-widest text-turd-mustard-bright">
        {title}
      </h1>
      {children}
    </div>
  );
}

function Field({ label, value, mono }: { label: string; value?: string; mono?: boolean }) {
  return (
    <div>
      <span className="uppercase tracking-wider text-turd-cream-dim/60">{label}:</span>{' '}
      <span className={mono ? 'font-mono text-turd-cream' : 'text-turd-cream'}>
        {value ?? '—'}
      </span>
    </div>
  );
}

function ClassDetail({ entry }: { entry: ClassEntry }) {
  return (
    <div className="space-y-3 text-sm">
      <div className="flex flex-wrap items-baseline gap-x-4">
        <h2 className="font-mono text-lg text-turd-cream">{entry.name}</h2>
        <span className="text-xs text-turd-cream-dim">kind: {entry.kind}</span>
        <span className="text-xs text-turd-cream-dim">size: {entry.size}</span>
        {entry.parent && (
          <span className="text-xs text-turd-cream-dim">
            extends <span className="font-mono text-turd-green">{entry.parent}</span>
          </span>
        )}
      </div>
      <Subsection title={`Properties (${entry.properties.length})`}>
        {entry.properties.length === 0 ? (
          <Empty />
        ) : (
          <PropertyTable rows={entry.properties} />
        )}
      </Subsection>
      <Subsection title={`Functions (${entry.functions.length})`}>
        {entry.functions.length === 0 ? (
          <Empty />
        ) : (
          <div className="space-y-1 font-mono text-xs">
            {entry.functions.map((fn) => (
              <div key={fn.name} className="text-turd-cream">
                <span className="text-turd-mustard-bright">{fn.name}</span>
                <span className="text-turd-cream-dim">
                  ({fn.params.map((p) => `${p.name}: ${p.type}`).join(', ')})
                </span>
              </div>
            ))}
          </div>
        )}
      </Subsection>
    </div>
  );
}

function EnumDetail({ entry }: { entry: EnumEntry }) {
  const entries = entry.entries ?? [];
  return (
    <div className="space-y-3 text-sm">
      <div className="flex items-baseline gap-x-4">
        <h2 className="font-mono text-lg text-turd-cream">{entry.name}</h2>
        {entry.cppForm && (
          <span className="text-xs text-turd-cream-dim">form: {entry.cppForm}</span>
        )}
      </div>
      <Subsection title={`Entries (${entries.length})`}>
        {entries.length === 0 ? (
          <Empty />
        ) : (
          <div className="space-y-0.5 font-mono text-xs">
            {entries.map((v) => (
              <div key={v.name} className="flex justify-between">
                <span className="text-turd-cream">{v.name}</span>
                <span className="text-turd-bronze-light">{v.value}</span>
              </div>
            ))}
          </div>
        )}
      </Subsection>
    </div>
  );
}

function StructDetail({ entry }: { entry: StructEntry }) {
  const fields = entry.fields ?? [];
  return (
    <div className="space-y-3 text-sm">
      <div className="flex flex-wrap items-baseline gap-x-4">
        <h2 className="font-mono text-lg text-turd-cream">{entry.name}</h2>
        <span className="text-xs text-turd-cream-dim">size: {entry.size}</span>
        {entry.parent && (
          <span className="text-xs text-turd-cream-dim">
            extends <span className="font-mono text-turd-green">{entry.parent}</span>
          </span>
        )}
      </div>
      <Subsection title={`Fields (${fields.length})`}>
        {fields.length === 0 ? <Empty /> : <PropertyTable rows={fields} />}
      </Subsection>
    </div>
  );
}

function PropertyTable({ rows }: { rows: ClassProperty[] }) {
  return (
    <table className="w-full font-mono text-xs">
      <thead>
        <tr className="text-left text-turd-cream-dim/60">
          <th className="py-1 font-normal">name</th>
          <th className="py-1 font-normal">type</th>
          <th className="py-1 text-right font-normal">offset</th>
          <th className="py-1 text-right font-normal">size</th>
          <th className="py-1 text-right font-normal">dim</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((p, i) => (
          <tr key={`${p.name}-${i}`} className="border-t border-turd-bronze/10">
            <td className="py-0.5 text-turd-cream">{p.name}</td>
            <td className="py-0.5 text-turd-bronze-light">{p.type}</td>
            <td className="py-0.5 text-right text-turd-cream-dim">{p.offset}</td>
            <td className="py-0.5 text-right text-turd-cream-dim">{p.size}</td>
            <td className="py-0.5 text-right text-turd-cream-dim">{p.arrayDim}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Subsection({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h3 className="mb-1 text-xs uppercase tracking-wider text-turd-cream-dim/60">{title}</h3>
      {children}
    </div>
  );
}

function Empty() {
  return <p className="text-xs text-turd-cream-dim/60">—</p>;
}
