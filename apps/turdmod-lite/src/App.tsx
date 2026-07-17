import { NavLink, Route, Routes } from 'react-router-dom';
import { ServersPage } from './pages/ServersPage';
import { WelcomePage } from './pages/WelcomePage';

const NAV = [
  { to: '/', label: 'Welcome', exact: true },
  { to: '/servers', label: 'Servers' },
];

export function App() {
  return (
    <div className="grid h-full grid-cols-[180px_1fr]">
      <aside className="flex flex-col border-r border-turd-bronze/30 bg-turd-bg-mid/40 p-3">
        <div className="mb-3 px-2 pt-1">
          <p className="font-display text-sm text-turd-mustard-bright">
            TurdMOD
          </p>
          <p className="text-[10px] uppercase tracking-wider text-turd-cream-dim">Lite</p>
        </div>
        <nav className="flex flex-col gap-1 text-sm">
          {NAV.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.exact}
              className={({ isActive }) =>
                [
                  'rounded px-3 py-2 transition-colors',
                  isActive
                    ? 'bg-turd-bg-soft text-turd-mustard-bright'
                    : 'text-turd-cream-dim hover:bg-turd-bg-soft/40 hover:text-turd-cream',
                ].join(' ')
              }
            >
              {item.label}
            </NavLink>
          ))}
        </nav>
        <div className="mt-auto px-2 text-[10px] text-turd-cream-dim">v{__APP_VERSION__}</div>
      </aside>
      <main className="overflow-auto p-6">
        <Routes>
          <Route path="/" element={<WelcomePage />} />
          <Route path="/servers" element={<ServersPage />} />
        </Routes>
      </main>
    </div>
  );
}
