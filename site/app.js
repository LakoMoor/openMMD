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
      copyStatus.textContent = 'Commands copied.';
    } catch {
      copyStatus.textContent = 'Select the commands above to copy them.';
    }
    clearTimeout(timeout);
    timeout = setTimeout(() => { copyStatus.textContent = ''; }, 5000);
  });
}
