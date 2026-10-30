// lipik-site: the usage counts and the installer. The site itself lives on
// lipik.pages.dev, whose /api/* and /install.sh come here through a service
// binding. Every other request (the old lipik.refflux.dev pages) is sent there.
// /api/ping   POST {id, version, os, arch, step}  every 3 hours from the app
// /api/email  POST {email, updates}         from the setup step
// /api/event  POST {kind, page}             website .dmg clicks and install-command copies
// /api/stats  GET                           counts for /stats (no emails)
// /api/github GET                           stars and release downloads, cached 10 minutes
// /api/admin/emails  GET, Authorization: Bearer ADMIN_KEY
// /install.sh is counted, then served from the static files.

const UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const VERSION = /^\d{1,3}\.\d{1,3}\.\d{1,3}[0-9A-Za-z.-]{0,16}$/;
const OS = /^\d{1,2}(\.\d{1,5}){0,3}$/;
const ARCH = new Set(["arm64", "x86_64"]);
// Setup steps in order; installs.step stores the position (0 = not sent).
const STEPS = ["installed", "permissions", "model_ready", "first_dictation"];
const EMAIL = /^[^\s@]{1,64}@[^\s@]+\.[^\s@]{2,}$/;
const EVENT_KINDS = new Set(["dmg_click", "curl_copy"]);
// The site's pages, as in sitemap.xml.
const PAGES = new Set([
  "/",
  "/pricing",
  "/releases",
  "/hinglish",
  "/hinglish-speech-to-text",
  "/hindi-voice-typing-mac",
  "/meeting-notes",
  "/free-dictation-mac",
  "/benchmark",
  "/wispr-flow-alternative",
  "/superwhisper-alternative",
  "/macwhisper-alternative",
  "/granola-alternative",
]);
const MAX_BODY = 2048;
const SITE = "https://lipik.pages.dev";

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

// JSON only: a web page can't send this content type to another site
// without asking first, so pages can't post on someone's behalf.
async function readBody(request) {
  if (
    !(request.headers.get("content-type") || "").startsWith("application/json")
  ) {
    return null;
  }
  if (Number(request.headers.get("content-length")) > MAX_BODY) return null;
  const reader = request.body?.getReader();
  if (!reader) return null;
  const body = new Uint8Array(MAX_BODY);
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (size + value.length > MAX_BODY) {
      await reader.cancel();
      return null;
    }
    body.set(value, size);
    size += value.length;
  }
  try {
    return JSON.parse(new TextDecoder().decode(body.subarray(0, size)));
  } catch {
    return null;
  }
}

// Requests per minute from one address; the address is used for this and not kept.
const limited = async (request, env) => {
  if (!env.LIMIT) return false;
  const key = request.headers.get("cf-connecting-ip") || "unknown";
  const { success } = await env.LIMIT.limit({ key });
  return !success;
};

