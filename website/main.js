const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// Spoken lines use markers: ~word is a filler, ^word is replaced by a
// change of mind, =word is written as a number or symbol.
function wordSpans(line) {
  return line.split(" ").map((raw) => {
    const span = document.createElement("span");
    const kind = { "~": "drop", "^": "fix", "=": "turn" }[raw[0]];
    span.className = "w" + (kind ? ` ${kind}` : "");
    span.textContent = kind ? raw.slice(1) : raw;
    return span;
  });
}

function fill(el, spans) {
  el.replaceChildren();
  spans.forEach((s, i) => el.append(s, i < spans.length - 1 ? " " : ""));
}

/* Nav shadow once the page scrolls */
const nav = $("#nav");
const onScroll = () => nav.classList.toggle("is-scrolled", window.scrollY > 8);
window.addEventListener("scroll", onScroll, { passive: true });
onScroll();

/* Waveforms: every .wave gets bars; bars move while the wave is "live" */
const waves = $$(".wave").map((el) => {
  const n = +el.dataset.wave || 9;
  for (let i = 0; i < n; i++) el.append(document.createElement("i"));
  return { el, bars: [...el.children], seed: Math.random() * 10 };
});
const visible = new Set();
const io = new IntersectionObserver((entries) =>
  entries.forEach((e) => (e.isIntersecting ? visible.add(e.target) : visible.delete(e.target)))
);
waves.forEach((w) => io.observe(w.el));

function waveLive(el) {
  const pill = el.closest(".pill, .mini-pill");
  if (!pill) return false;
  return pill.dataset.state === "listen" || pill.classList.contains("is-on");
}

function drawWaves(t) {
  for (const w of waves) {
    if (!visible.has(w.el)) continue;
    const live = waveLive(w.el);
    const max = w.el.clientHeight || 20;
    w.bars.forEach((b, i) => {
      const k = live
        ? 0.2 + 0.8 * Math.abs(Math.sin(t / 170 + i * 1.9 + w.seed) * Math.sin(t / 460 + i * 0.7))
        : 0.12;
      b.style.height = `${Math.max(3, Math.round(max * k))}px`;
    });
  }
  requestAnimationFrame(drawWaves);
}
if (reduced) {
  waves.forEach((w) => w.bars.forEach((b, i) => (b.style.height = `${[6, 12, 18, 10, 16, 8, 14, 9, 5][i % 9]}px`)));
} else {
  requestAnimationFrame(drawWaves);
}

/* Hero: a MacBook screen where Lipik types into a different app each round */
const stage = $("#stage");
const screen = $("#screen");
const frame = $("#screen-frame");
const clip = $("#screen-clip");

function fitScreen() {
  const cs = getComputedStyle(frame);
  const padX = parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight);
  const padY = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom);
  const inner = frame.clientWidth - padX;
  const compact = inner < 640;
  screen.classList.toggle("is-compact", compact);
  stage.classList.toggle("is-compact", compact);
  const [w, h] = compact ? [420, 560] : [1120, 700];
  const s = inner / w;
  screen.style.setProperty("--s", s);
  clip.style.height = `${h * s}px`;
  frame.style.height = `${h * s + padY}px`;
}
new ResizeObserver(fitScreen).observe(frame);
fitScreen();

const SAFARI_MENUS = "File Edit View History Bookmarks Window Help";
const SCENES = [
  {
    win: "slack",
    app: "Slack",
    menus: "File Edit View Go Window Help",
    said: "hey can we, ~um, move the call to ^four, ^no, =five",
    typed: "Hey, can we move the call to 5?",
  },
  {
    win: "gmail",
    app: "Safari",
    menus: SAFARI_MENUS,
    said: "~uh send it to rivaan =underscore m =at quavia =dot com",
    typed: "Send it to rivaan_m@quavia.com",
  },
  {
    win: "claude",
    app: "Safari",
    menus: SAFARI_MENUS,
    said: "can you, ~um, summarise this meeting for the ^team, ^no, for the clients in =five bullet points",
    typed: "Can you summarise this meeting for the clients in 5 bullet points?",
  },
];

const pill = $("#pill");
const said = $("#said");
const saidWords = $("#said-words");
const fnKey = $("#fn-key");
const mbApp = $("#mb-app");
const mbMenus = $("#mb-menus");
const mbLipik = $("#mb-lipik");

