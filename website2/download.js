/* LeePanel download page — renders builds from the live update manifest */
(function () {
  "use strict";

  var FEED = "https://raw.githubusercontent.com/gna1280072/LeePanel/gh-pages/update.json";
  var RELEASES = "https://github.com/gna1280072/LeePanel/releases";

  // ponytail: one flat dictionary picked by <html lang> — no framework needed
  var zh = document.documentElement.lang === "zh-CN";
  var T = zh ? {
    thisDevice: "本机推荐",
    signed: "已签名", unsigned: "未签名",
    signedTip: "清单中包含更新签名", unsignedTip: "该平台无更新签名",
    copy: "复制链接", copied: "已复制 ✓",
    download: "下载",
    fetching: "正在获取实时清单…",
    macHint: "检测到 <strong>macOS</strong>——Apple Silicon 芯片选第一个 .dmg，Intel 芯片选第二个。",
    detected: "检测到本机系统为 <strong>", detectedTail: "</strong>——已为你高亮推荐版本。",
    noDetect: "未能识别你的系统——以下列出全部版本。",
    released: "发布于 ",
    errText: "发布通道没有响应。请检查网络后重试。",
    retry: "重试", fallback: "打开 GitHub Releases →", errHint: "通道不可达——已显示备用入口"
  } : {
    thisDevice: "this device",
    signed: "signed", unsigned: "unsigned",
    signedTip: "Update signature present in manifest", unsignedTip: "No update signature for this platform",
    copy: "copy link", copied: "copied ✓",
    download: "Download",
    fetching: "fetching live manifest…",
    macHint: 'Detected <strong>macOS</strong> — Apple Silicon? Take the first .dmg; Intel Macs take the second.',
    detected: 'Detected <strong>', detectedTail: "</strong> on this machine — recommended build highlighted.",
    noDetect: "Couldn't detect your platform — every build is listed below.",
    released: "Released ",
    errText: "The release feed didn't answer. Check your connection and try again.",
    retry: "Retry", fallback: "Open GitHub Releases →", errHint: "feed unreachable — fallback shown"
  };

  var ICONS = {
    windows: '<svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor" aria-hidden="true"><path d="M3 5.1 10.6 4v7.4H3V5.1zM11.5 3.9 21 2.5v8.9h-9.5V3.9zM21 12.6v8.9l-9.5-1.3v-7.6H21zM10.6 20 3 18.9v-5.3h7.6V20z"/></svg>',
    apple: '<svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor" aria-hidden="true"><path d="M16.4 12.7c0-2.4 2-3.6 2.1-3.7-1.1-1.7-2.9-1.9-3.5-1.9-1.5-.2-2.9.9-3.7.9-.8 0-1.9-.9-3.2-.8-1.6 0-3.1 1-4 2.4-1.7 3-.4 7.4 1.2 9.8.8 1.2 1.8 2.5 3.1 2.4 1.2-.1 1.7-.8 3.2-.8s1.9.8 3.2.8c1.3 0 2.2-1.2 3-2.4.9-1.4 1.3-2.7 1.3-2.8-.1 0-2.6-1-2.7-3.9zM14 4.6c.7-.8 1.1-1.9 1-3.1-1 0-2.1.7-2.8 1.5-.6.7-1.2 1.9-1 3 1.1.1 2.2-.6 2.8-1.4z"/></svg>',
    linux: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>'
  };

  // order matters: recommended rows render top-down
  var PLATFORMS = [
    { key: "windows-x86_64", os: "Windows", arch: "x86_64", icon: "windows", kind: zh ? "安装程序 · .exe" : "installer · .exe", req: "Windows 10+" },
    { key: "darwin-aarch64", os: "macOS", arch: "Apple Silicon", icon: "apple", kind: zh ? "磁盘映像 · .dmg" : "disk image · .dmg", req: "macOS 11+" },
    { key: "darwin-x86_64", os: "macOS", arch: "Intel", icon: "apple", kind: zh ? "磁盘映像 · .dmg" : "disk image · .dmg", req: "macOS 11+" },
    { key: "linux-x86_64", os: "Linux", arch: "x86_64 / amd64", icon: "linux", kind: zh ? "软件包 · .deb" : "package · .deb", req: "Ubuntu / Debian" }
  ];

  // ponytail: UA sniff is only a hint for the "this device" badge — UA can't
  // reliably split Intel vs Apple Silicon, so macOS highlights both rows.
  var ua = navigator.userAgent;
  var detected = /Windows/i.test(ua) ? ["windows-x86_64"]
    : /Mac OS X/i.test(ua) ? ["darwin-aarch64", "darwin-x86_64"]
    : (/Linux/i.test(ua) && !/Android/i.test(ua)) ? ["linux-x86_64"]
    : [];

  var manifest = document.getElementById("manifest");
  var osHint = document.getElementById("osHint");

  function esc(s) {
    var t = document.createElement("span");
    t.textContent = s == null ? "" : s;
    return t.innerHTML;
  }

  function fileName(url) { return url ? String(url).split("/").pop() : ""; }

  function fmtDate(iso) {
    try {
      return new Date(iso).toLocaleDateString(zh ? "zh-CN" : "en-US", { year: "numeric", month: "short", day: "numeric" });
    } catch (e) { return iso || ""; }
  }

  /* ---------- clipboard with file:// fallback ---------- */
  function copyText(text, btn) {
    var orig = btn.textContent;
    function done() {
      btn.textContent = T.copied;
      btn.classList.add("is-copied");
      setTimeout(function () {
        btn.textContent = orig;
        btn.classList.remove("is-copied");
      }, 1500);
    }
    function fallback() {
      var ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy"); done(); } catch (e) { /* give up quietly */ }
      ta.remove();
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(done, fallback);
    } else {
      fallback();
    }
  }

  document.getElementById("copyFeed").addEventListener("click", function () {
    copyText(document.getElementById("feedUrl").textContent, this);
  });

  /* ---------- skeleton while fetching ---------- */
  function skeleton() {
    manifest.innerHTML = "";
    for (var i = 0; i < 4; i++) {
      var d = document.createElement("div");
      d.className = "m-row m-skel";
      d.style.animationDelay = (i * 60) + "ms";
      d.innerHTML = '<span class="sk sk-icon"></span><span class="sk sk-a"></span><span class="sk sk-b"></span><span class="sk sk-btn"></span>';
      manifest.appendChild(d);
    }
  }

  /* ---------- rows ---------- */
  function renderRows(data) {
    manifest.innerHTML = "";
    var plats = data.platforms || {};

    PLATFORMS.forEach(function (p, i) {
      var info = plats[p.key];
      var url = (info && info.url) || RELEASES;
      var signed = !!(info && info.signature);
      var isRec = detected.indexOf(p.key) !== -1;

      var row = document.createElement("div");
      row.className = "m-row";
      row.style.animationDelay = (i * 80) + "ms";
      row.innerHTML =
        '<div class="m-os">' +
          '<span class="m-icon">' + ICONS[p.icon] + "</span>" +
          '<div class="m-os-txt"><strong>' + esc(p.os) + "</strong><span>" + esc(p.arch) + "</span></div>" +
          (isRec ? '<span class="m-rec">' + T.thisDevice + "</span>" : "") +
        "</div>" +
        '<div class="m-file"><code>' + esc(fileName(url)) + "</code><span>" + esc(p.kind + " · " + p.req) + "</span></div>" +
        '<div class="m-side">' +
          (signed
            ? '<span class="m-badge m-badge--ok" title="' + esc(T.signedTip) + '">' + T.signed + "</span>"
            : '<span class="m-badge" title="' + esc(T.unsignedTip) + '">' + T.unsigned + "</span>") +
          '<button class="copy-btn m-copy" type="button" data-url="' + esc(url) + '">' + T.copy + "</button>" +
          '<a class="btn btn-primary" href="' + esc(url) + '" target="_blank" rel="noopener">' + T.download + "</a>" +
        "</div>";
      manifest.appendChild(row);
    });

    manifest.querySelectorAll(".m-copy").forEach(function (b) {
      b.addEventListener("click", function () { copyText(b.getAttribute("data-url"), b); });
    });

    // hint line
    if (detected.length > 1) {
      osHint.innerHTML = T.macHint;
    } else if (detected.length === 1) {
      var match = PLATFORMS.filter(function (p) { return p.key === detected[0]; })[0];
      osHint.innerHTML = T.detected + esc(match.os) + T.detectedTail;
    } else {
      osHint.textContent = T.noDetect;
    }
  }

  function renderError() {
    manifest.innerHTML =
      '<div class="m-error">' +
        "<p>" + T.errText + "</p>" +
        '<div class="m-error-actions">' +
          '<button class="btn btn-primary" id="retryBtn" type="button">' + T.retry + "</button>" +
          '<a class="btn btn-ghost" href="' + RELEASES + '" target="_blank" rel="noopener">' + T.fallback + "</a>" +
        "</div>" +
      "</div>";
    osHint.textContent = T.errHint;
    document.getElementById("retryBtn").addEventListener("click", load);
  }

  /* ---------- load ---------- */
  function load() {
    skeleton();
    osHint.textContent = T.fetching;

    fetch(FEED, { cache: "no-store" })
      .then(function (r) {
        if (!r.ok) throw new Error("HTTP " + r.status);
        return r.json();
      })
      .then(function (data) {
        if (!data || !data.version) throw new Error("bad manifest");

        document.getElementById("dlVersion").textContent = "v" + data.version;
        document.getElementById("dlDate").textContent = T.released + fmtDate(data.pub_date);
        document.getElementById("dlNotes").textContent = data.notes || "";
        document.getElementById("jsonVersion").textContent = '"' + data.version + '"';
        document.getElementById("jsonDate").textContent = '"' + String(data.pub_date || "").slice(0, 10) + '"';

        renderRows(data);
      })
      .catch(renderError);
  }

  load();
})();
