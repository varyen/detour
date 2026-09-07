/* Параллакс фоновых слоёв. Двигаются только элементы с data-parallax="<доля>"
 * (0.1–0.2 — от прокрутки), продуктовые кадры не трогаются. Считаем в rAF по
 * данным IntersectionObserver: обработчик scroll только взводит флаг, layout не
 * дёргает. При prefers-reduced-motion скрипт не запускается вовсе. */
(function () {
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  var layers = Array.prototype.slice.call(document.querySelectorAll('[data-parallax]'));
  if (!layers.length) return;
  var active = new Set();
  var io = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) {
      var host = e.target;
      host.__layers.forEach(function (l) { e.isIntersecting ? active.add(l) : active.delete(l); });
    });
    dirty = true; tick();
  }, { rootMargin: '20% 0px' });
  layers.forEach(function (l) {
    var host = l.closest('[data-parallax-host]') || l.parentElement;
    if (!host.__layers) { host.__layers = []; io.observe(host); }
    host.__layers.push(l);
    l.__host = host; l.__k = parseFloat(l.getAttribute('data-parallax')) || 0.15;
    l.style.willChange = 'transform';
  });
  var dirty = false, raf = 0, vh = window.innerHeight;
  function tick() {
    if (raf) return;
    raf = requestAnimationFrame(function () {
      raf = 0; if (!dirty) return; dirty = false;
      active.forEach(function (l) {
        var r = l.__host.getBoundingClientRect();
        var centre = r.top + r.height / 2 - vh / 2;   /* 0 когда блок в центре экрана */
        var y = -centre * l.__k;
        l.style.transform = 'translate3d(0,' + y.toFixed(1) + 'px,0)';
      });
    });
  }
  window.addEventListener('scroll', function () { dirty = true; tick(); }, { passive: true });
  window.addEventListener('resize', function () { vh = window.innerHeight; dirty = true; tick(); });
  dirty = true; tick();
})();