function showWindow(scene) {
  $$(".win", screen).forEach((w) => w.classList.toggle("is-on", w.dataset.win === scene.win));
  mbApp.textContent = scene.app;
  mbMenus.replaceChildren(
    ...scene.menus.split(" ").map((m) => {
      const span = document.createElement("span");
      span.textContent = m;
      return span;
    })
  );
  const field = $(`[data-win="${scene.win}"] .field`, screen);
  $(".slot", field).textContent = "";
  field.classList.remove("has-text");
  return field;
}

function land(field, text) {
  const slot = $(".slot", field);
  slot.textContent = text;
  field.classList.add("has-text");
  slot.classList.add("is-new");
  requestAnimationFrame(() => requestAnimationFrame(() => slot.classList.remove("is-new")));
}

let heroInView = true;
new IntersectionObserver(([e]) => (heroInView = e.isIntersecting)).observe(frame);
const ready = async () => {
  while (!heroInView || document.hidden) await wait(400);
};

async function playScene(scene) {
  await ready();
  const field = showWindow(scene);
  said.classList.remove("is-on", "is-struck");
  await wait(1100);

  fnKey.classList.add("is-down");
  pill.dataset.state = "listen";
  mbLipik.classList.add("is-live");
  const words = wordSpans(scene.said);
  fill(saidWords, words);
  await wait(300);
  said.classList.add("is-on");
  for (const w of words) {
    w.classList.add("on");
    await wait(w.textContent.length > 5 ? 260 : 210);
  }
  await wait(350);

  fnKey.classList.remove("is-down");
  mbLipik.classList.remove("is-live");
  pill.dataset.state = "think";
  said.classList.add("is-struck");
  await wait(900);

  pill.dataset.state = "idle";
  land(field, scene.typed);
  await wait(900);
  said.classList.remove("is-on");
  await wait(2600);
}

// ?scene=N shows that scene's finished state without motion, for screenshots.
const shot = new URLSearchParams(location.search).get("scene");
if (reduced || shot !== null) {
  const scene = SCENES[shot] || SCENES[0];
  if (shot !== null) document.documentElement.classList.add("is-shot");
  const field = showWindow(scene);
  fill(saidWords, wordSpans(scene.said));
  $$(".w", saidWords).forEach((w) => w.classList.add("on"));
  said.classList.add("is-on", "is-struck");
  land(field, scene.typed);
} else {
  (async () => {
    await wait(1300);
    for (let i = 0; ; i = (i + 1) % SCENES.length) await playScene(SCENES[i]);
  })();
}

/* Messy to clean: tabs */
const CLEAN = [
  {
    tab: "Changed your mind",
    said: "hey can we, ~um, move the call to ^four, ^no, =five",
    typed: "Hey, can we move the call to 5?",
  },
  {
    tab: "Email address",
    said: "send it to rivaan =underscore m =at quavia =dot com",
    typed: "Send it to rivaan_m@quavia.com",
  },
  {
    tab: "Times",
    said: "meet at =three =thirty =pm tomorrow",
    typed: "Meet at 3:30 PM tomorrow.",
  },
  {
    tab: "Français",
    lang: "fr",
    said: "~euh je prends le train de ^huit ^heures, ^non, de =neuf heures",
    typed: "Je prends le train de 9 heures.",
  },
  {
    tab: "Español",
    lang: "es",
    said: "~eh nos vemos el ^martes, ^bueno ^no, el miércoles a las =diez",
    typed: "Nos vemos el miércoles a las 10.",
  },
  {
    tab: "Hinglish",
    beta: true,
    said: "yaar kal ka standup, ~um, =ten baje rakh lete hain",
    typed: "Yaar, kal ka standup 10 baje rakh lete hain.",
  },
];

const tabsEl = $("#clean-tabs");
const panel = $("#clean-panel");
const spokenEl = $("#clean-spoken");
const resultEl = $("#clean-result");
let cleanRun = 0;

const tabs = CLEAN.map((ex, i) => {
  const b = document.createElement("button");
  b.type = "button";
  b.setAttribute("role", "tab");
  b.id = `clean-tab-${i}`;
  b.setAttribute("aria-controls", "clean-panel");
  b.textContent = ex.tab;
  if (ex.beta) {
    const tag = document.createElement("span");
    tag.className = "beta";
    tag.textContent = "beta";
    b.append(tag);
  }
  b.addEventListener("click", () => selectClean(i));
  tabsEl.append(b);
  return b;
});

