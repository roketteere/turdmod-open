import type { MiddlewareHandler } from "hono";

/**
 * Bearer-token middleware. Reads a single shared secret from
 * `GUARD_ADMIN_TOKEN` (env or runtime override) and compares it timing-
 * safely against the `Authorization: Bearer <token>` header.
 *
 * Skipped (returns 503) when the env is unset — admin endpoints stay
 * locked, but the service still serves public lookups so a misconfigured
 * deploy doesn't accidentally leak ban-list edits.
 */
export function requireAdmin(getToken: () => string | undefined): MiddlewareHandler {
  return async (c, next) => {
    const expected = getToken();
    if (!expected) {
      return c.json(
        { error: "guard_admin_token_unset", message: "Server has no GUARD_ADMIN_TOKEN configured." },
        503,
      );
    }
    const header = c.req.header("authorization") ?? "";
    const match = /^Bearer\s+(.+)$/i.exec(header);
    if (!match || !match[1] || !timingSafeEqual(match[1], expected)) {
      return c.json({ error: "unauthorized" }, 401);
    }
    await next();
  };
}

function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}
