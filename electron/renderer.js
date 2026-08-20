const previewEl = document.getElementById("preview");
const emptyEl = document.getElementById("empty");
const statusEl = document.getElementById("status");
const progressEl = document.getElementById("progress");
const progressFill = document.getElementById("progressFill");
const progressPct = document.getElementById("progressPct");
const progressLabel = document.getElementById("progressLabel");
const stopBtn = document.getElementById("stopBtn");
const folderLabel = document.getElementById("folderLabel");
const counter = document.getElementById("counter");
const filenameEl = document.getElementById("filename");
const detailsEl = document.getElementById("details");
const prevBtn = document.getElementById("prevBtn");
const nextBtn = document.getElementById("nextBtn");
const openBtn = document.getElementById("openBtn");
const compressBtn = document.getElementById("compressBtn");
const decompressBtn = document.getElementById("decompressBtn");
const compressDirBtn = document.getElementById("compressDirBtn");
const decompressDirBtn = document.getElementById("decompressDirBtn");
const removeOriginalEl = document.getElementById("removeOriginal");
const diskSpaceEl = document.getElementById("diskSpace");

let files = [];
let index = 0;
let busy = false;
let previewToken = 0;
let batchProgress = null;
let stopRequested = false;

try {
  removeOriginalEl.checked = localStorage.getItem("arwc-remove-original") === "1";
} catch {
  /* ignore */
}
removeOriginalEl.addEventListener("change", () => {
  try {
    localStorage.setItem("arwc-remove-original", removeOriginalEl.checked ? "1" : "0");
  } catch {
    /* ignore */
  }
});

function current() {
  return files[index] || null;
}

function setDetails(text, title) {
  detailsEl.textContent = text;
  if (title) detailsEl.title = title;
  else detailsEl.removeAttribute("title");
}

function compressionTitle(info, file) {
  if (!file || !file.encoded || !info || !info.orig_bytes) return "";
  const orig = Number(info.orig_bytes);
  if (!(orig > 0) || !file.size) return "";
  const ratio = orig / file.size;
  const pct = (100 * file.size) / orig;
  return `${ratio.toFixed(2)}:1 · ${pct.toFixed(1)}% of original`;
}