tabsEl.addEventListener("keydown", (e) => {
  const i = tabs.indexOf(document.activeElement);
  if (i < 0) return;
  const next = { ArrowRight: i + 1, ArrowLeft: i - 1, Home: 0, End: tabs.length - 1 }[e.key];
  if (next === undefined) return;
  e.preventDefault();
  const j = (next + tabs.length) % tabs.length;
  tabs[j].focus();
  selectClean(j);
});

async function selectClean(i) {
  const run = ++cleanRun;
  const ex = CLEAN[i];
  tabs.forEach((t, j) => {
    t.setAttribute("aria-selected", String(i === j));
    t.tabIndex = i === j ? 0 : -1;
  });
  panel.setAttribute("aria-labelledby", tabs[i].id);
  spokenEl.lang = resultEl.lang = ex.lang || "";

  const words = wordSpans(ex.said);
  fill(spokenEl, words);
  spokenEl.classList.remove("is-struck");
  const out = ex.typed.split(" ").map((t) => {
    const s = document.createElement("span");
    s.className = "rw";
    s.textContent = t;
    return s;
  });
  fill(resultEl, out);

  if (reduced) {
    words.forEach((w) => w.classList.add("on"));
    out.forEach((w) => w.classList.add("on"));
    spokenEl.classList.add("is-struck");
    return;
  }
  for (const w of words) {
    if (run !== cleanRun) return;
    w.classList.add("on");
    await wait(70);
  }
  await wait(250);
  if (run !== cleanRun) return;
  spokenEl.classList.add("is-struck");
  await wait(450);
  for (const w of out) {
    if (run !== cleanRun) return;
    w.classList.add("on");
    await wait(55);
  }
}

let cleanStarted = false;
new IntersectionObserver(
  ([e], obs) => {
    if (e.isIntersecting && !cleanStarted) {
      cleanStarted = true;
      selectClean(0);
      obs.disconnect();
    }
  },
  { threshold: 0.4 }
).observe(panel);

/* Radio groups made of buttons (tone, snippets): click and arrow keys */
function radioGroup(group, onPick) {
  const items = $$("[role=radio]", group);
  const pick = (i, focus) => {
    items.forEach((b, j) => {
      b.setAttribute("aria-checked", String(i === j));
      b.tabIndex = i === j ? 0 : -1;
    });
    if (focus) items[i].focus();
    onPick(items[i], i);
  };
  items.forEach((b, i) => b.addEventListener("click", () => pick(i)));
  group.addEventListener("keydown", (e) => {
    const i = items.indexOf(document.activeElement);
    const d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (i < 0 || !d) return;
    e.preventDefault();
    pick((i + d + items.length) % items.length, true);
  });
}

/* Tone */
const TONES = {
  formal: "Hey, are we still good for Friday? I can bring the slides.",
  relaxed: "Hey, are we still good for Friday? I can bring the slides",
  lower: "hey are we still good for friday? i can bring the slides",
};
const toneText = $("#tone-text");
radioGroup($("#tone-seg"), async (b) => {
  if (!reduced) {
    toneText.classList.add("is-swapping");
    await wait(180);
  }
  toneText.textContent = TONES[b.dataset.tone];
  toneText.classList.remove("is-swapping");
});

/* Languages: cycle while the card is on screen */
const langItems = $$("#lang-list li");
const langTags = $$("#lang-tags li:not(.more)");
langItems[0].classList.add("is-on");
langTags[0].classList.add("is-on");
if (!reduced) {
  let li = 0;
  let langVisible = false;
  new IntersectionObserver(([e]) => (langVisible = e.isIntersecting)).observe($("#lang-list"));
  setInterval(() => {
    if (!langVisible || document.hidden) return;
    const prev = langItems[li];
    li = (li + 1) % langItems.length;
    prev.classList.replace("is-on", "is-off");
    setTimeout(() => prev.classList.remove("is-off"), 700);
    langItems[li].classList.add("is-on");
    langTags.forEach((t, j) => t.classList.toggle("is-on", j === li));
  }, 2200);
}

