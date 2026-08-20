const { app, BrowserWindow, dialog, ipcMain, Menu, shell } = require("electron");
const { spawn } = require("child_process");
const crypto = require("crypto");
const fs = require("fs");
const os = require("os");
const path = require("path");

let win;
let folder = null;
let currentChild = null;

function sha1File(filePath) {
  return crypto.createHash("sha1").update(fs.readFileSync(filePath)).digest("hex");
}

function verifyUncompressed(outPath, info) {
  const size = fs.statSync(outPath).size;
  if (info && info.orig_bytes != null && Number(info.orig_bytes) !== size) {
    throw new Error(`uncompressed size mismatch: expected ${info.orig_bytes}, got ${size}`);
  }
  if (info && info.orig_sha1) {
    const got = sha1File(outPath);
    const want = String(info.orig_sha1).toLowerCase();
    if (got !== want) {
      throw new Error(`SHA-1 mismatch: expected ${want}, got ${got}`);
    }
  }
}

function rustBin() {
  const exe = process.platform === "win32" ? "compress-arw.exe" : "compress-arw";
  const candidates = [];
  if (app.isPackaged) {
    candidates.push(path.join(process.resourcesPath, exe));
    candidates.push(path.join(path.dirname(process.execPath), exe));
  }
  const root = path.join(__dirname, "..");
  candidates.push(
    path.join(root, "target", "release", exe),
    path.join(root, "target", "debug", exe)
  );
  if (process.env.CARGO_TARGET_DIR) {
    candidates.push(
      path.join(process.env.CARGO_TARGET_DIR, "release", exe),
      path.join(process.env.CARGO_TARGET_DIR, "debug", exe)
    );
  }
  for (const bin of candidates) {
    if (fs.existsSync(bin)) return bin;
  }
  throw new Error("compress-arw binary not found. Run cargo build --release");
}

function runCli(args, onProgress) {
  return new Promise((resolve, reject) => {
    const child = spawn(rustBin(), args);
    currentChild = child;
    let stdout = "";
    let stderr = "";
    let errBuf = "";
    const takeLines = (chunk, handle) => {
      errBuf += chunk;
      const lines = errBuf.split(/\r?\n/);
      errBuf = lines.pop() || "";
      for (const line of lines) handle(line);
    };
    const onErrLine = (line) => {
      const m = /^progress (\d{1,3})\s*$/.exec(line);
      if (m && onProgress) {
        onProgress(Math.min(100, Number(m[1])));
        return;
      }
      stderr += line + "\n";
    };
    child.stdout.on("data", (d) => {
      stdout += d;
    });
    child.stderr.on("data", (d) => takeLines(d.toString(), onErrLine));
    child.on("error", (err) => {
      if (currentChild === child) currentChild = null;
      reject(err);
    });
    child.on("close", (code, signal) => {
      if (currentChild === child) currentChild = null;
      if (errBuf) onErrLine(errBuf);
      if (code === 0) resolve({ stdout, stderr });
      else if (signal) reject(Object.assign(new Error("stopped"), { stopped: true }));
      else reject(new Error(stderr.trim() || stdout.trim() || `exit ${code}`));
    });
  });
}

function stopCli() {
  if (!currentChild) return false;
  const child = currentChild;
  try {
    child.kill("SIGTERM");
  } catch {
    /* already gone */
  }
  setTimeout(() => {
    try {
      child.kill("SIGKILL");
    } catch {
      /* already gone */
    }
  }, 300);
  return true;
}

function isPhotoName(name) {
  const n = name.toLowerCase();
  return n.endsWith(".arwc.jpg") || n.endsWith(".arw");
}

function samePath(a, b) {
  return (
    String(a).replace(/\\/g, "/").toLowerCase() === String(b).replace(/\\/g, "/").toLowerCase()
  );
}

function listedPath(filePath) {
  const dir = path.dirname(filePath);
  const base = path.basename(filePath).toLowerCase();
  try {
    const hit = fs.readdirSync(dir).find((name) => name.toLowerCase() === base);
    if (hit) return path.join(dir, hit);
  } catch {
    /* fall through */
  }
  return filePath;
}

function findPhotoIndex(files, selectPath) {
  if (!selectPath) return -1;
  return files.findIndex((f) => samePath(f.path, selectPath));
}

const FOOTER_LEN = 32;
const FOOTER_MAGIC = "ARWH";

function readOrigBytes(filePath, fileSize) {
  if (fileSize < FOOTER_LEN) return null;
  const buf = Buffer.alloc(FOOTER_LEN);
  const fd = fs.openSync(filePath, "r");
  try {
    fs.readSync(fd, buf, 0, FOOTER_LEN, fileSize - FOOTER_LEN);
  } finally {
    fs.closeSync(fd);
  }
  if (buf.toString("latin1", 28, 32) !== FOOTER_MAGIC) return null;
  const n = Number(buf.readBigUInt64LE(0));
  return Number.isFinite(n) && n > 0 ? n : null;
}

function listPhotos(dir) {
  return fs
    .readdirSync(dir)
    .filter(isPhotoName)
    .sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }))
    .map((name) => {
      const filePath = path.join(dir, name);
      const st = fs.statSync(filePath);
      const encoded = name.toLowerCase().endsWith(".arwc.jpg");
      return {
        name,
        path: filePath,
        size: st.size,
        encoded,
        orig_bytes: encoded ? readOrigBytes(filePath, st.size) : st.size,
        mtimeMs: st.mtimeMs,
      };
    });
}

