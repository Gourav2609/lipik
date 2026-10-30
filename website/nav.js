{
  const nav = document.getElementById("nav");
  const toggle = nav.querySelector(".nav-toggle");
  const links = document.getElementById("nav-links");

  function setOpen(open) {
    nav.classList.toggle("is-open", open);
    toggle.setAttribute("aria-expanded", String(open));
    toggle.setAttribute("aria-label", open ? "Close menu" : "Open menu");
  }

  toggle.addEventListener("click", () =>
    setOpen(!nav.classList.contains("is-open")),
  );
  links.addEventListener("click", (e) => {
    if (e.target.closest("a")) setOpen(false);
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && nav.classList.contains("is-open")) {
      setOpen(false);
      toggle.focus();
    }
  });
  document.addEventListener("click", (e) => {
    if (!nav.contains(e.target)) setOpen(false);
  });
}

// Anonymous download counts: which page, and whether it was the .dmg or the
// Terminal command. Nothing about the visitor is sent.
const countDownload = (kind) => {
  const page =
    location.pathname
      .toLowerCase()
      .replace(/\.html$/, "")
      .replace(/\/(index)?$/, "") || "/";
  const body = JSON.stringify({ kind, page });
  try {
    if (
      navigator.sendBeacon(
        "/api/event",
        new Blob([body], { type: "application/json" }),
      )
    )
      return;
  } catch {}
  fetch("/api/event", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body,
    keepalive: true,
  }).catch(() => {});
};

document.addEventListener("click", (e) => {
  if (e.target.closest?.('a[href$=".dmg"]')) countDownload("dmg_click");
});

document.addEventListener("copy", () => {
  if (String(getSelection()).includes("install.sh")) countDownload("curl_copy");
});

{
  for (const button of document.querySelectorAll("[data-copy]")) {
    button.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(button.dataset.copy);
        if (button.dataset.copy.includes("install.sh"))
          countDownload("curl_copy");
        button.textContent = "Copied";
      } catch {
        button.textContent = "Select it";
      }
      setTimeout(() => (button.textContent = "Copy"), 1600);
    });
  }
}
