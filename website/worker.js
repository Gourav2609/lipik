// lipik.refflux.dev: the static site, plus the usage counts.
// /api/ping   POST {id, version, os, arch}  every 3 hours from the app
// /api/email  POST {id, email, updates}     from the setup step
// /api/stats  GET                           counts for /stats (no emails)
// /api/admin/emails  GET, Authorization: Bearer ADMIN_KEY
// /install.sh is counted, then served from the static files.

const UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const VERSION = /^\d{1,3}\.\d{1,3}\.\d{1,3}[0-9A-Za-z.-]{0,16}$/;
const OS = /^\d{1,2}(\.\d{1,3}){0,2}$/;
const ARCH = new Set(["arm64", "x86_64"]);
const EMAIL = /^[^\s@]{1,64}@[^\s@]+\.[^\s@]{2,}$/;

// Days are counted in India time, where the owner reads them.
const dayOf = (ms) => new Date(ms + 5.5 * 3600_000).toISOString().slice(0, 10);
const today = () => dayOf(Date.now());
const daysAgo = (n) => dayOf(Date.now() - n * 86400_000);

const json = (body, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
      "cache-control": "no-store",
    },
  });

async function readBody(request) {
  if (Number(request.headers.get("content-length") || 0) > 2048) return null;
  try {
    return await request.json();
  } catch {
    return null;
  }
}

async function ping(request, env) {
  const b = await readBody(request);
  if (
    !b ||
    !UUID.test(b.id || "") ||
    !VERSION.test(b.version || "") ||
    !OS.test(b.os || "") ||
    !ARCH.has(b.arch)
  ) {
    return json({ ok: false }, 400);
  }
  const day = today();
  const country = String(request.cf?.country || "").slice(0, 2) || "??";
  await env.DB.batch([
    env.DB.prepare(
      `INSERT INTO installs (id, first_day, last_day, version, os, arch, country, last_seen)
       VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7)
       ON CONFLICT (id) DO UPDATE SET last_day = ?2, version = ?3, os = ?4, arch = ?5, country = ?6, last_seen = ?7`,
    ).bind(b.id, day, b.version, b.os, b.arch, country, Date.now()),
    env.DB.prepare(
      "INSERT OR IGNORE INTO active_days (day, id, version) VALUES (?1, ?2, ?3)",
    ).bind(day, b.id, b.version),
  ]);
  return json({ ok: true });
}

async function email(request, env) {
  const b = await readBody(request);
  const address = String(b?.email || "")
    .trim()
    .toLowerCase();
  if (
    !b ||
    !UUID.test(b.id || "") ||
    address.length > 254 ||
    !EMAIL.test(address)
  ) {
    return json({ ok: false }, 400);
  }
  const day = today();
  await env.DB.prepare(
    `INSERT INTO emails (email, install_id, updates, first_day, last_day)
     VALUES (?1, ?2, ?3, ?4, ?4)
     ON CONFLICT (email) DO UPDATE SET install_id = ?2, updates = ?3, last_day = ?4`,
  )
    .bind(address, b.id, b.updates ? 1 : 0, day)
    .run();
  return json({ ok: true });
}

async function countInstaller(env) {
  await env.DB.prepare(
    `INSERT INTO installer_runs (day, runs) VALUES (?1, 1)
     ON CONFLICT (day) DO UPDATE SET runs = runs + 1`,
  )
    .bind(today())
    .run();
}

async function stats(env) {
  const all = (sql, ...args) =>
    env.DB.prepare(sql)
      .bind(...args)
      .all()
      .then((r) => r.results);
  const one = (sql, ...args) =>
    env.DB.prepare(sql)
      .bind(...args)
      .first("n");
  const since30 = daysAgo(29);
  const [
    installs,
    activeNow,
    activeToday,
    active7,
    active30,
    emails,
    installerTotal,
    newByDay,
    activeByDay,
    installerByDay,
    versions,
    countries,
    systems,
  ] = await Promise.all([
    one("SELECT COUNT(*) AS n FROM installs"),
    one(
      "SELECT COUNT(*) AS n FROM installs WHERE last_seen >= ?1",
      Date.now() - 3 * 3600_000,
    ),
    one("SELECT COUNT(*) AS n FROM active_days WHERE day = ?1", today()),
    one(
      "SELECT COUNT(DISTINCT id) AS n FROM active_days WHERE day >= ?1",
      daysAgo(6),
    ),
    one(
      "SELECT COUNT(DISTINCT id) AS n FROM active_days WHERE day >= ?1",
      since30,
    ),
    one("SELECT COUNT(*) AS n FROM emails"),
    one("SELECT COALESCE(SUM(runs), 0) AS n FROM installer_runs"),
    all(
      "SELECT first_day AS day, COUNT(*) AS n FROM installs WHERE first_day >= ?1 GROUP BY day",
      since30,
    ),
    all(
      "SELECT day, COUNT(*) AS n FROM active_days WHERE day >= ?1 GROUP BY day",
      since30,
    ),
    all("SELECT day, runs AS n FROM installer_runs WHERE day >= ?1", since30),
    all(
      "SELECT version AS name, COUNT(*) AS n FROM installs WHERE last_day >= ?1 GROUP BY version ORDER BY n DESC LIMIT 8",
      daysAgo(6),
    ),
    all(
      "SELECT country AS name, COUNT(*) AS n FROM installs GROUP BY country ORDER BY n DESC LIMIT 12",
    ),
    all(
      "SELECT os AS name, COUNT(*) AS n FROM installs GROUP BY os ORDER BY n DESC LIMIT 8",
    ),
  ]);
  return json({
    today: today(),
    installs,
    activeNow,
    activeToday,
    active7,
    active30,
    emails,
    installerTotal,
    newByDay,
    activeByDay,
    installerByDay,
    versions,
    countries,
    systems,
  });
}

async function adminEmails(request, env) {
  const key = (request.headers.get("authorization") || "").replace(
    /^Bearer /,
    "",
  );
  if (!env.ADMIN_KEY || key.length < 32 || key !== env.ADMIN_KEY.trim())
    return json({ ok: false }, 401);
  const rows = await env.DB.prepare(
    "SELECT email, updates, first_day, last_day FROM emails ORDER BY first_day DESC",
  ).all();
  return json({ emails: rows.results });
}

export default {
  async fetch(request, env, ctx) {
    const { pathname } = new URL(request.url);
    const method = request.method;
    try {
      if (pathname === "/api/ping" && method === "POST")
        return await ping(request, env);
      if (pathname === "/api/email" && method === "POST")
        return await email(request, env);
      if (pathname === "/api/stats" && method === "GET")
        return await stats(env);
      if (pathname === "/api/admin/emails" && method === "GET")
        return await adminEmails(request, env);
      if (pathname.startsWith("/api/")) return json({ ok: false }, 404);
    } catch (e) {
      console.error(e);
      return json({ ok: false }, 500);
    }
    if (pathname === "/install.sh" && method === "GET") {
      ctx.waitUntil(countInstaller(env).catch((e) => console.error(e)));
    }
    return env.ASSETS.fetch(request);
  },
};
