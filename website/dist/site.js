const GITHUB_URL = 'https://github.com/Turgo82/EmuLuna';
document.querySelectorAll('.github-link').forEach(link => { link.href = GITHUB_URL; });
const SUPPORT_URL = 'https://buymeacoffee.com/turgo';
document.querySelectorAll('.support-link').forEach(link => { link.href = SUPPORT_URL; });

// A small screenshot gallery with keyboard navigation and no dependencies.
const tabs = [...document.querySelectorAll('[role="tab"]')];
const captions = {
  gameplay: 'Launch from your library and keep the controls you need close at hand.',
  library: 'Your collection, arranged your way. Switch between a cover grid and a detailed list.',
  states: 'Remember the moment. Browse save previews by console, with box art to help you find your game.'
};
function selectTab(tab, focus = false) {
  tabs.forEach(item => {
    const selected = item === tab;
    item.setAttribute('aria-selected', String(selected));
    item.tabIndex = selected ? 0 : -1;
    document.getElementById(item.getAttribute('aria-controls')).hidden = !selected;
  });
  document.getElementById('screenshot-note').textContent = captions[tab.dataset.view];
  if (focus) tab.focus();
}
tabs.forEach((tab, index) => {
  tab.addEventListener('click', () => selectTab(tab));
  tab.addEventListener('keydown', event => {
    const keys = ['ArrowLeft', 'ArrowRight', 'Home', 'End'];
    if (!keys.includes(event.key)) return;
    event.preventDefault();
    let target = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 :
      (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
    selectTab(tabs[target], true);
  });
});

const dialog = document.getElementById('image-dialog');
document.querySelectorAll('[data-image]').forEach(button => {
  button.addEventListener('click', () => {
    const image = document.getElementById('dialog-image');
    image.src = button.dataset.image;
    image.alt = button.querySelector('img').alt;
    document.getElementById('image-dialog-title').textContent = button.dataset.caption;
    dialog.showModal();
    document.body.classList.add('dialog-open');
  });
});
dialog.querySelector('.dialog-close').addEventListener('click', () => dialog.close());
dialog.addEventListener('click', event => {
  const rect = dialog.getBoundingClientRect();
  if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) dialog.close();
});
dialog.addEventListener('close', () => document.body.classList.remove('dialog-open'));

// Content is visible by default if scripting is unavailable. Reduced-motion
// users keep the same layout with no entrance or ambient animation.
const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
if (!reduceMotion.matches && 'IntersectionObserver' in window) {
  const observer = new IntersectionObserver(entries => {
    entries.forEach(entry => {
      if (entry.isIntersecting) {
        entry.target.classList.add('is-visible');
        observer.unobserve(entry.target);
      }
    });
  }, { threshold: 0.08 });
  document.querySelectorAll('.reveal').forEach(element => {
    element.classList.add('will-reveal');
    observer.observe(element);
  });
  reduceMotion.addEventListener('change', event => {
    if (event.matches) {
      document.querySelectorAll('.will-reveal').forEach(element => element.classList.add('is-visible'));
      observer.disconnect();
    }
  });
}
