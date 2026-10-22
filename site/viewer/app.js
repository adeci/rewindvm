// The viewer page: the desktop app's window, built for the browser
// (crates/rewind-app/src/bin/rewind-web.rs), on a canvas below the site's
// header. The app has no file chooser, no way to fetch and no address bar
// of its own; this page does those for it, through window.rewindPage,
// which crates/rewind-app/src/web.rs calls.
//
//     viewer/?url=<rwd>&compare=<rwd>&step=<n>
//
// opens an export, compared with another, at a step. Relative links
// resolve against this page, so the examples beside it open as
// ?url=examples/philosophers.rwd.

// The app's module, loaded only once the frame is wide enough for it, so a
// phone never downloads it.
let web = null;

// The fonts the app is set in, which the web platform needs as files: it
// starts with none, and cannot read the system's or the stylesheet's.
const FONTS = [
  "fonts/IBMPlexSans-Regular.otf",
  "fonts/IBMPlexSans-Italic.otf",
  "fonts/IBMPlexSans-SemiBold.otf",
  "fonts/IBMPlexSans-Bold.otf",
  "fonts/JetBrainsMono-Regular.ttf",
  "fonts/JetBrainsMono-Italic.ttf",
  "fonts/JetBrainsMono-Bold.ttf",
];

// How long the playhead rests before the address follows it.
const ADDRESS_DELAY_MS = 300;

// The narrowest layout the app has. A frame narrower than this when the
// page opens shows a note instead of the app; one narrowed after the app
// started scrolls sideways.
const APP_MIN_WIDTH = 960;

const header = document.getElementById("site-header");
const narrowNote = document.getElementById("viewer-narrow");
const app = document.getElementById("viewer-app");
const frame = document.getElementById("viewer-frame");
const status = document.getElementById("viewer-status");
const pick = document.getElementById("pick");
const veil = document.getElementById("drop-veil");

// The window fills the viewport below the header, however tall it wraps.
function fitBelowHeader() {
  const top = header.offsetHeight;
  document.documentElement.style.setProperty("--viewer-top", `${top}px`);
}
new ResizeObserver(fitBelowHeader).observe(document.body);
fitBelowHeader();

// The app appends its canvas, and any message that the browser cannot
// draw, to the body; they go into the frame. On a touch screen narrower
// than the app, a sideways swipe scrolls the frame instead of reaching
// the app.
function adopt(node) {
  if (node instanceof HTMLCanvasElement) {
    frame.appendChild(node);
    status.hidden = true;
    const narrow = app.clientWidth < APP_MIN_WIDTH;
    if (narrow && matchMedia("(pointer: coarse)").matches) {
      node.style.touchAction = "pan-x";
    }
    return;
  }
  if (
    node instanceof HTMLParagraphElement &&
    node.parentElement === document.body
  ) {
    status.hidden = true;
    frame.appendChild(node);
  }
}
new MutationObserver((changes) => {
  for (const change of changes) {
    change.addedNodes.forEach(adopt);
  }
}).observe(document.body, { childList: true });

// The bytes of an export, fetched whole. A server's error answer comes
// back as its status; a fetch the browser refused, as a server without
// CORS headers gets, as null.
async function fetchBytes(url) {
  let response;
  try {
    response = await fetch(new URL(url, location.href));
  } catch {
    return { answer: null };
  }
  if (!response.ok) {
    return { answer: `${response.status} ${response.statusText}`.trim() };
  }
  return { bytes: new Uint8Array(await response.arrayBuffer()) };
}

// Fetches the exports an address names and hands them to the app, or
// tells it which one did not come.
async function fetchRuns(url, compare, step) {
  const [run, other] = await Promise.all([
    fetchBytes(url),
    compare ? fetchBytes(compare) : null,
  ]);
  if (!run.bytes) {
    web.unfetched(url, run.answer ?? undefined);
    return;
  }
  if (other && !other.bytes) {
    web.unfetched(compare, other.answer ?? undefined);
    return;
  }
  web.open_fetched(
    run.bytes,
    url,
    other?.bytes,
    compare ?? undefined,
    step ?? undefined,
  );
}

// Hands a picked or dropped file to the app, read in the browser.
async function openFile(file) {
  web.open_file(file.name, new Uint8Array(await file.arrayBuffer()));
}

// The address follows what is on screen once the playhead rests, without
// a history entry for every step.
let addressTimer;
function showAddress(query) {
  clearTimeout(addressTimer);
  addressTimer = setTimeout(() => {
    history.replaceState(
      null,
      "",
      `${location.pathname}${query}${location.hash}`,
    );
  }, ADDRESS_DELAY_MS);
}

window.rewindPage = {
  pickFile: () => pick.click(),
  fetchRuns,
  showAddress,
};

pick.addEventListener("change", () => {
  const [file] = pick.files;
  pick.value = "";
  if (file) {
    openFile(file);
  }
});

// A .rwd file dropped anywhere on the page opens. The app's canvas takes
// drops for itself and discards their files, so the page listens first.
const dragsFiles = (event) => event.dataTransfer?.types.includes("Files");
window.addEventListener(
  "dragover",
  (event) => {
    if (!web || !dragsFiles(event)) {
      return;
    }
    event.preventDefault();
    veil.hidden = false;
  },
  { capture: true },
);
window.addEventListener(
  "dragleave",
  (event) => {
    if (event.relatedTarget === null) {
      veil.hidden = true;
    }
  },
  { capture: true },
);
window.addEventListener(
  "drop",
  (event) => {
    if (!web || !dragsFiles(event)) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    veil.hidden = true;
    const [file] = event.dataTransfer.files;
    if (file) {
      openFile(file);
    }
  },
  { capture: true },
);

// The fonts load beside the app's code; one that does not come only costs
// itself, as the browser draws what is missing with its own.
async function fetchFont(url) {
  const fetched = await fetchBytes(url);
  return fetched.bytes ?? null;
}

// Loads the app's code and fonts and starts it on the page's address.
async function startApp() {
  narrowNote.hidden = true;
  status.hidden = false;
  frame.classList.add("running");
  try {
    const [module, ...fonts] = await Promise.all([
      import("./pkg/rewind-web.js"),
      ...FONTS.map(fetchFont),
    ]);
    await module.default();
    for (const font of fonts) {
      if (font) {
        module.add_font(font);
      }
    }
    web = module;
    web.start(location.search);
  } catch (error) {
    status.hidden = false;
    status.textContent = `The viewer did not start: ${error}`;
  }
}

// The app starts once the frame is as wide as its narrowest layout: at
// once on a computer, or when a narrow window is widened. Until then the
// note says why it is not there.
const watchWidth = new ResizeObserver(() => {
  if (app.clientWidth < APP_MIN_WIDTH) {
    narrowNote.hidden = false;
    status.hidden = true;
    return;
  }
  watchWidth.disconnect();
  startApp();
});
watchWidth.observe(app);
