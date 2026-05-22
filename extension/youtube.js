// FreeIX YouTube Ad Blocker
// Skips video ads, removes overlay ads, and blocks ad-related requests on YouTube.

(function() {
  'use strict';

  // --- Config ---
  const CHECK_INTERVAL = 500; // ms
  const DEBUG = false;

  function log(...args) {
    if (DEBUG) console.log('[FreeIX]', ...args);
  }

  // --- Skip video ads ---
  function skipVideoAd() {
    const video = document.querySelector('video');
    if (!video) return;

    // Check if an ad is playing
    const adOverlay = document.querySelector('.ad-showing');
    if (!adOverlay) return;

    log('Ad detected, skipping...');

    // Fast-forward the ad
    if (video.duration && isFinite(video.duration)) {
      video.currentTime = video.duration;
    }

    // Click "Skip Ad" button if available
    const skipBtn = document.querySelector('.ytp-skip-ad-button, .ytp-ad-skip-button, .ytp-ad-skip-button-modern, [id^="skip-button"]');
    if (skipBtn) {
      skipBtn.click();
      log('Clicked skip button');
    }

    // Mute during ad just in case
    video.muted = true;
  }

  // --- Remove static/overlay ad elements ---
  function removeAdElements() {
    const selectors = [
      // Player ads
      '.ytp-ad-overlay-container',
      '.ytp-ad-text-overlay',
      '.ytp-ad-overlay-slot',
      '.ytp-ad-image-overlay',
      // Banner ads
      '#player-ads',
      '#masthead-ad',
      '#ad_creative_3',
      'ytd-ad-slot-renderer',
      'ytd-banner-promo-renderer',
      'ytd-statement-banner-renderer',
      'ytd-in-feed-ad-layout-renderer',
      'ytd-promoted-sparkles-web-renderer',
      'ytd-display-ad-renderer',
      'ytd-promoted-video-renderer',
      // Sidebar ads
      '#related ytd-ad-slot-renderer',
      // Search ads
      'ytd-search-pyv-renderer',
      // Merch shelf
      'ytd-merch-shelf-renderer',
      // Engagement panels (some are ads)
      'ytd-engagement-panel-section-list-renderer[target-id="engagement-panel-ads"]',
    ];

    selectors.forEach(sel => {
      document.querySelectorAll(sel).forEach(el => {
        el.remove();
        log('Removed:', sel);
      });
    });
  }

  // --- Intercept and neuter ad-related properties ---
  function patchPlayerConfig() {
    // Override the ad flag in ytInitialPlayerResponse
    if (window.ytInitialPlayerResponse) {
      const resp = window.ytInitialPlayerResponse;
      if (resp.adPlacements) {
        resp.adPlacements = [];
        log('Cleared adPlacements');
      }
      if (resp.playerAds) {
        resp.playerAds = [];
        log('Cleared playerAds');
      }
    }
  }

  // --- Intercept fetch/XHR to block ad-related API calls ---
  const origFetch = window.fetch;
  window.fetch = function(...args) {
    const url = args[0] instanceof Request ? args[0].url : String(args[0]);
    if (isAdUrl(url)) {
      log('Blocked fetch:', url);
      return Promise.resolve(new Response('', { status: 204 }));
    }
    return origFetch.apply(this, args);
  };

  const origXHROpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function(method, url, ...rest) {
    if (isAdUrl(url)) {
      log('Blocked XHR:', url);
      // Point to a dummy URL that returns nothing
      return origXHROpen.call(this, method, 'data:text/plain,', ...rest);
    }
    return origXHROpen.call(this, method, url, ...rest);
  };

  function isAdUrl(url) {
    const adPatterns = [
      '/pagead/',
      '/ptracking',
      '/api/stats/ads',
      '/api/stats/atr',
      '/get_midroll_info',
      'googlesyndication.com',
      'doubleclick.net',
      'googleadservices.com',
      '/youtubei/v1/player/ad_break',
      '&ad_type=',
      '&adurl=',
      '/generate_204',
      '/ad_data_',
      'google.com/pagead',
      '/log_interaction',
      'play.google.com/log',
    ];
    return adPatterns.some(p => url.includes(p));
  }

  // --- MutationObserver: remove ads as they appear ---
  const observer = new MutationObserver(() => {
    skipVideoAd();
    removeAdElements();
  });

  // --- Initialize ---
  function init() {
    patchPlayerConfig();
    removeAdElements();
    skipVideoAd();

    observer.observe(document.documentElement, {
      childList: true,
      subtree: true
    });

    // Periodic check as fallback
    setInterval(() => {
      skipVideoAd();
      removeAdElements();
    }, CHECK_INTERVAL);

    // Unmute video after ad skip
    setInterval(() => {
      const video = document.querySelector('video');
      if (video && video.muted && !document.querySelector('.ad-showing')) {
        video.muted = false;
      }
    }, 1000);

    log('FreeIX YouTube Ad Blocker initialized');
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
