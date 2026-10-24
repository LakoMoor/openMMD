'use strict';

// Screenshot links remain ordinary full-size image links without JavaScript.
const lightbox = document.querySelector('.lightbox');
const image = lightbox?.querySelector('img');
const caption = document.querySelector('#lightbox-caption');
let lastTrigger;
if (lightbox && typeof lightbox.showModal === 'function') {
  document.querySelectorAll('.screenshot').forEach((link) => {
    link.addEventListener('click', (event) => {
      // Preserve native open-in-new-tab/window behavior.
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      lastTrigger = link;
      image.src = link.href;
      image.alt = link.querySelector('img')?.alt || 'Application screenshot';
      caption.textContent = link.dataset.caption || image.alt;
      document.body.classList.add('modal-open');
      lightbox.showModal();
    });
  });
  lightbox.querySelector('.lightbox-close').addEventListener('click', () => lightbox.close());
  lightbox.addEventListener('click', (event) => {
    if (event.target !== lightbox) return;
    const bounds = lightbox.getBoundingClientRect();
    if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) lightbox.close();
  });
  lightbox.addEventListener('close', () => {
    document.body.classList.remove('modal-open');
    lastTrigger?.focus({ preventScroll: true });
  });
}

const copyButton = document.querySelector('[data-copy]');
const copyStatus = document.querySelector('.copy-status');
if (!navigator.clipboard) {
  copyButton.hidden = true;
} else {
  let timeout;
  copyButton.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(document.querySelector('#setup-commands').textContent);
      copyStatus.textContent = ({ja:'コマンドをコピーしました。','zh-Hans':'已复制命令。'})[document.documentElement.lang] || 'Commands copied.';
    } catch {
      copyStatus.textContent = ({ja:'上のコマンドを選択してコピーしてください。','zh-Hans':'请选择上方命令并复制。'})[document.documentElement.lang] || 'Select the commands above to copy them.';
    }
    clearTimeout(timeout);
    timeout = setTimeout(() => { copyStatus.textContent = ''; }, 5000);
  });
}

// Without JavaScript the navigation and both screenshots remain available.
document.documentElement.classList.add('js');
const menuToggle = document.querySelector('.menu-toggle');
const navigation = document.querySelector('#main-nav');
function closeMenu() {
  navigation.classList.remove('is-open');
  menuToggle.setAttribute('aria-expanded', 'false');
}
menuToggle.addEventListener('click', () => {
  const open = menuToggle.getAttribute('aria-expanded') !== 'true';
  navigation.classList.toggle('is-open', open);
  menuToggle.setAttribute('aria-expanded', String(open));
});
navigation.querySelectorAll('a').forEach(link => link.addEventListener('click', closeMenu));
document.addEventListener('keydown', event => {
  if (event.key === 'Escape' && menuToggle.getAttribute('aria-expanded') === 'true') {
    closeMenu();
    menuToggle.focus();
  }
});
matchMedia('(min-width: 761px)').addEventListener('change', closeMenu);
const previewButtons = document.querySelectorAll('[data-preview]');
function selectPreview(name) {
  previewButtons.forEach(button => button.setAttribute('aria-pressed', String(button.dataset.preview === name)));
  document.querySelectorAll('[data-capture]').forEach(capture => { capture.hidden = capture.dataset.capture !== name; });
}
previewButtons.forEach(button => button.addEventListener('click', () => selectPreview(button.dataset.preview)));
selectPreview('browser');

// Explicit language links keep the selected language without redirecting visitors.
try { localStorage.setItem('openmmd-language', document.documentElement.lang); } catch {}
