/* LeePanel website2 — homepage interactions (nav/reveal live in site.js) */
(function () {
  "use strict";

  /* ---------- live clock in terminal bar ---------- */
  var clock = document.getElementById("termClock");
  var tick = function () {
    var d = new Date();
    var p = function (n) { return String(n).padStart(2, "0"); };
    clock.textContent = p(d.getHours()) + ":" + p(d.getMinutes()) + ":" + p(d.getSeconds());
  };
  tick();
  setInterval(tick, 1000);

  /* ---------- hero terminal typing loop ---------- */
  var termBody = document.getElementById("termBody");
  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // [cssClass, text] segments per line; "$ " prompt auto-prefixed on cmd lines
  var script = [
    { type: "cmd", text: "leepanel connect prod-01 --key ~/.ssh/id_ed25519" },
    { type: "out", html: '<span class="t-info">→</span> resolving prod-01.example.com …' },
    { type: "out", html: '<span class="t-ok">✓</span> ssh session established <span class="t-dim">(ed25519, 42 ms)</span>' },
    { type: "cmd", text: "systemctl status nginx mysql php8.1-fpm" },
    { type: "out", html: '<span class="t-ok">●</span> nginx.service      <span class="t-dim">active (running)</span>  <span class="t-ok">since 14 days ago</span>' },
    { type: "out", html: '<span class="t-ok">●</span> mysql.service      <span class="t-dim">active (running)</span>  <span class="t-ok">since 14 days ago</span>' },
    { type: "out", html: '<span class="t-ok">●</span> php8.1-fpm.service <span class="t-dim">active (running)</span>  <span class="t-ok">since 3 days ago</span>' },
    { type: "cmd", text: "leepanel monitor --live" },
    { type: "out", html: '<span class="t-info">cpu</span> 3%   <span class="t-info">mem</span> 667M / 3.8G   <span class="t-info">net</span> ↓ 9.1G ↑ 12.6M' },
    { type: "out", html: '<span class="t-ok">✓</span> streaming metrics every 5 s <span class="t-dim">— zero agents installed</span>' },
  ];

  function makeLine() {
    var div = document.createElement("div");
    div.className = "t-line";
    return div;
  }

  function runTerminal() {
    termBody.innerHTML = "";
    var li = 0;

    function nextLine() {
      if (li >= script.length) {
        // idle prompt with blinking cursor, then restart
        var idle = makeLine();
        idle.innerHTML = '<span class="t-prompt">$ </span><span class="cursor"></span>';
        termBody.appendChild(idle);
        setTimeout(runTerminal, 5200);
        return;
      }
      var step = script[li++];
      var line = makeLine();
      termBody.appendChild(line);

      if (step.type === "cmd") {
        var prompt = document.createElement("span");
        prompt.className = "t-prompt";
        prompt.textContent = "$ ";
        var cmd = document.createElement("span");
        cmd.className = "t-cmd";
        var caret = document.createElement("span");
        caret.className = "cursor";
        line.appendChild(prompt);
        line.appendChild(cmd);
        line.appendChild(caret);

        if (reduced) {
          cmd.textContent = step.text;
          caret.remove();
          setTimeout(nextLine, 120);
          return;
        }
        var ci = 0;
        (function typeChar() {
          if (ci < step.text.length) {
            cmd.textContent += step.text[ci++];
            setTimeout(typeChar, 16 + Math.random() * 34);
          } else {
            caret.remove();
            setTimeout(nextLine, 340);
          }
        })();
      } else {
        line.innerHTML = step.html;
        setTimeout(nextLine, reduced ? 120 : 260);
      }
    }
    nextLine();
  }
  runTerminal();

  /* ---------- screenshot showcase tabs ---------- */
  var tabs = document.querySelectorAll(".showcase-tab");
  var imgs = document.querySelectorAll(".frame-img");
  var frameUrl = document.getElementById("frameUrl");
  var zh = document.documentElement.lang === "zh-CN";
  var urls = zh ? [
    "LeePanel — 文件 · /etc",
    "LeePanel — 软件仓库",
    "LeePanel — 监控 · prod-01",
  ] : [
    "LeePanel — Files · /etc",
    "LeePanel — Software Repository",
    "LeePanel — Monitor · prod-01",
  ];
  var autoTimer = null;
  var current = 0;

  function showScreen(i) {
    current = i;
    tabs.forEach(function (t, k) {
      t.classList.toggle("is-active", k === i);
      t.setAttribute("aria-selected", k === i ? "true" : "false");
    });
    imgs.forEach(function (img, k) {
      img.classList.toggle("is-active", k === i);
    });
    frameUrl.textContent = urls[i];
  }

  tabs.forEach(function (t) {
    t.addEventListener("click", function () {
      showScreen(Number(t.dataset.screen));
      restartAuto();
    });
  });

  // ponytail: simple 6s auto-rotate, resets on manual click; ceiling = none worth upgrading
  function restartAuto() {
    clearInterval(autoTimer);
    autoTimer = setInterval(function () {
      showScreen((current + 1) % imgs.length);
    }, 6000);
  }
  restartAuto();

  /* ---------- latest version + direct links from the release feed ---------- */
  var FEED = "https://raw.githubusercontent.com/gna1280072/LeePanel/gh-pages/update.json";
  fetch(FEED, { cache: "no-store" })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (data) {
      if (!data || !data.version) return;
      var v = data.version;
      var p = data.platforms || {};
      var hero = document.getElementById("heroVersion");
      var dlWin = document.getElementById("dlWin");
      var dlMac = document.getElementById("dlMac");
      if (hero) hero.textContent = v;
      if (dlWin && p["windows-x86_64"]) {
        dlWin.href = p["windows-x86_64"].url;
        document.getElementById("dlVerWin").textContent = "v" + v;
      }
      if (dlMac && p["darwin-aarch64"]) {
        dlMac.href = p["darwin-aarch64"].url;
        document.getElementById("dlVerMac").textContent = "v" + v;
      }
    })
    .catch(function () { /* offline / rate-limited: keep "latest" fallback */ });
})();
