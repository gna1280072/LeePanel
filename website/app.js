/* LeePanel website — language toggle, scroll effects, fade-in */

(function () {
  'use strict';

  const STORAGE_KEY = 'leepanel-lang';

  /* ---- Language switching ---- */
  function setLang(lang) {
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en';
    document.querySelectorAll('[data-zh]').forEach(function (el) {
      el.innerHTML = el.getAttribute('data-' + lang);
    });
    try { localStorage.setItem(STORAGE_KEY, lang); } catch (e) {}
  }

  function initLang() {
    var saved = null;
    try { saved = localStorage.getItem(STORAGE_KEY); } catch (e) {}
    var lang = saved || 'en';
    setLang(lang);

    var btn = document.getElementById('langToggle');
    if (btn) {
      btn.addEventListener('click', function () {
        setLang(btn.textContent.trim() === 'EN' ? 'en' : 'zh');
      });
    }
  }

  /* ---- Fade-in on scroll ---- */
  function initFadeIn() {
    var els = document.querySelectorAll('.fade-in');
    if (!('IntersectionObserver' in window)) {
      els.forEach(function (el) { el.classList.add('visible'); });
      return;
    }
    var obs = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting) {
          e.target.classList.add('visible');
          obs.unobserve(e.target);
        }
      });
    }, { threshold: 0.15 });
    els.forEach(function (el) { obs.observe(el); });
  }

  /* ---- Mobile hamburger ---- */
  function initHamburger() {
    var btn = document.getElementById('hamburger');
    var links = document.querySelector('.nav-links');
    if (!btn || !links) return;
    btn.addEventListener('click', function () {
      links.classList.toggle('open');
    });
    // close on link click
    links.querySelectorAll('a').forEach(function (a) {
      a.addEventListener('click', function () { links.classList.remove('open'); });
    });
  }

  /* ---- Fetch downloads from update.json ---- */
  function fetchDownloads() {
    var UPDATE_URL = 'https://raw.githubusercontent.com/gna1280072/LeePanel/gh-pages/update.json';
    fetch(UPDATE_URL)
      .then(function (r) { return r.json(); })
      .then(function (data) {
        // Update version display
        var verEl = document.getElementById('dlVersion');
        if (verEl && data.version) {
          var v = 'v' + data.version;
          verEl.setAttribute('data-zh', '最新版本：' + v);
          verEl.setAttribute('data-en', 'Latest: ' + v);
          // Apply current language
          var lang = document.documentElement.lang === 'zh-CN' ? 'zh' : 'en';
          verEl.textContent = verEl.getAttribute('data-' + lang);
        }
        // Map platform keys to download buttons
        var urls = {};
        if (data.platforms) {
          urls['windows'] = data.platforms['windows-x86_64'] ? data.platforms['windows-x86_64'].url : '';
          urls['macos-intel'] = data.platforms['darwin-x86_64'] ? data.platforms['darwin-x86_64'].url : '';
          urls['macos-arm'] = data.platforms['darwin-aarch64'] ? data.platforms['darwin-aarch64'].url : '';
          urls['linux'] = data.platforms['linux-x86_64'] ? data.platforms['linux-x86_64'].url : '';
          // Windows zip: derive from exe URL by replacing .exe with .zip
          if (urls['windows']) {
            urls['windows-zip'] = urls['windows'].replace(/\.exe$/i, '.zip');
          }
        }
        document.querySelectorAll('.dl-file-btn[data-platform]').forEach(function (btn) {
          var key = btn.getAttribute('data-platform');
          if (urls[key]) btn.href = urls[key];
        });
      })
      .catch(function () { /* silent fail — links stay as # */ });
  }

  /* ---- Init ---- */
  document.addEventListener('DOMContentLoaded', function () {
    initLang();
    initFadeIn();
    initHamburger();
    fetchDownloads();
    initCookieBanner();
  });

  /* ---- Cookie consent ---- */
  function initCookieBanner() {
    var banner = document.getElementById('cookieBanner');
    if (!banner) return;
    var consent = null;
    try { consent = localStorage.getItem('cookie-consent'); } catch (e) {}
    if (!consent) {
      banner.hidden = false;
    }
    var acceptBtn = document.getElementById('cookieAccept');
    var declineBtn = document.getElementById('cookieDecline');
    if (acceptBtn) {
      acceptBtn.addEventListener('click', function () {
        try { localStorage.setItem('cookie-consent', 'accepted'); } catch (e) {}
        banner.hidden = true;
      });
    }
    if (declineBtn) {
      declineBtn.addEventListener('click', function () {
        try { localStorage.setItem('cookie-consent', 'declined'); } catch (e) {}
        banner.hidden = true;
      });
    }
  }
})();
