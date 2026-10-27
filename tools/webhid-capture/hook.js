// WebHID capture hook for wootility.io.
// Paste into the DevTools console (or inject via the Claude Chrome extension)
// BEFORE Wootility connects, or call window.__woot.rehook() after.
// Every report is logged as `WOOTCAP {json}` and kept in window.__woot.log.
(() => {
  if (window.__woot) { console.log("WOOTCAP already installed"); return; }
  const hex = (buf) => Array.from(new Uint8Array(buf instanceof ArrayBuffer ? buf : buf.buffer || buf))
    .map((b) => b.toString(16).padStart(2, "0")).join(" ");
  const log = [];
  const emit = (entry) => {
    entry.t = performance.now().toFixed(1);
    entry.label = window.__woot.label;
    log.push(entry);
    console.log("WOOTCAP " + JSON.stringify(entry));
  };
  const P = HIDDevice.prototype;
  const origSend = P.sendReport, origFeat = P.sendFeatureReport, origRecv = P.receiveFeatureReport;
  P.sendReport = function (id, data) {
    emit({ dir: "out", kind: "output", id, len: data.byteLength, hex: hex(data) });
    return origSend.call(this, id, data);
  };
  P.sendFeatureReport = function (id, data) {
    emit({ dir: "out", kind: "feature", id, len: data.byteLength, hex: hex(data) });
    return origFeat.call(this, id, data);
  };
  P.receiveFeatureReport = async function (id) {
    const dv = await origRecv.call(this, id);
    emit({ dir: "in", kind: "feature", id, len: dv.byteLength, hex: hex(dv.buffer) });
    return dv;
  };
  const hookDevice = (d) => {
    if (d.__wootHooked) return;
    d.__wootHooked = true;
    d.addEventListener("inputreport", (e) => {
      emit({ dir: "in", kind: "input", id: e.reportId, len: e.data.byteLength, hex: hex(e.data.buffer) });
    });
  };
  const origGet = navigator.hid.getDevices.bind(navigator.hid);
  navigator.hid.getDevices = async () => { const ds = await origGet(); ds.forEach(hookDevice); return ds; };
  const origReq = navigator.hid.requestDevice.bind(navigator.hid);
  navigator.hid.requestDevice = async (o) => { const ds = await origReq(o); ds.forEach(hookDevice); return ds; };
  window.__woot = {
    log, label: "",
    mark(label) { this.label = label; console.log("WOOTCAP MARK " + label); },
    rehook: async () => (await origGet()).forEach(hookDevice),
    dump: () => JSON.stringify(log),
    clear: () => { log.length = 0; },
  };
  window.__woot.rehook();
  console.log("WOOTCAP installed");
})();