/* Vocabulary */
const MORE_WORDS = ["Anaïs", "pgvector", "Siobhán", "Llanfair", "Okonkwo", "Hyderabadi"];
const chips = $("#chips");
const addBtn = $("#chip-add");
const vocabLine = $("#vocab-line");
const VOCAB_LINES = {
  "Anaïs": "<mark>Anaïs</mark> has the signed contract.",
  pgvector: "We moved search to <mark>pgvector</mark> last week.",
  "Siobhán": "Loop in <mark>Siobhán</mark> before Friday.",
  Llanfair: "The office in <mark>Llanfair</mark> opens at nine.",
  Okonkwo: "Dr <mark>Okonkwo</mark> can see you at 4:15.",
  Hyderabadi: "Order the <mark>Hyderabadi</mark> biryani for six.",
};
addBtn.addEventListener("click", () => {
  const word = MORE_WORDS.shift();
  if (!word) return;
  const li = document.createElement("li");
  li.textContent = word;
  li.className = "is-new";
  chips.append(li);
  vocabLine.innerHTML = VOCAB_LINES[word];
  if (!MORE_WORDS.length) {
    addBtn.disabled = true;
    addBtn.lastChild.textContent = " All added";
  }
});

/* Hands-free */
const freeKeys = $("#free-keys");
const freePill = $("#free-pill");
freeKeys.addEventListener("click", () => {
  const on = freeKeys.getAttribute("aria-pressed") !== "true";
  freeKeys.setAttribute("aria-pressed", String(on));
  freePill.classList.toggle("is-on", on);
});

/* Snippets */
const SNIPS = [
  "14 Orchard Lane, Flat 3B\nPune 411001",
  "Hi, I'm Rivaan. I look after product at Quavia, and I'd love to show you what we're building.",
  "Thanks,\nRivaan",
];
const snipOut = $("#snip-out");
let snipRun = 0;
radioGroup($("#snip-cues"), async (b) => {
  const run = ++snipRun;
  const text = SNIPS[+b.dataset.snip];
  if (reduced) {
    snipOut.innerText = text;
    return;
  }
  const chars = [...text].map((c) => {
    if (c === "\n") return document.createElement("br");
    const s = document.createElement("span");
    s.className = "ch";
    s.textContent = c;
    return s;
  });
  snipOut.replaceChildren(...chars);
  for (const c of chars) {
    if (run !== snipRun) return;
    c.classList?.add("on");
    await wait(14);
  }
});

/* Meeting notes tabs */
const nmTabs = $$("#nm-tabs [role=tab]");
function selectNm(i) {
  nmTabs.forEach((t, j) => {
    t.setAttribute("aria-selected", String(i === j));
    t.tabIndex = i === j ? 0 : -1;
    document.getElementById(t.getAttribute("aria-controls")).hidden = i !== j;
  });
}
nmTabs.forEach((t, i) => t.addEventListener("click", () => selectNm(i)));
$("#nm-tabs").addEventListener("keydown", (e) => {
  if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
  const i = nmTabs.indexOf(document.activeElement);
  const j = (i + 1) % 2;
  nmTabs[j].focus();
  selectNm(j);
});

/* Offline toggle */
const wifi = $("#wifi");
const offStatus = $("#off-status");
const offText = $("#off-text");
const OFF_LINES = [
  "Notes from the train: the venue is booked for the 14th.",
  "Reminder: call the landlord about the leak on Monday.",
  "Draft: thanks for the feedback, I'll send a new version tonight.",
];
let offIdx = 0;
wifi.addEventListener("click", () => {
  const on = wifi.getAttribute("aria-checked") !== "true";
  wifi.setAttribute("aria-checked", String(on));
  offStatus.textContent = on ? "Online" : "Offline, still typing";
  offIdx = (offIdx + 1) % OFF_LINES.length;
  offText.textContent = OFF_LINES[offIdx];
  offText.classList.remove("is-new");
  void offText.offsetWidth;
  offText.classList.add("is-new");
});

/* How it works: the fn key in step 1 presses on a loop while visible */
const stepFn = $(".kb .key-fn");
if (reduced) {
  stepFn.classList.add("is-down");
} else {
  let stepVisible = false;
  new IntersectionObserver(([e]) => (stepVisible = e.isIntersecting)).observe(stepFn);
  setInterval(() => {
    if (stepVisible) stepFn.classList.toggle("is-down");
  }, 1200);
}
