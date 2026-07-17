# @turdmod/turdmod-ui

Shared React components for Manager / Lite / Pro.

## Current exports

Low-level visual primitives (all use Tailwind classes that assume
the `turd-*` palette in the consumer app's `tailwind.config.ts`):

- `PageHeader` — page title + subtitle + actions
- `Section` — bordered card wrapper with optional title
- `Field` — label/value display
- `EmptyState` — empty placeholder with optional action
- `Button` — primary / secondary / danger variants

## Coming next

The whole point of this package is to share the **heavy** SCUM-specific
editors (Notifications.json editor, ServerSettings 250-key editor,
EconomyOverride.json editor, RaidTimes.json editor) across the three
frontends so we don't fork them per app. Those land here as Lite + Pro
need them.

## License

MIT.
