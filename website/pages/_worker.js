// lipik.pages.dev: Pages serves the static files itself; _routes.json sends
// only /api/* and /install.sh here, and they go on to the lipik-site Worker,
// which owns the database, the rate limit and the install count.
export default {
  fetch(request, env) {
    return env.SITE.fetch(request);
  },
};
