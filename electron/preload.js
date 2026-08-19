const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("arwc", {
  openFolder: () => ipcRenderer.invoke("open-folder"),
  openFile: () => ipcRenderer.invoke("open-file"),
  reveal: (filePath) => ipcRenderer.invoke("reveal", filePath),
  refresh: (selectPath) => ipcRenderer.invoke("refresh", selectPath),
  inspect: (filePath) => ipcRenderer.invoke("inspect", filePath),
  preview: (filePath) => ipcRenderer.invoke("preview", filePath),
  compress: (filePath, opts) => ipcRenderer.invoke("compress", filePath, opts || {}),
  decompress: (filePath, opts) => ipcRenderer.invoke("decompress", filePath, opts || {}),
  confirm: (message) => ipcRenderer.invoke("confirm", message),
  stop: () => ipcRenderer.invoke("stop"),
  onFolderOpened: (cb) => {
    ipcRenderer.on("folder-opened", (_e, payload) => cb(payload));
  },
  onCompressProgress: (cb) => {
    ipcRenderer.on("compress-progress", (_e, pct) => cb(pct));
  },
  onCompressDir: (cb) => {
    ipcRenderer.on("run-compress-dir", () => cb());
  },
  onDecompressDir: (cb) => {
    ipcRenderer.on("run-decompress-dir", () => cb());
  },
});
