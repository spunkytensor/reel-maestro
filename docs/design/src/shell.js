// Mockup shell: copies ?theme=light|dark, ?solid=1 and any variant flags onto <html data-*>,
// and injects the shared icon sprite. Pages build their own top bar.
(function () {
  const q = new URLSearchParams(location.search);
  const root = document.documentElement;
  root.dataset.theme = q.get('theme') || 'light';
  for (const [k, v] of q) root.dataset[k] = v;
  const sprite = `<svg style="display:none" xmlns="http://www.w3.org/2000/svg">
  <symbol id="i-play" viewBox="0 0 24 24"><path d="M8 5.5v13l10-6.5z" fill="currentColor" stroke="none"/></symbol>
  <symbol id="i-pause" viewBox="0 0 24 24"><rect x="6.5" y="5" width="3.5" height="14" rx="1" fill="currentColor" stroke="none"/><rect x="14" y="5" width="3.5" height="14" rx="1" fill="currentColor" stroke="none"/></symbol>
  <symbol id="i-cc" viewBox="0 0 24 24"><rect x="3" y="5" width="18" height="14" rx="3.5"/><path d="M10 10.5a2 2 0 1 0 0 3M17 10.5a2 2 0 1 0 0 3"/></symbol>
  <symbol id="i-vol" viewBox="0 0 24 24"><path d="M4 10v4h4l5 4V6L8 10z"/><path d="M16 9a4 4 0 0 1 0 6"/></symbol>
  <symbol id="i-full" viewBox="0 0 24 24"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/></symbol>
  <symbol id="i-undo" viewBox="0 0 24 24"><path d="M9 14 4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/></symbol>
  <symbol id="i-redo" viewBox="0 0 24 24"><path d="m15 14 5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h3"/></symbol>
  <symbol id="i-back" viewBox="0 0 24 24"><path d="m14 6-6 6 6 6"/></symbol>
  <symbol id="i-chev" viewBox="0 0 24 24"><path d="m8 10 4 4 4-4"/></symbol>
  <symbol id="i-x" viewBox="0 0 24 24"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/></symbol>
  <symbol id="i-check" viewBox="0 0 24 24"><path d="m5.5 12.5 4 4 9-9"/></symbol>
  <symbol id="i-search" viewBox="0 0 24 24"><circle cx="11" cy="11" r="6.5"/><path d="m19.5 19.5-3.2-3.2"/></symbol>
  <symbol id="i-plus" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></symbol>
  <symbol id="i-sparkle" viewBox="0 0 24 24"><path d="M12 4c.6 4 2.5 6 7 7-4.5 1-6.4 3-7 7-.6-4-2.5-6-7-7 4.5-1 6.4-3 7-7z" fill="currentColor" stroke="none"/></symbol>
  <symbol id="i-image" viewBox="0 0 24 24"><rect x="3.5" y="5" width="17" height="14" rx="3"/><circle cx="9" cy="10" r="1.6"/><path d="m20 15-4.5-4.5L8 18"/></symbol>
  <symbol id="i-film" viewBox="0 0 24 24"><rect x="3.5" y="4.5" width="17" height="15" rx="3"/><path d="M8 4.5v15M16 4.5v15M3.5 9.5H8M3.5 14.5H8M16 9.5h4.5M16 14.5h4.5"/></symbol>
  <symbol id="i-music" viewBox="0 0 24 24"><path d="M9.5 17.5V6.5l10-2v11"/><circle cx="7" cy="17.5" r="2.5"/><circle cx="17" cy="15.5" r="2.5"/></symbol>
  <symbol id="i-mic" viewBox="0 0 24 24"><rect x="9" y="3.5" width="6" height="10.5" rx="3"/><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v2.5"/></symbol>
  <symbol id="i-text" viewBox="0 0 24 24"><path d="M5 6.5h14M12 6.5v12M8.5 18.5h7"/></symbol>
  <symbol id="i-link" viewBox="0 0 24 24"><path d="M10 14a3.5 3.5 0 0 0 5 0l3-3a3.5 3.5 0 0 0-5-5l-1.2 1.2"/><path d="M14 10a3.5 3.5 0 0 0-5 0l-3 3a3.5 3.5 0 0 0 5 5l1.2-1.2"/></symbol>
  <symbol id="i-doc" viewBox="0 0 24 24"><path d="M6.5 3.5h7l4.5 4.5v12.5h-11.5z"/><path d="M13.5 3.5V8H18"/></symbol>
  <symbol id="i-download" viewBox="0 0 24 24"><path d="M12 4.5v10M8 11l4 4 4-4"/><path d="M5 18.5h14"/></symbol>
  <symbol id="i-copy" viewBox="0 0 24 24"><rect x="8.5" y="8.5" width="11" height="11" rx="2.5"/><path d="M15.5 8.5v-2a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2"/></symbol>
  <symbol id="i-clock" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8"/><path d="M12 8v4.5l3 1.5"/></symbol>
  <symbol id="i-warn" viewBox="0 0 24 24"><path d="M12 4 3.5 19h17z"/><path d="M12 10v4M12 16.5h.01"/></symbol>
  <symbol id="i-more" viewBox="0 0 24 24"><circle cx="6" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="18" cy="12" r="1.4" fill="currentColor" stroke="none"/></symbol>
  <symbol id="i-layers" viewBox="0 0 24 24"><path d="m12 4 8 4.5-8 4.5-8-4.5z"/><path d="m4 13.5 8 4.5 8-4.5"/></symbol>
  <symbol id="i-sun" viewBox="0 0 24 24"><circle cx="12" cy="12" r="3.5"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4"/></symbol>
  <symbol id="i-moon" viewBox="0 0 24 24"><path d="M19 14.5A7.5 7.5 0 0 1 9.5 5a7.5 7.5 0 1 0 9.5 9.5z"/></symbol>
  <symbol id="i-laptop" viewBox="0 0 24 24"><rect x="4.5" y="5.5" width="15" height="10" rx="2"/><path d="M3 18.5h18"/></symbol>
  <symbol id="i-folder" viewBox="0 0 24 24"><path d="M3.5 7a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v8.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/></symbol>
  <symbol id="i-refresh" viewBox="0 0 24 24"><path d="M19.5 11a7.5 7.5 0 0 0-13.5-3.5M4.5 13a7.5 7.5 0 0 0 13.5 3.5"/><path d="M19.5 4.5V11H13M4.5 19.5V13H11"/></symbol>
  <symbol id="i-share" viewBox="0 0 24 24"><path d="M12 4v11M8.5 7.5 12 4l3.5 3.5"/><path d="M6 12v6.5a1.5 1.5 0 0 0 1.5 1.5h9a1.5 1.5 0 0 0 1.5-1.5V12"/></symbol>
  </svg>`;
  document.body.insertAdjacentHTML('afterbegin', sprite);
  window.ic = (id, cls = '') => `<svg class="i ${cls}"><use href="#${id}"/></svg>`;
  window.brandMark = `<svg class="mark" viewBox="0 0 28 28"><circle cx="14" cy="14" r="9" fill="none" stroke="currentColor" stroke-width="2.2"/><circle cx="14" cy="14" r="2" fill="currentColor"/><circle cx="14" cy="8" r="1.5" fill="currentColor"/><circle cx="14" cy="20" r="1.5" fill="currentColor"/><circle cx="8" cy="14" r="1.5" fill="currentColor"/><circle cx="20" cy="14" r="1.5" fill="currentColor"/></svg>`;
})();