async function ping(request, env) {
  const b = await readBody(request);
  if (
    !b ||
    !UUID.test(b.id || "") ||
    !VERSION.test(b.version || "") ||
    !OS.test(b.os || "") ||
    !ARCH.has(b.arch) ||
    (b.step !== undefined && !STEPS.includes(b.step))
  ) {
    return json({ ok: false }, 400);
  }
  const step = STEPS.indexOf(b.step) + 1;
  if (await limited(request, env)) return json({ ok: false }, 429);
  const day = today();
  // A check-in within the hour on the same day changes nothing worth a write.
  const seen = await env.DB.prepare(
    "SELECT last_day, last_seen, version, step FROM installs WHERE id = ?1",
  )
    .bind(b.id)
    .first();
  if (
    seen &&
    seen.last_day === day &&
    seen.version === b.version &&
    seen.step >= step &&
    Date.now() - seen.last_seen < 3600_000
  ) {
    return json({ ok: true });
  }
  const country = String(request.cf?.country || "").slice(0, 2) || "??";
  await env.DB.batch([
    env.DB.prepare(
      `INSERT INTO installs (id, first_day, last_day, version, os, arch, country, last_seen, step)
       VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
       ON CONFLICT (id) DO UPDATE SET last_day = ?2, version = ?3, os = ?4, arch = ?5, country = ?6, last_seen = ?7, step = MAX(step, ?8)`,
    ).bind(b.id, day, b.version, b.os, b.arch, country, Date.now(), step),
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
  if (!b || address.length > 254 || !EMAIL.test(address)) {
    return json({ ok: false }, 400);
  }
  if (await limited(request, env)) return json({ ok: false }, 429);
  // Only ever adds: nobody can turn someone else's emails off from here.
  const day = today();
  await env.DB.prepare(
    `INSERT INTO emails (email, install_id, updates, first_day, last_day)
     VALUES (?1, '', 1, ?2, ?2)
     ON CONFLICT (email) DO UPDATE SET last_day = ?2`,
  )
    .bind(address, day)
    .run();
  return json({ ok: true });
}

async function event(request, env) {
  const b = await readBody(request);
  if (!b || !EVENT_KINDS.has(b.kind) || !PAGES.has(b.page)) {
    return json({ ok: false }, 400);
  }
  if (await limited(request, env)) return json({ ok: false }, 429);
  await env.DB.prepare(
    `INSERT INTO site_events (day, kind, page, n) VALUES (?1, ?2, ?3, 1)
     ON CONFLICT (day, kind, page) DO UPDATE SET n = n + 1`,
  )
    .bind(today(), b.kind, b.page)
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
  // Of the installs that dictated at least once and were first seen at least
  // `days` days ago, how many were active again on or after first_day + `after`.
  const kept = (days, after) =>
    env.DB.prepare(
      `SELECT COUNT(*) AS installs,
         COALESCE(SUM(EXISTS (SELECT 1 FROM active_days a
           WHERE a.id = i.id AND a.day >= date(i.first_day, ?2))), 0) AS kept
       FROM installs i WHERE i.first_day <= ?1 AND i.step = ?3`,
    )
      .bind(
        daysAgo(days),
        `+${after} days`,
        STEPS.indexOf("first_dictation") + 1,
      )
      .first();
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
    stepsAll,
    steps30,
    day2,
    day7,
    siteEvents,
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
    all("SELECT step, COUNT(*) AS n FROM installs GROUP BY step"),
    all(
      "SELECT step, COUNT(*) AS n FROM installs WHERE first_day >= ?1 GROUP BY step",
      since30,
    ),
    kept(2, 1),
    kept(7, 6),
    all(
      `SELECT page,
         SUM(CASE WHEN kind = 'dmg_click' THEN n ELSE 0 END) AS dmg,
         SUM(CASE WHEN kind = 'curl_copy' THEN n ELSE 0 END) AS curl
       FROM site_events WHERE day >= ?1 GROUP BY page ORDER BY dmg + curl DESC LIMIT 100`,
      since30,
    ),
  ]);
  // An install that reached a step also passed every step before it.
  const reached = (rows) => {
    const at = (k) => rows.find((r) => r.step === k)?.n || 0;
    return {
      unknown: at(0),
      steps: STEPS.map((name, i) => ({
        name,
        n: STEPS.slice(i).reduce((sum, _, j) => sum + at(i + j + 1), 0),
      })),
    };
  };
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
    funnel: { all: reached(stepsAll), last30: reached(steps30) },
    retention: [
      { name: "day2", installs: day2.installs, kept: day2.kept },
      { name: "day7", installs: day7.installs, kept: day7.kept },
    ],
    siteEvents,
  });
}

// GitHub allows 60 unsigned requests an hour per address, which one visitor
// can use up; the Worker asks once and shares the answer for 10 minutes.
async function github(request, ctx) {
  const cache = caches.default;
  const key = new Request(new URL("/api/github", request.url));
  const hit = await cache.match(key);
  if (hit) return hit;
  const get = (path) =>
    fetch(`https://api.github.com/repos/Gourav2609/lipik${path}`, {
      headers: {
        "user-agent": "lipik-site",
        accept: "application/vnd.github+json",
      },
    }).then((r) => {
      if (!r.ok) throw new Error(`GitHub ${r.status}`);
      return r.json();
    });
  const [repo, releases] = await Promise.all([
    get(""),
    get("/releases?per_page=100"),
  ]);
  const count = (assets, name) =>
    assets
      .filter((a) => a.name === name)
      .reduce((s, a) => s + a.download_count, 0);
  const body = {
    stars: repo.stargazers_count,
    forks: repo.forks_count,
    issues: repo.open_issues_count,
    releases: releases
      .filter((r) => !r.draft)
      .map((r) => ({
        tag: r.tag_name,
        published: r.published_at,
        installs: count(r.assets, "Lipik.dmg"),
        updates: count(r.assets, "Lipik.app.tar.gz"),
      })),
  };
  const res = new Response(JSON.stringify(body), {
    headers: {
      "content-type": "application/json",
      "cache-control": "public, max-age=600",
    },
  });
  ctx.waitUntil(cache.put(key, res.clone()));
  return res;
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
    const { pathname, search } = new URL(request.url);
    const method = request.method;
    try {
      if (pathname === "/api/ping" && method === "POST")
        return await ping(request, env);
      if (pathname === "/api/email" && method === "POST")
        return await email(request, env);
      if (pathname === "/api/event" && method === "POST")
        return await event(request, env);
      if (pathname === "/api/stats" && method === "GET")
        return await stats(env);
      if (pathname === "/api/github" && method === "GET")
        return await github(request, ctx);
      if (pathname === "/api/admin/emails" && method === "GET")
        return await adminEmails(request, env);
      if (pathname.startsWith("/api/")) return json({ ok: false }, 404);
    } catch (e) {
      console.error(e);
      return json({ ok: false }, 500);
    }
    if (pathname === "/install.sh") {
      if (method === "GET" && !(await limited(request, env))) {
        ctx.waitUntil(countInstaller(env).catch((e) => console.error(e)));
      }
      return env.ASSETS.fetch(request);
    }
    return Response.redirect(`${SITE}${pathname}${search}`, 301);
  },
};