function createWindow() {
  win = new BrowserWindow({
    width: 1100,
    height: 780,
    minWidth: 860,
    minHeight: 520,
    backgroundColor: "#121212",
    title: "ARWC",
    icon: path.join(__dirname, "build", "icon.png"),
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  win.loadFile(path.join(__dirname, "index.html"));
}

function setFolder(dir, selectPath) {
  folder = dir;
  const files = listPhotos(dir);
  let index = 0;
  if (selectPath) {
    const i = findPhotoIndex(files, selectPath);
    if (i >= 0) index = i;
  }
  win.webContents.send("folder-opened", { folder, files, index });
}

async function openFolder() {
  const res = await dialog.showOpenDialog(win, {
    title: "Open folder",
    properties: ["openDirectory"],
  });
  if (res.canceled || !res.filePaths[0]) return;
  setFolder(res.filePaths[0]);
}

async function openFile() {
  const res = await dialog.showOpenDialog(win, {
    title: "Open ARW or ARWC",
    properties: ["openFile"],
    filters: [
      { name: "ARW / ARWC", extensions: ["arw", "ARW", "jpg", "JPG"] },
      { name: "All files", extensions: ["*"] },
    ],
  });
  if (res.canceled || !res.filePaths[0]) return;
  const filePath = res.filePaths[0];
  if (!isPhotoName(path.basename(filePath))) {
    await dialog.showErrorBox("Unsupported file", "Open an .ARW or .ARWC.JPG file.");
    return;
  }
  setFolder(path.dirname(filePath), filePath);
}

app.whenReady().then(() => {
  const isMac = process.platform === "darwin";
  Menu.setApplicationMenu(
    Menu.buildFromTemplate([
      ...(isMac ? [{ role: "appMenu" }] : []),
      {
        label: "File",
        submenu: [
          { label: "Open Folder…", accelerator: "CmdOrCtrl+O", click: () => openFolder() },
          { label: "Open File…", accelerator: "CmdOrCtrl+Shift+O", click: () => openFile() },
          { type: "separator" },
          {
            label: "Compress Directory",
            click: () => {
              if (win && !win.isDestroyed()) win.webContents.send("run-compress-dir");
            },
          },
          {
            label: "Decompress Directory",
            click: () => {
              if (win && !win.isDestroyed()) win.webContents.send("run-decompress-dir");
            },
          },
          { type: "separator" },
          isMac ? { role: "close" } : { role: "quit" },
        ],
      },
      { role: "editMenu" },
      { role: "viewMenu" },
      { role: "windowMenu" },
    ])
  );
  createWindow();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
app.on("activate", () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow();
});

ipcMain.handle("open-folder", () => openFolder());
ipcMain.handle("open-file", () => openFile());
ipcMain.handle("reveal", (_e, filePath) => shell.showItemInFolder(filePath));

ipcMain.handle("refresh", (_e, selectPath) => {
  if (!folder) return { folder: null, files: [], index: 0 };
  const files = listPhotos(folder);
  let index = 0;
  if (selectPath) {
    const i = findPhotoIndex(files, selectPath);
    if (i >= 0) index = i;
  }
  return { folder, files, index };
});

ipcMain.handle("stop", () => stopCli());

ipcMain.handle("confirm", async (_e, message) => {
  const { response } = await dialog.showMessageBox(win, {
    type: "question",
    buttons: ["Cancel", "Continue"],
    defaultId: 1,
    cancelId: 0,
    message,
  });
  return response === 1;
});

ipcMain.handle("inspect", async (_e, filePath) => {
  const { stdout } = await runCli(["info", "--json", filePath]);
  return JSON.parse(stdout);
});

ipcMain.handle("preview", async (_e, filePath) => {
  const tmp = path.join(
    os.tmpdir(),
    `arwc-preview-${path.basename(filePath)}-${fs.statSync(filePath).mtimeMs}.jpg`
  );
  if (!fs.existsSync(tmp)) {
    await runCli(["preview", filePath, "-o", tmp]);
  }
  const buf = fs.readFileSync(tmp);
  return `data:image/jpeg;base64,${buf.toString("base64")}`;
});

ipcMain.handle("compress", async (_e, filePath, opts = {}) => {
  await runCli(["encode", "--progress", filePath], (pct) => {
    if (win && !win.isDestroyed()) win.webContents.send("compress-progress", pct);
  });
  const out = listedPath(filePath.replace(/\.arw$/i, ".ARWC.JPG"));
  if (!fs.existsSync(out)) {
    throw new Error("encode finished but output was not found");
  }
  if (opts.removeOriginal && !samePath(filePath, out) && fs.existsSync(filePath)) {
    fs.unlinkSync(filePath);
  }
  return out;
});

ipcMain.handle("decompress", async (_e, filePath, opts = {}) => {
  const guessed = filePath.replace(/\.arwc\.jpg$/i, ".ARW");
  const dest = fs.existsSync(guessed) ? listedPath(guessed) : guessed;
  if (fs.existsSync(dest) && !opts.overwrite) {
    const { response } = await dialog.showMessageBox(win, {
      type: "question",
      buttons: ["Overwrite", "Cancel"],
      defaultId: 0,
      cancelId: 1,
      message: `${path.basename(dest)} already exists. Overwrite?`,
    });
    if (response !== 0) return null;
  }
  const info = JSON.parse((await runCli(["info", "--json", filePath])).stdout);
  await runCli(["decode", filePath, "-o", dest]);
  const out = listedPath(dest);
  verifyUncompressed(out, info);
  if (opts.removeOriginal && !samePath(filePath, out) && fs.existsSync(filePath)) {
    fs.unlinkSync(filePath);
  }
  return out;
});