function fmtSize(n) {
  const v = Number(n) || 0;
  if (v < 1024) return `${v} B`;
  if (v < 1024 * 1024) return `${(v / 1024).toFixed(1)} KB`;
  if (v < 1024 * 1024 * 1024) return `${(v / (1024 * 1024)).toFixed(1)} MB`;
  return `${(v / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function photoStem(name) {
  const n = String(name).toLowerCase();
  if (n.endsWith(".arwc.jpg")) return n.slice(0, -".arwc.jpg".length);
  if (n.endsWith(".arw")) return n.slice(0, -".arw".length);
  return n;
}

function folderDiskStats(list) {
  const groups = new Map();
  for (const file of list) {
    const stem = photoStem(file.name);
    let group = groups.get(stem);
    if (!group) {
      group = { arw: null, enc: null };
      groups.set(stem, group);
    }
    if (file.encoded) group.enc = file;
    else group.arw = file;
  }
  let occupied = 0;
  let original = 0;
  for (const group of groups.values()) {
    if (group.arw) occupied += group.arw.size;
    if (group.enc) occupied += group.enc.size;
    const orig =
      (group.enc && Number(group.enc.orig_bytes)) ||
      (group.arw && group.arw.size) ||
      0;
    original += orig;
  }
  return { saved: Math.max(0, original - occupied), occupied };
}

function renderDiskSpace() {
  if (!files.length) {
    diskSpaceEl.textContent = "Saved — / Occupied —";
    return;
  }
  const { saved, occupied } = folderDiskStats(files);
  diskSpaceEl.textContent = `Saved ${fmtSize(saved)} / Occupied ${fmtSize(occupied)}`;
}

function setStatus(text) {
  if (!text) {
    statusEl.hidden = true;
    statusEl.textContent = "";
    return;
  }
  statusEl.hidden = false;
  statusEl.textContent = text;
}

function setProgress(pct) {
  if (pct == null) {
    progressEl.hidden = true;
    progressFill.style.width = "0%";
    return;
  }
  const n = Math.max(0, Math.min(100, Math.round(pct)));
  progressEl.hidden = false;
  statusEl.hidden = true;
  progressFill.style.width = `${n}%`;
  progressPct.textContent = `${n}%`;
  progressEl.setAttribute("aria-valuenow", String(n));
}

function renderChrome() {
  const file = current();
  const has = files.length > 0;
  renderDiskSpace();
  emptyEl.hidden = has;
  prevBtn.disabled = !has || busy;
  nextBtn.disabled = !has || busy;
  if (!file) {
    filenameEl.textContent = "—";
    setDetails("", "");
    counter.textContent = "";
    compressBtn.disabled = true;
    decompressBtn.disabled = true;
    compressDirBtn.disabled = true;
    decompressDirBtn.disabled = true;
    compressDirBtn.textContent = "Compress directory (0 files)";
    decompressDirBtn.textContent = "Decompress directory (0 files)";
    previewEl.hidden = true;
    previewEl.removeAttribute("src");
    previewEl.removeAttribute("data-orient");
    return;
  }
  counter.textContent = `${index + 1} / ${files.length}`;
  filenameEl.textContent = file.name;
  const kind = file.encoded ? "ARWC JPEG" : "ARW";
  setDetails(`${kind} · ${fmtSize(file.size)}`);
  compressBtn.disabled = busy || file.encoded;
  decompressBtn.disabled = busy || !file.encoded;
  const rawN = files.filter((f) => !f.encoded).length;
  const encN = files.filter((f) => f.encoded).length;
  compressDirBtn.textContent = `Compress directory (${rawN} file${rawN === 1 ? "" : "s"})`;
  decompressDirBtn.textContent = `Decompress directory (${encN} file${encN === 1 ? "" : "s"})`;
  compressDirBtn.disabled = busy || rawN === 0;
  decompressDirBtn.disabled = busy || encN === 0;
}

function applyOrientation(orient) {
  const o = Number(orient);
  if (!o || o < 2 || o > 8) {
    previewEl.removeAttribute("data-orient");
    return;
  }
  previewEl.setAttribute("data-orient", String(o));
}

function displaySize(info) {
  let w = info.width;
  let h = info.height;
  if (info.orientation >= 5 && info.orientation <= 8) {
    [w, h] = [h, w];
  }
  return `${w}×${h}`;
}

async function showCurrent(opts = {}) {
  renderChrome();
  const file = current();
  if (!file) return;
  const token = ++previewToken;
  if (!opts.keepProgress) setStatus("Loading preview…");
  try {
    const [dataUrl, info] = await Promise.all([
      window.arwc.preview(file.path),
      window.arwc.inspect(file.path).catch(() => null),
    ]);
    if (token !== previewToken) return;
    previewEl.hidden = false;
    previewEl.removeAttribute("src");
    previewEl.src = dataUrl;
    applyOrientation(info && info.orientation);
    if (info) {
      const kind = file.encoded ? "ARWC JPEG" : "ARW";
      setDetails(
        `${kind} · ${displaySize(info)} · ${fmtSize(file.size)}`,
        compressionTitle(info, file)
      );
    }
    if (!opts.keepProgress) setStatus("");
  } catch (err) {
    if (token !== previewToken) return;
    if (opts.keepProgress || isStopped(err)) return;
    previewEl.hidden = true;
    previewEl.removeAttribute("data-orient");
    setStatus(String(err.message || err));
  }
}

function samePath(a, b) {
  return (
    String(a).replace(/\\/g, "/").toLowerCase() === String(b).replace(/\\/g, "/").toLowerCase()
  );
}

function applyListing(payload, preferredPath) {
  files = payload.files || [];
  folderLabel.textContent = payload.folder || "Open a folder of .ARW / .ARWC.JPG files";
  let next = Number.isInteger(payload.index) ? payload.index : 0;
  if (preferredPath) {
    const i = files.findIndex((f) => samePath(f.path, preferredPath));
    if (i >= 0) next = i;
  }
  index = Math.min(Math.max(next, 0), Math.max(files.length - 1, 0));
  return showCurrent();
}

function step(delta) {
  if (!files.length || busy) return;
  index = (index + delta + files.length) % files.length;
  showCurrent();
}

async function withBusy(fn) {
  if (busy) return;
  busy = true;
  stopRequested = false;
  renderChrome();
  try {
    await fn();
  } catch (err) {
    if (!isStopped(err)) setStatus(String(err.message || err));
  } finally {
    busy = false;
    stopRequested = false;
    renderChrome();
  }
}

openBtn.addEventListener("click", () => window.arwc.openFolder());
prevBtn.addEventListener("click", () => step(-1));
nextBtn.addEventListener("click", () => step(1));

function isStopped(err) {
  if (stopRequested) return true;
  const msg = String((err && err.message) || err || "").toLowerCase();
  return msg === "stopped" || msg.includes("stopped");
}

function requestStop() {
  if (!busy) return;
  stopRequested = true;
  window.arwc.stop();
}

async function showTarget(file) {
  const i = files.findIndex((f) => samePath(f.path, file.path));
  if (i >= 0) index = i;
  await showCurrent({ keepProgress: true });
}

function removeOriginal() {
  return removeOriginalEl.checked;
}

async function afterWrite(out, verb, name) {
  const listing = await window.arwc.refresh(out);
  await applyListing(listing, out);
  setProgress(null);
  setStatus(`${verb} ${name}`);
}

compressBtn.addEventListener("click", () =>
  withBusy(async () => {
    const file = current();
    if (!file || file.encoded) return;
    batchProgress = { index: 0, total: 1 };
    progressLabel.textContent = "Compressing…";
    setProgress(0);
    try {
      const out = await window.arwc.compress(file.path, {
        removeOriginal: removeOriginal(),
      });
      if (stopRequested) return;
      setProgress(100);
      await afterWrite(out, "Wrote", out.split(/[/\\]/).pop());
    } catch (err) {
      setProgress(null);
      if (isStopped(err)) {
        setStatus("Stopped");
        return;
      }
      throw err;
    } finally {
      batchProgress = null;
      if (!progressEl.hidden) setProgress(null);
    }
  })
);

decompressBtn.addEventListener("click", () =>
  withBusy(async () => {
    const file = current();
    if (!file || !file.encoded) return;
    setStatus("Decompressing…");
    const out = await window.arwc.decompress(file.path, {
      removeOriginal: removeOriginal(),
    });
    if (!out) {
      setStatus("");
      return;
    }
    await afterWrite(out, "Wrote", `${out.split(/[/\\]/).pop()} · SHA-1 verified`);
  })
);

compressDirBtn.addEventListener("click", () => compressDirectory());
decompressDirBtn.addEventListener("click", () => decompressDirectory());

async function compressDirectory() {
  const targets = files.filter((f) => !f.encoded);
  if (!targets.length) return;
  const n = targets.length;
  const extra = removeOriginal() ? " Original .ARW files will be deleted." : "";
  const ok = await window.arwc.confirm(
    `Compress ${n} file${n === 1 ? "" : "s"} to .ARWC.JPG?${extra}`
  );
  if (!ok) return;
  await withBusy(async () => {
    let last = null;
    let done = 0;
    batchProgress = { index: 0, total: n };
    setProgress(0);
    try {
      for (let i = 0; i < n; i++) {
        if (stopRequested) break;
        batchProgress = { index: i, total: n };
        progressLabel.textContent = `Compressing ${i + 1} / ${n}`;
        await showTarget(targets[i]);
        if (stopRequested) break;
        last = await window.arwc.compress(targets[i].path, {
          removeOriginal: removeOriginal(),
        });
        done = i + 1;
      }
      if (stopRequested) {
        const listing = await window.arwc.refresh(last || targets[0].path);
        await applyListing(listing, last || targets[0].path);
        setProgress(null);
        setStatus(`Stopped after ${done} of ${n}`);
        return;
      }
      setProgress(100);
      await afterWrite(
        last,
        "Wrote",
        `${n} .ARWC.JPG file${n === 1 ? "" : "s"}`
      );
    } catch (err) {
      setProgress(null);
      if (last) {
        const listing = await window.arwc.refresh(last);
        await applyListing(listing, last);
      }
      if (isStopped(err)) {
        setStatus(`Stopped after ${done} of ${n}`);
        return;
      }
      throw err;
    } finally {
      batchProgress = null;
      if (!progressEl.hidden) setProgress(null);
    }
  });
}

async function decompressDirectory() {
  const targets = files.filter((f) => f.encoded);
  if (!targets.length) return;
  const n = targets.length;
  const extra = removeOriginal() ? " Original .ARWC.JPG files will be deleted." : "";
  const ok = await window.arwc.confirm(
    `Decompress ${n} file${n === 1 ? "" : "s"} to .ARW? Existing .ARW files will be overwritten.${extra}`
  );
  if (!ok) return;
  await withBusy(async () => {
    let last = null;
    let done = 0;
    batchProgress = { index: 0, total: n };
    progressLabel.textContent = `Decompressing 1 / ${n}`;
    setProgress(0);
    try {
      for (let i = 0; i < n; i++) {
        if (stopRequested) break;
        batchProgress = { index: i, total: n };
        progressLabel.textContent = `Decompressing ${i + 1} / ${n}`;
        setProgress((i / n) * 100);
        await showTarget(targets[i]);
        if (stopRequested) break;
        const out = await window.arwc.decompress(targets[i].path, {
          removeOriginal: removeOriginal(),
          overwrite: true,
        });
        if (out) last = out;
        done = i + 1;
      }
      if (stopRequested) {
        const listing = await window.arwc.refresh(last || targets[0].path);
        await applyListing(listing, last || targets[0].path);
        setProgress(null);
        setStatus(`Stopped after ${done} of ${n}`);
        return;
      }
      if (!last) {
        setProgress(null);
        setStatus("");
        return;
      }
      setProgress(100);
      await afterWrite(last, "Wrote", `${n} .ARW file${n === 1 ? "" : "s"} · SHA-1 verified`);
    } catch (err) {
      setProgress(null);
      if (last) {
        const listing = await window.arwc.refresh(last);
        await applyListing(listing, last);
      }
      if (isStopped(err)) {
        setStatus(`Stopped after ${done} of ${n}`);
        return;
      }
      throw err;
    } finally {
      batchProgress = null;
      if (!progressEl.hidden) setProgress(null);
    }
  });
}

window.arwc.onFolderOpened((payload) => applyListing(payload));
window.arwc.onCompressProgress((pct) => {
  if (progressEl.hidden) return;
  if (batchProgress && batchProgress.total > 1) {
    setProgress(((batchProgress.index + pct / 100) / batchProgress.total) * 100);
  } else {
    setProgress(pct);
  }
});
window.arwc.onCompressDir(() => compressDirectory());
window.arwc.onDecompressDir(() => decompressDirectory());

stopBtn.addEventListener("click", (e) => {
  e.preventDefault();
  e.stopPropagation();
  requestStop();
});

window.addEventListener("keydown", (e) => {
  if (e.target && ["INPUT", "TEXTAREA"].includes(e.target.tagName)) return;
  if (e.key === "ArrowLeft") {
    e.preventDefault();
    step(-1);
  } else if (e.key === "ArrowRight") {
    e.preventDefault();
    step(1);
  } else if (e.key === "Escape") {
    if (busy) {
      e.preventDefault();
      requestStop();
    }
  } else if (e.key === "c" || e.key === "C") {
    if (busy) return;
    e.preventDefault();
    compressBtn.click();
  } else if (e.key === "d" || e.key === "D") {
    if (busy) return;
    e.preventDefault();
    decompressBtn.click();
  } else if (e.key === "o" || e.key === "O") {
    if (!(e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      openBtn.click();
    }
  }
});
